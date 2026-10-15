//! Secure update foundation: signed manifest, verified download, staged swap-on-next-start with rollback.
//!
//! Nothing in the signed channel runs unless an HTTPS manifest URL is configured
//! (`%LOCALAPPDATA%\Bowser\update-url.txt` or the compile-time `BOWSER_UPDATE_URL`). Installation
//! additionally requires an ECDSA P-256 public key embedded at compile time (`BOWSER_UPDATE_PUBKEY`,
//! 128 hex characters of X||Y). Without a key that channel is check-only. See `docs/updater.md`.
//!
//! Installed builds with no signed channel instead follow the GitHub pin
//! `stable/release.json` on `main`. That pin names one repository, tag and `Bowser.exe`. The
//! download URL is built from those fields, and the file is replaced only when its SHA-256
//! differs. Dev and benchmark launches never check or apply updates.
//!
//! Manifest (LF-only UTF-8, at most 16 KiB), signed over every byte before the final `signature=` line:
//!
//! ```text
//! bowser-update-manifest-v1
//! version=0.2.0
//! min_version=0.0.1
//! url=https://example.com/bowser-0.2.0.exe
//! sha256=<64 hex>
//! size=1234567
//! notes=one line of release notes
//! signature=<128 hex, r||s of ECDSA P-256 over SHA-256(body)>
//! ```

use std::{
    collections::HashSet,
    fmt, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use url::Url;
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0},
        System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject},
    },
    core::PCWSTR,
};

/// Compile-time public key (hex of 64-byte X||Y). Unset means the updater can never install.
pub const COMPILED_PUBLIC_KEY: Option<&str> = option_env!("BOWSER_UPDATE_PUBKEY");
/// Compile-time default manifest URL. Unset or empty means updates are disabled unless the user file exists.
pub const COMPILED_MANIFEST_URL: Option<&str> = option_env!("BOWSER_UPDATE_URL");

const MANIFEST_MAGIC: &str = "bowser-update-manifest-v1";
const MAX_MANIFEST_BYTES: usize = 16 * 1024;
const MAX_NOTES_CHARS: usize = 2000;
/// Upper bound for any update package, whatever the manifest claims.
pub const MAX_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;
const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;
const MAX_REDIRECTS: u32 = 5;
/// Crashed launches tolerated after a swap before the previous build is restored.
const MAX_TRIAL_ATTEMPTS: u32 = 3;
/// A process that survives this long after start counts as a healthy launch.
const HEALTHY_AFTER: Duration = Duration::from_secs(45);
/// Failed swaps of one staged file before it is discarded (each one may rehash up to 256 MB).
const MAX_APPLY_ATTEMPTS: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    ManifestTooLarge,
    Malformed(&'static str),
    BadSignature,
    InsecureUrl,
    Oversize,
    NotNewer,
    BelowMinVersion,
    Rejected,
    HashMismatch,
    SizeMismatch,
    Authenticode(i32),
    Network(String),
    Io(String),
    /// Automatic updates were turned off while a background check was downloading.
    SwitchedOff,
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ManifestTooLarge => f.write_str("manifest is too large"),
            Self::Malformed(why) => write!(f, "malformed manifest: {why}"),
            Self::BadSignature => f.write_str("manifest signature is missing or invalid"),
            Self::InsecureUrl => f.write_str("url is not a plain HTTPS url"),
            Self::Oversize => f.write_str("package exceeds the size limit"),
            Self::NotNewer => f.write_str("manifest version is not newer than the running build"),
            Self::BelowMinVersion => {
                f.write_str("running build is older than the manifest's min_version")
            }
            Self::Rejected => {
                f.write_str("this version was rolled back earlier and is not offered again")
            }
            Self::HashMismatch => f.write_str("sha256 mismatch"),
            Self::SizeMismatch => f.write_str("size mismatch"),
            Self::Authenticode(code) => write!(f, "Authenticode check failed ({code:#x})"),
            Self::Network(why) => write!(f, "network error: {why}"),
            Self::Io(why) => write!(f, "file error: {why}"),
            Self::SwitchedOff => f.write_str("automatic updates are off"),
        }
    }
}

impl std::error::Error for UpdateError {}

impl From<std::io::Error> for UpdateError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

// ---------------------------------------------------------------------------------------------
// Versions
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    /// Strict `major.minor.patch`: three non-empty ASCII digit groups of at most nine digits.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('.');
        let mut next = || {
            let part = parts.next()?;
            if part.is_empty() || part.len() > 9 || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            part.parse::<u32>().ok()
        };
        let version = Self(next()?, next()?, next()?);
        parts.next().is_none().then_some(version)
    }

    pub fn current() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Self(0, 0, 0))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

// ---------------------------------------------------------------------------------------------
// Hex
// ---------------------------------------------------------------------------------------------

fn hex_decode<const N: usize>(text: &str) -> Option<[u8; N]> {
    let bytes = text.as_bytes();
    if bytes.len() != N * 2 {
        return None;
    }
    let nibble = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    };
    let mut out = [0u8; N];
    for (i, pair) in bytes.chunks(2).enumerate() {
        out[i] = nibble(pair[0])? << 4 | nibble(pair[1])?;
    }
    Some(out)
}

pub fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 15) as usize] as char);
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Windows CNG: SHA-256 and ECDSA P-256 verification
// ---------------------------------------------------------------------------------------------

mod cng {
    use std::ptr::null_mut;

    use windows::{
        Win32::Security::Cryptography::{
            BCRYPT_ALG_HANDLE, BCRYPT_ECCKEY_BLOB, BCRYPT_ECCPUBLIC_BLOB,
            BCRYPT_ECDSA_P256_ALGORITHM, BCRYPT_ECDSA_PUBLIC_P256_MAGIC, BCRYPT_FLAGS,
            BCRYPT_HASH_HANDLE, BCRYPT_KEY_HANDLE, BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS,
            BCRYPT_SHA256_ALGORITHM, BCryptCloseAlgorithmProvider, BCryptCreateHash,
            BCryptDestroyHash, BCryptDestroyKey, BCryptFinishHash, BCryptHashData,
            BCryptImportKeyPair, BCryptOpenAlgorithmProvider, BCryptVerifySignature,
        },
        core::PCWSTR,
    };

    fn open(id: PCWSTR) -> Option<BCRYPT_ALG_HANDLE> {
        let mut alg = BCRYPT_ALG_HANDLE(null_mut());
        let status = unsafe {
            BCryptOpenAlgorithmProvider(
                &mut alg,
                id,
                PCWSTR::null(),
                BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0),
            )
        };
        (status.0 >= 0 && !alg.is_invalid()).then_some(alg)
    }

    /// Streaming SHA-256 backed by CNG.
    pub struct Sha256 {
        alg: BCRYPT_ALG_HANDLE,
        hash: BCRYPT_HASH_HANDLE,
    }

    impl Sha256 {
        pub fn new() -> Option<Self> {
            let alg = open(BCRYPT_SHA256_ALGORITHM)?;
            let mut hash = BCRYPT_HASH_HANDLE(null_mut());
            let status = unsafe { BCryptCreateHash(alg, &mut hash, None, None, 0) };
            if status.0 < 0 {
                let _ = unsafe { BCryptCloseAlgorithmProvider(alg, 0) };
                return None;
            }
            Some(Self { alg, hash })
        }

        pub fn update(&mut self, data: &[u8]) -> bool {
            data.is_empty() || unsafe { BCryptHashData(self.hash, data, 0) }.0 >= 0
        }

        pub fn finish(self) -> Option<[u8; 32]> {
            let mut out = [0u8; 32];
            let ok = unsafe { BCryptFinishHash(self.hash, &mut out, 0) }.0 >= 0;
            ok.then_some(out)
        }
    }

    impl Drop for Sha256 {
        fn drop(&mut self) {
            unsafe {
                let _ = BCryptDestroyHash(self.hash);
                let _ = BCryptCloseAlgorithmProvider(self.alg, 0);
            }
        }
    }

    pub fn sha256(data: &[u8]) -> Option<[u8; 32]> {
        let mut hasher = Sha256::new()?;
        hasher.update(data).then_some(())?;
        hasher.finish()
    }

    /// Verifies a P1363 (r||s) ECDSA P-256 signature over an already-computed SHA-256 digest.
    pub fn ecdsa_p256_verify(
        public_xy: &[u8; 64],
        digest: &[u8; 32],
        signature: &[u8; 64],
    ) -> bool {
        let Some(alg) = open(BCRYPT_ECDSA_P256_ALGORITHM) else {
            return false;
        };
        let header = BCRYPT_ECCKEY_BLOB {
            dwMagic: BCRYPT_ECDSA_PUBLIC_P256_MAGIC,
            cbKey: 32,
        };
        let mut blob = Vec::with_capacity(72);
        blob.extend_from_slice(&header.dwMagic.to_le_bytes());
        blob.extend_from_slice(&header.cbKey.to_le_bytes());
        blob.extend_from_slice(public_xy);
        let mut key = BCRYPT_KEY_HANDLE(null_mut());
        let imported =
            unsafe { BCryptImportKeyPair(alg, None, BCRYPT_ECCPUBLIC_BLOB, &mut key, &blob, 0) };
        let verified = imported.0 >= 0
            && unsafe { BCryptVerifySignature(key, None, digest, signature, BCRYPT_FLAGS(0)) }.0
                >= 0;
        unsafe {
            if imported.0 >= 0 {
                let _ = BCryptDestroyKey(key);
            }
            let _ = BCryptCloseAlgorithmProvider(alg, 0);
        }
        verified
    }

    /// Test-only: generates a P-256 key pair and signs digests, so tests exercise the real verifier.
    #[cfg(test)]
    pub struct TestKey {
        alg: BCRYPT_ALG_HANDLE,
        key: BCRYPT_KEY_HANDLE,
        pub public_xy: [u8; 64],
    }

    #[cfg(test)]
    impl TestKey {
        pub fn generate() -> Self {
            use windows::Win32::Security::Cryptography::{
                BCryptExportKey, BCryptFinalizeKeyPair, BCryptGenerateKeyPair,
            };
            let alg = open(BCRYPT_ECDSA_P256_ALGORITHM).expect("ecdsa provider");
            let mut key = BCRYPT_KEY_HANDLE(null_mut());
            unsafe {
                assert!(BCryptGenerateKeyPair(alg, &mut key, 256, 0).0 >= 0);
                assert!(BCryptFinalizeKeyPair(key, 0).0 >= 0);
            }
            let mut blob = [0u8; 72];
            let mut written = 0u32;
            unsafe {
                assert!(
                    BCryptExportKey(
                        key,
                        None,
                        BCRYPT_ECCPUBLIC_BLOB,
                        Some(&mut blob),
                        &mut written,
                        0
                    )
                    .0 >= 0
                );
            }
            assert_eq!(written, 72);
            let mut public_xy = [0u8; 64];
            public_xy.copy_from_slice(&blob[8..]);
            Self {
                alg,
                key,
                public_xy,
            }
        }

        pub fn sign(&self, digest: &[u8; 32]) -> [u8; 64] {
            use windows::Win32::Security::Cryptography::BCryptSignHash;
            let mut sig = [0u8; 64];
            let mut written = 0u32;
            unsafe {
                assert!(
                    BCryptSignHash(
                        self.key,
                        None,
                        digest,
                        Some(&mut sig),
                        &mut written,
                        BCRYPT_FLAGS(0)
                    )
                    .0 >= 0
                );
            }
            assert_eq!(written, 64);
            sig
        }
    }

    #[cfg(test)]
    impl Drop for TestKey {
        fn drop(&mut self) {
            unsafe {
                let _ = BCryptDestroyKey(self.key);
                let _ = BCryptCloseAlgorithmProvider(self.alg, 0);
            }
        }
    }
}

pub fn sha256(data: &[u8]) -> Result<[u8; 32], UpdateError> {
    cng::sha256(data).ok_or_else(|| UpdateError::Io("SHA-256 provider unavailable".into()))
}

/// Streams a file through SHA-256, returning (digest, length).
pub fn sha256_file(path: &Path) -> Result<([u8; 32], u64), UpdateError> {
    let mut file = fs::File::open(path)?;
    let mut hasher =
        cng::Sha256::new().ok_or_else(|| UpdateError::Io("SHA-256 provider unavailable".into()))?;
    let mut buffer = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if !hasher.update(&buffer[..read]) {
            return Err(UpdateError::Io("hashing failed".into()));
        }
    }
    let digest = hasher
        .finish()
        .ok_or_else(|| UpdateError::Io("hashing failed".into()))?;
    Ok((digest, total))
}

pub fn parse_public_key(hex: &str) -> Option<[u8; 64]> {
    hex_decode::<64>(hex.trim())
}

/// The compile-time key, if one is embedded and well-formed.
pub fn compiled_public_key() -> Option<[u8; 64]> {
    COMPILED_PUBLIC_KEY.and_then(parse_public_key)
}

// ---------------------------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub version: Version,
    pub min_version: Version,
    pub url: Url,
    pub sha256: [u8; 32],
    pub size: u64,
    pub notes: String,
}

/// A https URL with a host and no embedded credentials.
pub fn validate_https_url(text: &str) -> Result<Url, UpdateError> {
    let url = Url::parse(text).map_err(|_| UpdateError::InsecureUrl)?;
    if url.scheme() != "https"
        || url.host_str().is_none_or(str::is_empty)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(UpdateError::InsecureUrl);
    }
    Ok(url)
}

/// Resolves a redirect target against the current URL and requires it to stay on HTTPS.
pub fn resolve_redirect(current: &Url, location: &str) -> Result<Url, UpdateError> {
    let next = current
        .join(location)
        .map_err(|_| UpdateError::InsecureUrl)?;
    validate_https_url(next.as_str())
}

/// Splits `text` into the signed body and the signature from the final line.
fn split_signed(text: &str) -> Result<(&str, [u8; 64]), UpdateError> {
    if text.len() > MAX_MANIFEST_BYTES {
        return Err(UpdateError::ManifestTooLarge);
    }
    if text.contains('\r') || text.contains('\0') {
        return Err(UpdateError::Malformed("CR or NUL"));
    }
    let trimmed = text.strip_suffix('\n').unwrap_or(text);
    let start = trimmed.rfind('\n').map_or(0, |i| i + 1);
    let (body, last) = trimmed.split_at(start);
    let hex = last
        .strip_prefix("signature=")
        .ok_or(UpdateError::BadSignature)?;
    let signature = hex_decode::<64>(hex).ok_or(UpdateError::BadSignature)?;
    Ok((body, signature))
}

fn parse_body(body: &str) -> Result<Manifest, UpdateError> {
    let mut lines = body.lines();
    if lines.next() != Some(MANIFEST_MAGIC) {
        return Err(UpdateError::Malformed("missing magic line"));
    }
    let mut seen = HashSet::new();
    let (mut version, mut min_version, mut url, mut digest, mut size, mut notes) =
        (None, None, None, None, None, String::new());
    for line in lines {
        let (key, value) = line
            .split_once('=')
            .ok_or(UpdateError::Malformed("line without ="))?;
        if !seen.insert(key) {
            return Err(UpdateError::Malformed("duplicate key"));
        }
        match key {
            "version" => {
                version = Some(Version::parse(value).ok_or(UpdateError::Malformed("version"))?)
            }
            "min_version" => {
                min_version =
                    Some(Version::parse(value).ok_or(UpdateError::Malformed("min_version"))?)
            }
            "url" => url = Some(validate_https_url(value)?),
            "sha256" => {
                digest = Some(hex_decode::<32>(value).ok_or(UpdateError::Malformed("sha256"))?)
            }
            "size" => {
                size = Some(
                    (!value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()))
                        .then(|| value.parse::<u64>().ok())
                        .flatten()
                        .ok_or(UpdateError::Malformed("size"))?,
                )
            }
            "notes" => {
                if value.chars().count() > MAX_NOTES_CHARS {
                    return Err(UpdateError::Malformed("notes too long"));
                }
                notes = value.to_owned();
            }
            _ => return Err(UpdateError::Malformed("unknown key")),
        }
    }
    let size = size.ok_or(UpdateError::Malformed("size missing"))?;
    if size == 0 || size > MAX_PACKAGE_BYTES {
        return Err(UpdateError::Oversize);
    }
    Ok(Manifest {
        version: version.ok_or(UpdateError::Malformed("version missing"))?,
        min_version: min_version.ok_or(UpdateError::Malformed("min_version missing"))?,
        url: url.ok_or(UpdateError::Malformed("url missing"))?,
        sha256: digest.ok_or(UpdateError::Malformed("sha256 missing"))?,
        size,
        notes,
    })
}

/// Result of evaluating a manifest. `authentic` is true only when a public key was supplied and the
/// signature verified; installation must require it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluated {
    pub manifest: Manifest,
    pub authentic: bool,
}

/// Verifies (when a key is available), parses and applies the version policy.
/// With a key, an unsigned or tampered manifest is an error. Without a key the manifest is parsed for
/// information only and `authentic` is false.
pub fn evaluate(
    text: &str,
    public_key: Option<&[u8; 64]>,
    current: Version,
) -> Result<Evaluated, UpdateError> {
    let (body, signature) = split_signed(text)?;
    let authentic = match public_key {
        Some(key) => {
            let digest = sha256(body.as_bytes())?;
            if !cng::ecdsa_p256_verify(key, &digest, &signature) {
                return Err(UpdateError::BadSignature);
            }
            true
        }
        None => false,
    };
    let manifest = parse_body(body)?;
    if manifest.version <= current {
        return Err(UpdateError::NotNewer);
    }
    if current < manifest.min_version {
        return Err(UpdateError::BelowMinVersion);
    }
    Ok(Evaluated {
        manifest,
        authentic,
    })
}

/// Checks downloaded bytes' length and digest against the manifest.
pub fn verify_package(
    manifest: &Manifest,
    length: u64,
    digest: &[u8; 32],
) -> Result<(), UpdateError> {
    if length > MAX_PACKAGE_BYTES {
        return Err(UpdateError::Oversize);
    }
    if length != manifest.size {
        return Err(UpdateError::SizeMismatch);
    }
    // Constant-time-ish comparison is unnecessary for a public digest; plain equality is fine.
    if *digest != manifest.sha256 {
        return Err(UpdateError::HashMismatch);
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Schedule and configuration
// ---------------------------------------------------------------------------------------------

/// True when a check is due: never checked, a full day has passed, or the stored time is in the future.
pub fn check_due(now_secs: u64, last_check_secs: Option<u64>) -> bool {
    match last_check_secs {
        None => true,
        Some(last) if last > now_secs => true,
        Some(last) => now_secs - last >= CHECK_INTERVAL_SECS,
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[derive(Debug, Clone)]
pub struct Config {
    pub manifest_url: Url,
    pub public_key: Option<[u8; 64]>,
}

/// `update-url.txt` (first non-empty line) overrides the compile-time default. Anything that is not a
/// plain HTTPS URL disables updates.
pub fn load_config(root: &Path) -> Option<Config> {
    let from_file = fs::read(root.join("update-url.txt"))
        .ok()
        .filter(|bytes| bytes.len() <= 2048)
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| {
            text.lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .map(str::to_owned)
        });
    let text = from_file.or_else(|| COMPILED_MANIFEST_URL.map(str::to_owned))?;
    let manifest_url = validate_https_url(text.trim()).ok()?;
    Some(Config {
        manifest_url,
        public_key: compiled_public_key(),
    })
}

pub fn data_root() -> Option<PathBuf> {
    crate::profile::app_data_dir()
}

// ---------------------------------------------------------------------------------------------
// WinHTTP (HTTPS only, redirects followed manually and re-validated)
// ---------------------------------------------------------------------------------------------

pub(crate) mod http {
    use std::ffi::c_void;

    use url::{Position, Url};
    use windows::{
        Win32::Networking::WinHttp::{
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_DISABLE_REDIRECTS, WINHTTP_FLAG_SECURE,
            WINHTTP_OPTION_DISABLE_FEATURE, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_LOCATION,
            WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen,
            WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
            WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts,
        },
        core::{HSTRING, PCWSTR, w},
    };

    use super::{MAX_REDIRECTS, UpdateError, resolve_redirect};

    struct Handle(*mut c_void);
    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let _ = WinHttpCloseHandle(self.0);
                }
            }
        }
    }

    fn net(what: &str, error: windows::core::Error) -> UpdateError {
        UpdateError::Network(format!("{what}: {error}"))
    }

    fn status(request: *mut c_void) -> Result<u32, UpdateError> {
        let mut code = 0u32;
        let mut len = 4u32;
        unsafe {
            WinHttpQueryHeaders(
                request,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some(&mut code as *mut u32 as *mut c_void),
                &mut len,
                std::ptr::null_mut(),
            )
        }
        .map_err(|e| net("status", e))?;
        Ok(code)
    }

    fn location(request: *mut c_void) -> Result<String, UpdateError> {
        let mut len = 0u32;
        let _ = unsafe {
            WinHttpQueryHeaders(
                request,
                WINHTTP_QUERY_LOCATION,
                PCWSTR::null(),
                None,
                &mut len,
                std::ptr::null_mut(),
            )
        };
        if len == 0 || len > 8192 {
            return Err(UpdateError::Network(
                "redirect without usable Location".into(),
            ));
        }
        let mut buffer = vec![0u16; len as usize / 2 + 1];
        unsafe {
            WinHttpQueryHeaders(
                request,
                WINHTTP_QUERY_LOCATION,
                PCWSTR::null(),
                Some(buffer.as_mut_ptr() as *mut c_void),
                &mut len,
                std::ptr::null_mut(),
            )
        }
        .map_err(|e| net("location", e))?;
        Ok(String::from_utf16_lossy(&buffer[..len as usize / 2]))
    }

    /// GETs `start`, passing body chunks to `sink`. Fails on non-HTTPS redirects, more than five hops,
    /// a declared or actual body larger than `max_bytes`, or any non-200 final status.
    pub fn get(
        start: &Url,
        max_bytes: u64,
        sink: &mut dyn FnMut(&[u8]) -> Result<(), UpdateError>,
    ) -> Result<u64, UpdateError> {
        let agent = HSTRING::from(concat!("Bowser-Updater/", env!("CARGO_PKG_VERSION")));
        let session = Handle(unsafe {
            WinHttpOpen(
                &agent,
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            )
        });
        if session.0.is_null() {
            return Err(UpdateError::Network("WinHttpOpen failed".into()));
        }
        unsafe { WinHttpSetTimeouts(session.0, 10_000, 10_000, 30_000, 30_000) }
            .map_err(|e| net("timeouts", e))?;
        let mut url = start.clone();
        for _hop in 0..=MAX_REDIRECTS {
            let host = HSTRING::from(url.host_str().unwrap_or_default());
            let port = url.port_or_known_default().unwrap_or(443);
            let object = HSTRING::from(&url[Position::BeforePath..Position::AfterQuery]);
            let connection = Handle(unsafe { WinHttpConnect(session.0, &host, port, 0) });
            if connection.0.is_null() {
                return Err(UpdateError::Network("connect failed".into()));
            }
            let request = Handle(unsafe {
                WinHttpOpenRequest(
                    connection.0,
                    w!("GET"),
                    &object,
                    PCWSTR::null(),
                    PCWSTR::null(),
                    std::ptr::null(),
                    WINHTTP_FLAG_SECURE,
                )
            });
            if request.0.is_null() {
                return Err(UpdateError::Network("open request failed".into()));
            }
            unsafe {
                WinHttpSetOption(
                    Some(request.0 as *const c_void),
                    WINHTTP_OPTION_DISABLE_FEATURE,
                    Some(&WINHTTP_DISABLE_REDIRECTS.to_ne_bytes()),
                )
            }
            .map_err(|e| net("disable redirects", e))?;
            unsafe { WinHttpSendRequest(request.0, None, None, 0, 0, 0) }
                .map_err(|e| net("send", e))?;
            unsafe { WinHttpReceiveResponse(request.0, std::ptr::null_mut()) }
                .map_err(|e| net("receive", e))?;
            match status(request.0)? {
                301 | 302 | 303 | 307 | 308 => {
                    url = resolve_redirect(&url, &location(request.0)?)?;
                    continue;
                }
                200 => {}
                other => return Err(UpdateError::Network(format!("HTTP status {other}"))),
            }
            let mut declared = 0u32;
            let mut len = 4u32;
            let has_length = unsafe {
                WinHttpQueryHeaders(
                    request.0,
                    windows::Win32::Networking::WinHttp::WINHTTP_QUERY_CONTENT_LENGTH
                        | WINHTTP_QUERY_FLAG_NUMBER,
                    PCWSTR::null(),
                    Some(&mut declared as *mut u32 as *mut c_void),
                    &mut len,
                    std::ptr::null_mut(),
                )
            }
            .is_ok();
            if has_length && u64::from(declared) > max_bytes {
                return Err(UpdateError::Oversize);
            }
            let mut buffer = vec![0u8; 64 * 1024];
            let mut total = 0u64;
            loop {
                let mut read = 0u32;
                unsafe {
                    WinHttpReadData(
                        request.0,
                        buffer.as_mut_ptr() as *mut c_void,
                        buffer.len() as u32,
                        &mut read,
                    )
                }
                .map_err(|e| net("read", e))?;
                if read == 0 {
                    return Ok(total);
                }
                total += u64::from(read);
                if total > max_bytes {
                    return Err(UpdateError::Oversize);
                }
                sink(&buffer[..read as usize])?;
            }
        }
        Err(UpdateError::Network("too many redirects".into()))
    }
}

// ---------------------------------------------------------------------------------------------
// Authenticode (bonus layer)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Authenticode {
    Valid,
    Unsigned,
    Invalid(i32),
}

const TRUST_E_NOSIGNATURE: i32 = 0x800B0100_u32 as i32;

/// WinVerifyTrust on a file, no UI, no revocation network traffic.
pub fn authenticode(path: &Path) -> Authenticode {
    use std::os::windows::ffi::OsStrExt;

    use windows::{
        Win32::{
            Foundation::HWND,
            Security::WinTrust::{
                WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_FILE_INFO,
                WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY,
                WTD_UI_NONE, WinVerifyTrust,
            },
        },
        core::PCWSTR,
    };

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(wide.as_ptr()),
        ..Default::default()
    };
    let mut data = WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        dwStateAction: WTD_STATEACTION_VERIFY,
        ..Default::default()
    };
    data.Anonymous.pFile = &mut file;
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let no_ui = HWND(-1isize as *mut std::ffi::c_void);
    let result = unsafe { WinVerifyTrust(no_ui, &mut action, &mut data as *mut _ as *mut _) };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    unsafe {
        WinVerifyTrust(no_ui, &mut action, &mut data as *mut _ as *mut _);
    }
    match result {
        0 => Authenticode::Valid,
        TRUST_E_NOSIGNATURE => Authenticode::Unsigned,
        other => Authenticode::Invalid(other),
    }
}

// ---------------------------------------------------------------------------------------------
// Staging, swap and rollback
// ---------------------------------------------------------------------------------------------

fn write_atomic(path: &Path, contents: &str) -> Result<(), UpdateError> {
    let temp = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&temp)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
    }
    fs::rename(&temp, path)?;
    Ok(())
}

fn read_kv(path: &Path) -> Option<Vec<(String, String)>> {
    let bytes = fs::read(path).ok().filter(|b| b.len() <= 4096)?;
    let text = String::from_utf8(bytes).ok()?;
    Some(
        text.lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
            .collect(),
    )
}

fn kv<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

/// Paths used by the swap, derived from the running executable and the data root.
#[derive(Debug, Clone)]
pub struct Layout {
    pub exe: PathBuf,
    pub updates: PathBuf,
}

impl Layout {
    pub fn new(exe: PathBuf, root: &Path) -> Self {
        Self {
            exe,
            updates: root.join("updates"),
        }
    }
    fn sibling(&self, name: &str) -> PathBuf {
        self.exe.with_file_name(name)
    }
    pub fn old_exe(&self) -> PathBuf {
        self.sibling("bowser.old.exe")
    }
    fn bad_exe(&self) -> PathBuf {
        self.sibling("bowser.bad.exe")
    }
    fn incoming_exe(&self) -> PathBuf {
        self.sibling("bowser.new.exe")
    }
    pub fn staged_exe(&self, version: Version) -> PathBuf {
        self.updates.join(format!("bowser-{version}.exe"))
    }
    fn download_file(&self, version: Version) -> PathBuf {
        self.updates.join(format!("bowser-{version}.download"))
    }
    fn pending(&self) -> PathBuf {
        self.updates.join("pending.txt")
    }
    fn trial(&self) -> PathBuf {
        self.updates.join("trial.txt")
    }
    fn rejected(&self) -> PathBuf {
        self.updates.join("rejected.txt")
    }
    fn last_check(&self) -> PathBuf {
        self.updates.join("last-check.txt")
    }
    fn status_file(&self) -> PathBuf {
        self.updates.join("status.txt")
    }
    fn pin_last_check(&self) -> PathBuf {
        self.updates.join("pin-last-check.txt")
    }
    fn rejected_shas(&self) -> PathBuf {
        self.updates.join("rejected-sha256.txt")
    }
}

fn pending_text(version: Version, digest: &[u8; 32], size: u64, attempts: u32) -> String {
    format!(
        "version={version}\nsha256={}\nsize={size}\nattempts={attempts}\n",
        hex_encode(digest)
    )
}

/// Writes the staged-update record after the package has been fully verified.
fn write_pending(layout: &Layout, manifest: &Manifest) -> Result<(), UpdateError> {
    write_atomic(
        &layout.pending(),
        &pending_text(manifest.version, &manifest.sha256, manifest.size, 0),
    )
}

/// Cross-process lock around the swap, rollback and trial bookkeeping. It is a named mutex, so the
/// OS releases it if the holder dies. Two Bowser processes starting together never both swap.
struct SwapLock(HANDLE);

impl SwapLock {
    /// Waits up to `wait_ms` for the lock; `None` when another process holds it.
    fn acquire(layout: &Layout, wait_ms: u32) -> Option<Self> {
        let key = layout.updates.to_string_lossy();
        // FNV-1a: stable across builds, so every Bowser build locks the same name.
        let hash = key.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        });
        let name: Vec<u16> = format!("Local\\BowserUpdate-{hash:016x}")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: `name` is NUL-terminated and outlives the call; the returned handle is owned by us.
        let handle = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }.ok()?;
        // SAFETY: `handle` is a valid mutex handle created above.
        let wait = unsafe { WaitForSingleObject(handle, wait_ms) };
        if wait == WAIT_OBJECT_0 || wait == WAIT_ABANDONED {
            Some(Self(handle))
        } else {
            // SAFETY: we own `handle` and did not acquire the mutex.
            let _ = unsafe { CloseHandle(handle) };
            None
        }
    }
}

impl Drop for SwapLock {
    fn drop(&mut self) {
        // SAFETY: the handle is owned by this guard and the mutex is held by this thread.
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyResult {
    /// Nothing staged, or the staged update was obsolete and was discarded.
    Nothing,
    /// The new build is now in place; the caller must relaunch.
    Swapped(Version),
    /// A staged update exists but could not be applied; the running build is untouched.
    Failed(String),
}

fn discard_staged(layout: &Layout, version: Option<Version>) {
    let _ = fs::remove_file(layout.pending());
    if let Some(version) = version {
        let _ = fs::remove_file(layout.staged_exe(version));
    }
}

/// Whether the next start installs the staged `version` of a pending record rather than discarding
/// it: a newer build, or a same-version build that a GitHub pin replaces by hash.
fn installs_over(version: Version, pending: &[(String, String)], running: Version) -> bool {
    version > running || (version == running && kv(pending, "pin") == Some("sha256"))
}

/// Applies a staged update, re-verifying the staged file against the pending record first.
/// Swap order: copy staged next to the exe, rename exe to `bowser.old.exe`, rename the copy into place.
/// On any failure after the first rename the old exe is restored. A failed attempt is counted in
/// `pending.txt`; after [`MAX_APPLY_ATTEMPTS`] the staged file is discarded.
/// Callers must hold the [`SwapLock`].
pub fn apply_pending(layout: &Layout, current: Version) -> ApplyResult {
    let Some(pairs) = read_kv(&layout.pending()) else {
        return ApplyResult::Nothing;
    };
    let version = kv(&pairs, "version").and_then(Version::parse);
    let digest = kv(&pairs, "sha256").and_then(hex_decode::<32>);
    let size = kv(&pairs, "size").and_then(|s| s.parse::<u64>().ok());
    let attempts = kv(&pairs, "attempts")
        .and_then(|a| a.parse::<u32>().ok())
        .unwrap_or(0);
    let (Some(version), Some(digest), Some(size)) = (version, digest, size) else {
        discard_staged(layout, None);
        return ApplyResult::Failed("pending record is malformed".into());
    };
    if !installs_over(version, &pairs, current) {
        discard_staged(layout, Some(version));
        return ApplyResult::Nothing;
    }
    // Only a pin record may install the running version again (a rebuilt `Bowser.exe`).
    let pin_replacement = version == current;
    if pin_replacement {
        match sha256_file(&layout.exe) {
            Ok((running, _)) if running == digest => {
                discard_staged(layout, Some(version));
                return ApplyResult::Nothing;
            }
            Ok(_) => {}
            Err(error) => {
                return ApplyResult::Failed(format!("cannot hash the running build: {error}"));
            }
        }
    }
    let staged = layout.staged_exe(version);
    let fail = |message: String| {
        record_failed_apply(
            layout,
            version,
            &digest,
            size,
            attempts,
            message,
            pin_replacement,
        )
    };
    // Cheap size check first, so a wrong-sized file is never hashed.
    match fs::metadata(&staged) {
        Ok(meta) if meta.len() == size && size <= MAX_PACKAGE_BYTES => {}
        Ok(_) => {
            discard_staged(layout, Some(version));
            return ApplyResult::Failed("staged file does not match its record; discarded".into());
        }
        Err(error) => {
            discard_staged(layout, Some(version));
            return ApplyResult::Failed(format!("staged file unreadable: {error}"));
        }
    }
    match sha256_file(&staged) {
        Ok((actual, length)) if actual == digest && length == size => {}
        Ok(_) => {
            discard_staged(layout, Some(version));
            return ApplyResult::Failed("staged file does not match its record; discarded".into());
        }
        Err(error) => {
            discard_staged(layout, Some(version));
            return ApplyResult::Failed(format!("staged file unreadable: {error}"));
        }
    }
    let (old, incoming) = (layout.old_exe(), layout.incoming_exe());
    let _ = fs::remove_file(&incoming);
    if let Err(error) = fs::copy(&staged, &incoming) {
        let _ = fs::remove_file(&incoming);
        return fail(format!("cannot copy next to executable: {error}"));
    }
    if old.exists() {
        if let Err(error) = fs::remove_file(&old) {
            let _ = fs::remove_file(&incoming);
            return fail(format!("cannot clear previous rollback copy: {error}"));
        }
    }
    if let Err(error) = fs::rename(&layout.exe, &old) {
        let _ = fs::remove_file(&incoming);
        return fail(format!("cannot move running executable aside: {error}"));
    }
    if let Err(error) = fs::rename(&incoming, &layout.exe) {
        let _ = fs::rename(&old, &layout.exe);
        let _ = fs::remove_file(&incoming);
        return fail(format!("cannot move new executable into place: {error}"));
    }
    let _ = write_trial(
        layout,
        Trial {
            version,
            attempts: 0,
            running: false,
        },
    );
    discard_staged(layout, Some(version));
    ApplyResult::Swapped(version)
}

/// Counts one failed swap in `pending.txt`, or discards the staged file once the limit is reached.
fn record_failed_apply(
    layout: &Layout,
    version: Version,
    digest: &[u8; 32],
    size: u64,
    attempts: u32,
    message: String,
    pin_replacement: bool,
) -> ApplyResult {
    let attempts = attempts.saturating_add(1);
    if attempts >= MAX_APPLY_ATTEMPTS {
        discard_staged(layout, Some(version));
        return ApplyResult::Failed(format!("{message}; discarded after {attempts} attempts"));
    }
    let mut text = pending_text(version, digest, size, attempts);
    if pin_replacement {
        text.push_str("pin=sha256\n");
    }
    let _ = write_atomic(&layout.pending(), &text);
    ApplyResult::Failed(message)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trial {
    pub version: Version,
    /// Crashed launches so far.
    pub attempts: u32,
    /// A launch of this version has started and has not yet exited cleanly or been marked healthy.
    /// Seen at the next start, it means that launch crashed.
    pub running: bool,
}

fn read_trial(layout: &Layout) -> Option<Trial> {
    let pairs = read_kv(&layout.trial())?;
    Some(Trial {
        version: kv(&pairs, "version").and_then(Version::parse)?,
        attempts: kv(&pairs, "attempts")
            .and_then(|a| a.parse().ok())
            .unwrap_or(MAX_TRIAL_ATTEMPTS),
        running: kv(&pairs, "running") == Some("1"),
    })
}

fn write_trial(layout: &Layout, trial: Trial) -> Result<(), UpdateError> {
    write_atomic(
        &layout.trial(),
        &format!(
            "version={}\nattempts={}\nrunning={}\n",
            trial.version,
            trial.attempts,
            u8::from(trial.running)
        ),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrialAction {
    /// No trial in progress.
    None,
    /// Record a new launch with this many crashed launches so far.
    Count(u32),
    /// The trial build keeps crashing; restore the previous build.
    Rollback,
    /// The trial record is stale (a different build is running); clear it.
    Clear,
}

/// Pure rollback decision for each start. Only crashed launches count: a launch whose process
/// exited cleanly (see [`note_clean_exit`]) or survived [`HEALTHY_AFTER`] clears the `running`
/// flag, so quick clean closes never lead to a rollback.
pub fn decide_trial(trial: Option<Trial>, running: Version) -> TrialAction {
    match trial {
        None => TrialAction::None,
        Some(t) if t.version != running => TrialAction::Clear,
        Some(t) => {
            let crashed = t.attempts.saturating_add(u32::from(t.running));
            if crashed >= MAX_TRIAL_ATTEMPTS {
                TrialAction::Rollback
            } else {
                TrialAction::Count(crashed)
            }
        }
    }
}

/// Restores `bowser.old.exe` over the current executable and remembers the rejected version.
/// Callers must hold the [`SwapLock`].
pub fn rollback(layout: &Layout, bad: Version) -> Result<(), UpdateError> {
    remember_rejected_build(layout);
    let old = layout.old_exe();
    let _ = fs::remove_file(layout.trial());
    let _ = write_atomic(&layout.rejected(), &format!("version={bad}\n"));
    if !old.exists() {
        return Err(UpdateError::Io("no rollback copy available".into()));
    }
    let side = layout.bad_exe();
    let _ = fs::remove_file(&side);
    fs::rename(&layout.exe, &side)?;
    if let Err(error) = fs::rename(&old, &layout.exe) {
        let _ = fs::rename(&side, &layout.exe);
        return Err(error.into());
    }
    Ok(())
}

fn rejected_version(layout: &Layout) -> Option<Version> {
    kv(&read_kv(&layout.rejected())?, "version").and_then(Version::parse)
}

#[derive(Debug, PartialEq, Eq)]
pub enum StartupAction {
    Continue,
    /// A different build is now in place; relaunch the executable and exit this process.
    Relaunch,
}

/// Called first thing in `main`. Cheap when no update state exists (one missing-file probe).
/// Benchmark launches (any `--benchmark-*` flag) and `--dev` never touch update state.
pub fn startup(benchmark: bool, dev: bool) -> StartupAction {
    if benchmark || dev {
        return StartupAction::Continue;
    }
    let (Some(root), Ok(exe)) = (data_root(), std::env::current_exe()) else {
        return StartupAction::Continue;
    };
    let layout = Layout::new(exe, &root);
    if !layout.updates.is_dir() {
        return StartupAction::Continue;
    }
    // Never wait here: if another process is swapping, this one simply starts the build it has.
    // The exception is a "Restart to update" start, whose parent may still be letting go.
    let wait_ms = if std::env::var_os(RESTART_ENV).is_some() {
        // SAFETY: called first thing in `main`, before any other thread exists.
        unsafe { std::env::remove_var(RESTART_ENV) };
        5_000
    } else {
        0
    };
    let Some(_lock) = SwapLock::acquire(&layout, wait_ms) else {
        return StartupAction::Continue;
    };
    let running = Version::current();
    if let TrialAction::Rollback = begin_launch(&layout, running) {
        return match rollback(&layout, running) {
            Ok(()) => StartupAction::Relaunch,
            Err(_) => StartupAction::Continue,
        };
    }
    match apply_pending(&layout, running) {
        ApplyResult::Swapped(_) => StartupAction::Relaunch,
        ApplyResult::Nothing | ApplyResult::Failed(_) => StartupAction::Continue,
    }
}

/// Records the start of a launch of `running` in the trial record and returns the decision.
/// Callers must hold the [`SwapLock`].
fn begin_launch(layout: &Layout, running: Version) -> TrialAction {
    let action = decide_trial(read_trial(layout), running);
    match action {
        TrialAction::None | TrialAction::Rollback => {}
        TrialAction::Clear => {
            let _ = fs::remove_file(layout.trial());
        }
        TrialAction::Count(attempts) => {
            let _ = write_trial(
                layout,
                Trial {
                    version: running,
                    attempts,
                    running: true,
                },
            );
        }
    }
    action
}

/// Called on a normal shutdown. The launch is then not counted as a crash, so quick clean closes
/// never lead to a rollback. Best effort: if the lock is busy the launch is simply counted.
pub fn note_clean_exit() {
    let (Some(root), Ok(exe)) = (data_root(), std::env::current_exe()) else {
        return;
    };
    let layout = Layout::new(exe, &root);
    if !layout.updates.is_dir() {
        return;
    }
    let Some(_lock) = SwapLock::acquire(&layout, 1_000) else {
        return;
    };
    mark_clean_exit(&layout, Version::current());
}

/// Clears the `running` flag of this build's trial record. Callers must hold the [`SwapLock`].
fn mark_clean_exit(layout: &Layout, running: Version) {
    if let Some(trial) = read_trial(layout)
        && trial.version == running
        && trial.running
    {
        let _ = write_trial(
            layout,
            Trial {
                running: false,
                ..trial
            },
        );
    }
}

/// Ends the trial once this build has survived [`HEALTHY_AFTER`]. Only removes a record for the
/// build that is actually running.
fn mark_healthy(layout: &Layout, running: Version) {
    let Some(_lock) = SwapLock::acquire(layout, 5_000) else {
        return;
    };
    if read_trial(layout).is_some_and(|trial| trial.version == running) {
        let _ = fs::remove_file(layout.trial());
    }
}

/// Spawns the (new or restored) executable with the same arguments. Returns false on failure.
pub fn relaunch() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    std::process::Command::new(exe)
        .args(std::env::args_os().skip(1))
        .spawn()
        .is_ok()
}

// ---------------------------------------------------------------------------------------------
// Background check
// ---------------------------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
pub enum CheckOutcome {
    UpToDate,
    /// A newer manifest exists but cannot be installed (no embedded key); nothing was downloaded.
    AvailableUnverified(Version),
    /// Downloaded, verified and staged; applied on the next start.
    Staged(Version),
}

/// One full check: fetch and evaluate the manifest, then (only when authentic) download, verify, stage.
/// With `gate`, see [`publish_staged`].
pub fn check_and_stage(
    config: &Config,
    layout: &Layout,
    current: Version,
    gate: Option<&Path>,
) -> Result<CheckOutcome, UpdateError> {
    fs::create_dir_all(&layout.updates)?;
    let mut text = Vec::new();
    http::get(
        &config.manifest_url,
        MAX_MANIFEST_BYTES as u64,
        &mut |chunk| {
            text.extend_from_slice(chunk);
            Ok(())
        },
    )?;
    let text = String::from_utf8(text).map_err(|_| UpdateError::Malformed("not UTF-8"))?;
    let evaluated = match evaluate(&text, config.public_key.as_ref(), current) {
        Ok(evaluated) => evaluated,
        Err(UpdateError::NotNewer) => return Ok(CheckOutcome::UpToDate),
        Err(error) => return Err(error),
    };
    let manifest = evaluated.manifest;
    if rejected_version(layout) == Some(manifest.version) {
        return Err(UpdateError::Rejected);
    }
    if !evaluated.authentic {
        return Ok(CheckOutcome::AvailableUnverified(manifest.version));
    }
    let partial = layout.download_file(manifest.version);
    let result = download_and_verify(&manifest, &partial);
    if let Err(error) = result {
        let _ = fs::remove_file(&partial);
        return Err(error);
    }
    publish_staged(layout, &manifest, &partial, write_pending, gate)
}

/// Moves a verified download into place and writes its pending record with `write`, under the lock
/// so a starting process never pairs them half-way through. With `gate` (a data root), the
/// automatic-update switch is read again under the same lock, so a background check never stages a
/// build after the switch was turned off.
fn publish_staged(
    layout: &Layout,
    manifest: &Manifest,
    partial: &Path,
    write: fn(&Layout, &Manifest) -> Result<(), UpdateError>,
    gate: Option<&Path>,
) -> Result<CheckOutcome, UpdateError> {
    let _lock =
        SwapLock::acquire(layout, 5_000).ok_or(UpdateError::Io("update lock is busy".into()))?;
    if gate.is_some_and(|root| !auto_update_in(root)) {
        let _ = fs::remove_file(partial);
        return Err(UpdateError::SwitchedOff);
    }
    let staged = layout.staged_exe(manifest.version);
    let _ = fs::remove_file(&staged);
    fs::rename(partial, &staged)?;
    write(layout, manifest)?;
    Ok(CheckOutcome::Staged(manifest.version))
}

fn download_and_verify(manifest: &Manifest, partial: &Path) -> Result<(), UpdateError> {
    let mut file = fs::File::create(partial)?;
    let mut hasher =
        cng::Sha256::new().ok_or_else(|| UpdateError::Io("SHA-256 provider unavailable".into()))?;
    let length = http::get(
        &manifest.url,
        manifest.size.min(MAX_PACKAGE_BYTES),
        &mut |chunk| {
            file.write_all(chunk)?;
            if hasher.update(chunk) {
                Ok(())
            } else {
                Err(UpdateError::Io("hashing failed".into()))
            }
        },
    )?;
    file.sync_all()?;
    drop(file);
    let digest = hasher
        .finish()
        .ok_or_else(|| UpdateError::Io("hashing failed".into()))?;
    verify_package(manifest, length, &digest)?;
    // A signed-but-untrusted binary is refused; an unsigned one relies on the manifest signature alone.
    match authenticode(partial) {
        Authenticode::Invalid(code) => Err(UpdateError::Authenticode(code)),
        Authenticode::Valid | Authenticode::Unsigned => Ok(()),
    }
}

/// The installed browser follows this pin when no signed update channel is configured.
const GITHUB_PIN_URL: &str =
    "https://raw.githubusercontent.com/grantcharlie1000-tech/Bowser/main/stable/release.json";
const GITHUB_REPOSITORY: &str = "grantcharlie1000-tech/Bowser";
const GITHUB_ASSET: &str = "Bowser.exe";
/// Bump this when publishing `stable/release.json`. A pin with a lower revision is ignored,
/// so a cached copy of an older pin cannot replace the build that contains this number.
const PIN_REVISION: u32 = 18;
/// GitHub pin checks are quieter than a launch, but a restarted browser still notices a release
/// within the hour.
const PIN_CHECK_INTERVAL_SECS: u64 = 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubPin {
    pub version: Version,
    pub revision: u32,
    pub tag: String,
    pub repository: String,
    pub asset: String,
    pub sha256: [u8; 32],
    pub bytes: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PinDecision {
    UpToDate,
    Replace(Url),
}

/// Reads the flat `stable/release.json` object. Extra whitespace is ignored. Unknown keys are ignored
/// so a pin can grow without breaking older browsers. The download URL is not taken from the file.
pub fn parse_github_pin(text: &str) -> Result<GithubPin, UpdateError> {
    if text.len() > MAX_MANIFEST_BYTES {
        return Err(UpdateError::ManifestTooLarge);
    }
    if text.contains('\0') {
        return Err(UpdateError::Malformed("NUL"));
    }
    let version = json_string(text, "version")
        .and_then(|value| Version::parse(&value))
        .ok_or(UpdateError::Malformed("version"))?;
    let tag = json_string(text, "tag").ok_or(UpdateError::Malformed("tag"))?;
    let repository =
        json_string(text, "repository").ok_or(UpdateError::Malformed("repository"))?;
    let asset = json_string(text, "asset").ok_or(UpdateError::Malformed("asset"))?;
    let sha_text = json_string(text, "sha256").ok_or(UpdateError::Malformed("sha256"))?;
    let sha256 = hex_decode::<32>(sha_text.trim()).ok_or(UpdateError::Malformed("sha256"))?;
    let bytes = json_u64(text, "bytes").ok_or(UpdateError::Malformed("bytes"))?;
    if bytes == 0 || bytes > MAX_PACKAGE_BYTES {
        return Err(UpdateError::Oversize);
    }
    let revision = json_u64(text, "revision").unwrap_or(0);
    if revision > u64::from(u32::MAX) {
        return Err(UpdateError::Malformed("revision"));
    }
    Ok(GithubPin {
        version,
        revision: revision as u32,
        tag,
        repository,
        asset,
        sha256,
        bytes,
    })
}

/// Decides whether the running build should be replaced by `pin`. The pin cannot name another
/// repository or asset, and it cannot move the browser to an older version. `running_sha` hashes
/// the running executable; it is called only when everything cheaper has passed and the digests
/// really have to be compared.
pub fn decide_github_pin(
    pin: &GithubPin,
    running_version: Version,
    running_sha: impl FnOnce() -> Result<[u8; 32], UpdateError>,
    rejected: &HashSet<[u8; 32]>,
) -> Result<PinDecision, UpdateError> {
    if pin.repository != GITHUB_REPOSITORY || pin.asset != GITHUB_ASSET {
        return Err(UpdateError::Malformed("unexpected release"));
    }
    if !pin.tag.starts_with('v')
        || pin.tag.len() > 64
        || !pin
            .tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(UpdateError::Malformed("tag"));
    }
    if pin.version < running_version || pin.revision < PIN_REVISION {
        return Err(UpdateError::NotNewer);
    }
    if rejected.contains(&pin.sha256) {
        return Err(UpdateError::Rejected);
    }
    if running_sha()? == pin.sha256 {
        return Ok(PinDecision::UpToDate);
    }
    let url = validate_https_url(&format!(
        "https://github.com/{}/releases/download/{}/{}",
        pin.repository, pin.tag, pin.asset
    ))?;
    Ok(PinDecision::Replace(url))
}

fn json_string(text: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\"");
    let mut rest = text.get(text.find(&pattern)? + pattern.len()..)?;
    rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let mut chars = rest.strip_prefix('"')?.chars();
    let mut out = String::new();
    while let Some(character) = chars.next() {
        match character {
            '"' => return Some(out),
            '\\' => out.push(chars.next()?),
            other => out.push(other),
        }
    }
    None
}

fn json_u64(text: &str, key: &str) -> Option<u64> {
    let pattern = format!("\"{key}\"");
    let mut rest = text.get(text.find(&pattern)? + pattern.len()..)?;
    rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let digits: String = rest
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    if digits.is_empty() || digits.len() > 12 {
        return None;
    }
    digits.parse().ok()
}

fn remember_rejected_build(layout: &Layout) {
    let Ok((digest, _)) = sha256_file(&layout.exe) else {
        return;
    };
    let path = layout.rejected_shas();
    let mut text = fs::read_to_string(&path).unwrap_or_default();
    if text.len() > 64 * 1024 {
        return;
    }
    let hex = hex_encode(&digest);
    if text
        .lines()
        .any(|line| line.trim().eq_ignore_ascii_case(&hex))
    {
        return;
    }
    text.push_str(&hex);
    text.push('\n');
    let _ = write_atomic(&path, &text);
}

fn read_rejected_shas(layout: &Layout) -> HashSet<[u8; 32]> {
    let mut rejected = HashSet::new();
    let Ok(text) = fs::read_to_string(layout.rejected_shas()) else {
        return rejected;
    };
    for line in text.lines().take(64) {
        if let Some(digest) = hex_decode::<32>(line.trim()) {
            rejected.insert(digest);
        }
    }
    rejected
}

fn write_pending_replacing_build(layout: &Layout, manifest: &Manifest) -> Result<(), UpdateError> {
    write_atomic(
        &layout.pending(),
        &format!(
            "{}pin=sha256\n",
            pending_text(manifest.version, &manifest.sha256, manifest.size, 0)
        ),
    )
}

/// Fetches the pin and stages the build it names when that differs from the running one. The
/// running executable is hashed only after the pin was fetched and parsed, so a failed fetch costs
/// no hashing.
fn stage_github_pin(layout: &Layout, current: Version, gate: Option<&Path>) -> Result<CheckOutcome, UpdateError> {
    fs::create_dir_all(&layout.updates)?;
    let pin_url = validate_https_url(GITHUB_PIN_URL)?;
    let mut text = Vec::new();
    http::get(&pin_url, MAX_MANIFEST_BYTES as u64, &mut |chunk| {
        text.extend_from_slice(chunk);
        Ok(())
    })?;
    let text = String::from_utf8(text).map_err(|_| UpdateError::Malformed("not UTF-8"))?;
    let pin = parse_github_pin(&text)?;
    let running_sha = || sha256_file(&layout.exe).map(|(digest, _)| digest);
    let url = match decide_github_pin(&pin, current, running_sha, &read_rejected_shas(layout))? {
        PinDecision::UpToDate => return Ok(CheckOutcome::UpToDate),
        PinDecision::Replace(url) => url,
    };
    let manifest = Manifest {
        version: pin.version,
        min_version: Version(0, 0, 1),
        url,
        sha256: pin.sha256,
        size: pin.bytes,
        notes: String::new(),
    };
    if rejected_version(layout) == Some(manifest.version) && manifest.version > current {
        return Err(UpdateError::Rejected);
    }
    let partial = layout.download_file(manifest.version);
    if let Err(error) = download_and_verify(&manifest, &partial) {
        let _ = fs::remove_file(&partial);
        return Err(error);
    }
    publish_staged(layout, &manifest, &partial, write_pending_replacing_build, gate)
}

/// Set once this process may check for updates: an installed launch, not a dev or benchmark one.
static CHANNEL_OPEN: AtomicBool = AtomicBool::new(false);
/// Set while a background or requested check is running, so two never overlap.
static CHECKING: AtomicBool = AtomicBool::new(false);
/// Set by "Restart to update": `main` starts Bowser again after a clean exit.
static RESTART_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Install-wide switch for automatic updates. It lives in the data root, not in a profile's
/// `settings.txt`, because every profile runs the same executable.
const AUTO_UPDATE_FILE: &str = "auto-update.txt";

fn auto_update_in(root: &Path) -> bool {
    read_kv(&root.join(AUTO_UPDATE_FILE))
        .and_then(|pairs| kv(&pairs, "auto_update").map(|value| value != "false"))
        .unwrap_or(true)
}

/// Whether Bowser checks for, downloads and installs updates without being asked (default on).
pub fn auto_update_enabled() -> bool {
    data_root().is_none_or(|root| auto_update_in(&root))
}

/// Saves the automatic-update switch. Turning it off also drops an update that was already staged,
/// so the next start installs nothing the user did not ask for.
pub fn set_auto_update(on: bool) -> Result<(), UpdateError> {
    let root = data_root().ok_or_else(|| UpdateError::Io("no data directory".into()))?;
    set_auto_update_in(&root, std::env::current_exe().ok(), on)
}

fn set_auto_update_in(root: &Path, exe: Option<PathBuf>, on: bool) -> Result<(), UpdateError> {
    fs::create_dir_all(root)?;
    let switch = root.join(AUTO_UPDATE_FILE);
    let text = if on { "auto_update=true\n" } else { "auto_update=false\n" };
    let (false, Some(exe)) = (on, exe) else {
        return write_atomic(&switch, text);
    };
    // Staging re-reads the switch under this lock (see `publish_staged`), so once it is written no
    // background check can stage again. A busy lock (held briefly) leaves the switch unchanged.
    let layout = Layout::new(exe, root);
    let _lock = SwapLock::acquire(&layout, 1_000)
        .ok_or_else(|| UpdateError::Io("update lock is busy".into()))?;
    write_atomic(&switch, text)?;
    if let Some(pairs) = read_kv(&layout.pending()) {
        discard_staged(&layout, kv(&pairs, "version").and_then(Version::parse));
    }
    Ok(())
}

/// The staged version the next start would install; a record that start would only discard is
/// not offered.
fn ready_version(layout: &Layout, running: Version) -> Option<Version> {
    let pairs = read_kv(&layout.pending())?;
    let version = kv(&pairs, "version").and_then(Version::parse)?;
    (installs_over(version, &pairs, running) && layout.staged_exe(version).is_file())
        .then_some(version)
}

/// `status.txt` texts the menu tells apart.
const STATUS_UP_TO_DATE: &str = "up to date";
const STATUS_FAILED: &str = "check failed";

/// The result of the most recent check, as far as the menu cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastCheck {
    UpToDate,
    Failed,
    /// Anything else, such as an unverified newer version.
    Other,
}

/// What the main menu shows about updates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Dev and benchmark launches never check.
    Unavailable,
    Checking,
    /// A verified build is staged; the next start installs it.
    Ready(Version),
    /// Nothing staged. `last` is the result of the most recent check, if one ran.
    Idle { last: Option<LastCheck> },
}

pub fn status() -> Status {
    if !CHANNEL_OPEN.load(Ordering::Acquire) {
        return Status::Unavailable;
    }
    if CHECKING.load(Ordering::Acquire) {
        return Status::Checking;
    }
    let (Some(root), Ok(exe)) = (data_root(), std::env::current_exe()) else {
        return Status::Unavailable;
    };
    let layout = Layout::new(exe, &root);
    if let Some(version) = ready_version(&layout, Version::current()) {
        return Status::Ready(version);
    }
    let last = fs::read(layout.status_file())
        .ok()
        .filter(|bytes| !bytes.is_empty() && bytes.len() <= 512)
        .map(|bytes| {
            if bytes.starts_with(STATUS_UP_TO_DATE.as_bytes()) {
                LastCheck::UpToDate
            } else if bytes.starts_with(STATUS_FAILED.as_bytes()) {
                LastCheck::Failed
            } else {
                LastCheck::Other
            }
        });
    Status::Idle { last }
}

/// Holds [`CHECKING`] for one check and clears it however the check ends.
struct CheckGuard;

impl CheckGuard {
    fn take() -> Option<Self> {
        CHECKING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            .then_some(Self)
    }
}

impl Drop for CheckGuard {
    fn drop(&mut self) {
        CHECKING.store(false, Ordering::Release);
    }
}

/// One check of whichever channel is configured: the signed manifest, otherwise the GitHub pin.
/// A background check (not `forced`) returns `None` when that channel's interval has not passed,
/// and stages nothing once automatic updates are off. The time and result are recorded for the
/// next check and for [`status`].
fn run_check(root: &Path, layout: &Layout, forced: bool) -> Option<Result<CheckOutcome, UpdateError>> {
    let _ = fs::create_dir_all(&layout.updates);
    let config = load_config(root);
    let (stamp, interval) = match config {
        Some(_) => (layout.last_check(), CHECK_INTERVAL_SECS),
        None => (layout.pin_last_check(), PIN_CHECK_INTERVAL_SECS),
    };
    if !forced {
        let last = fs::read_to_string(&stamp)
            .ok()
            .and_then(|text| text.trim().parse().ok());
        if !check_due_after(now_secs(), last, interval) {
            return None;
        }
    }
    let gate = (!forced).then_some(root);
    let outcome = match &config {
        Some(config) => check_and_stage(config, layout, Version::current(), gate),
        None => stage_github_pin(layout, Version::current(), gate),
    };
    let _ = write_atomic(&stamp, &now_secs().to_string());
    let summary = match &outcome {
        Ok(CheckOutcome::UpToDate) => STATUS_UP_TO_DATE.to_owned(),
        Ok(CheckOutcome::AvailableUnverified(version)) => {
            format!("{version} available; no signing key embedded, not installed")
        }
        Ok(CheckOutcome::Staged(version)) => format!("{version} staged for next start"),
        Err(UpdateError::SwitchedOff) => UpdateError::SwitchedOff.to_string(),
        Err(error) => format!("{STATUS_FAILED}: {error}"),
    };
    let _ = write_atomic(&layout.status_file(), &summary);
    Some(outcome)
}

/// Starts the lazy background worker. It sleeps first (so it never competes with startup), then marks a
/// surviving launch healthy and, when automatic updates are on, checks for an update. A configured
/// signed channel is used as before. Otherwise the GitHub pin is checked, at most once an hour.
/// Failures are silent. Dev launches do not start the thread.
pub fn spawn_background(benchmark: bool, dev: bool) {
    if benchmark || dev {
        return;
    }
    let Some(root) = data_root() else {
        return;
    };
    CHANNEL_OPEN.store(true, Ordering::Release);
    let _ = std::thread::Builder::new()
        .name("bowser-updater".into())
        .spawn(move || {
            std::thread::sleep(HEALTHY_AFTER);
            let Ok(exe) = std::env::current_exe() else {
                return;
            };
            let layout = Layout::new(exe, &root);
            mark_healthy(&layout, Version::current());
            // Read now rather than at launch, so switching it off during the first 45 s counts.
            if !auto_update_in(&root) {
                return;
            }
            let Some(_guard) = CheckGuard::take() else {
                return;
            };
            let _ = run_check(&root, &layout, false);
        });
}

/// Why a requested check did not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckRefused {
    /// Dev and benchmark launches never check.
    Unavailable,
    /// Another check is already running.
    Busy,
}

/// A check the user asked for. It ignores the interval and the automatic-update switch, and stages
/// a verified build the same way the background check does. It blocks on the network, so call it
/// off the UI thread.
pub fn check_now() -> Result<Result<CheckOutcome, UpdateError>, CheckRefused> {
    if !CHANNEL_OPEN.load(Ordering::Acquire) {
        return Err(CheckRefused::Unavailable);
    }
    let (Some(root), Ok(exe)) = (data_root(), std::env::current_exe()) else {
        return Err(CheckRefused::Unavailable);
    };
    let _guard = CheckGuard::take().ok_or(CheckRefused::Busy)?;
    let layout = Layout::new(exe, &root);
    Ok(run_check(&root, &layout, true).unwrap_or(Ok(CheckOutcome::UpToDate)))
}

/// Asks `main` to start Bowser again once this process has closed cleanly; that start installs
/// the staged build. The caller closes the window.
pub fn request_restart() {
    RESTART_REQUESTED.store(true, Ordering::Release);
}

pub fn restart_requested() -> bool {
    RESTART_REQUESTED.load(Ordering::Acquire)
}

/// Set on the process "Restart to update" starts. Its [`startup`] waits a little for the swap
/// lock, which the closing process may still hold, instead of skipping the update.
const RESTART_ENV: &str = "BOWSER_UPDATE_RESTART";

/// The arguments a restart passes on: the launch's own, minus `--open-url <url>`, whose page is
/// already among the tabs the restart restores.
fn restart_args(args: impl IntoIterator<Item = std::ffi::OsString>) -> Vec<std::ffi::OsString> {
    let mut out = Vec::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--open-url" {
            args.next();
        } else {
            out.push(arg);
        }
    }
    out
}

/// Starts Bowser again for "Restart to update". Call it once this process has closed its window
/// and released its profile. Returns false on failure.
pub fn relaunch_for_restart() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    std::process::Command::new(exe)
        .args(restart_args(std::env::args_os().skip(1)))
        .env(RESTART_ENV, "1")
        .spawn()
        .is_ok()
}

fn check_due_after(now_secs: u64, last_check_secs: Option<u64>, interval: u64) -> bool {
    match last_check_secs {
        Some(last) if now_secs.saturating_sub(last) < interval && now_secs >= last => false,
        _ => true,
    }
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const CURRENT: Version = Version(0, 0, 1);

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bowser-updater-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn restart_drops_the_one_shot_url_and_keeps_the_rest() {
        let args = ["--profile", "Work", "--open-url", "https://example.com/", "--new-window"]
            .map(std::ffi::OsString::from);
        assert_eq!(
            restart_args(args),
            ["--profile", "Work", "--new-window"].map(std::ffi::OsString::from)
        );
    }

    #[test]
    fn only_a_build_the_next_start_would_install_is_ready() {
        let root = temp_dir("ready");
        let layout = Layout::new(root.join("bowser.exe"), &root);
        fs::create_dir_all(&layout.updates).unwrap();
        let running = Version(1, 0, 0);
        let stage = |version: Version, pin: bool| {
            fs::write(layout.staged_exe(version), b"build").unwrap();
            let pin = if pin { "pin=sha256\n" } else { "" };
            let record = format!("{}{pin}", pending_text(version, &[0; 32], 5, 0));
            write_atomic(&layout.pending(), &record).unwrap();
        };
        assert_eq!(ready_version(&layout, running), None);
        stage(Version(1, 1, 0), false);
        assert_eq!(ready_version(&layout, running), Some(Version(1, 1, 0)));
        stage(Version(0, 9, 0), false);
        assert_eq!(ready_version(&layout, running), None, "older builds are discarded at start");
        stage(running, false);
        assert_eq!(ready_version(&layout, running), None, "same version needs a pin record");
        stage(running, true);
        assert_eq!(ready_version(&layout, running), Some(running));
        fs::remove_file(layout.staged_exe(running)).unwrap();
        assert_eq!(ready_version(&layout, running), None, "the staged file must exist");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn auto_update_defaults_on_and_turning_it_off_drops_a_staged_build() {
        let root = temp_dir("auto-update");
        let exe = root.join("bowser.exe");
        assert!(auto_update_in(&root), "no switch file means updates are on");
        let layout = Layout::new(exe.clone(), &root);
        fs::create_dir_all(&layout.updates).unwrap();
        let staged = layout.staged_exe(Version(9, 0, 0));
        fs::write(&staged, b"new build").unwrap();
        write_atomic(&layout.pending(), &pending_text(Version(9, 0, 0), &[0; 32], 9, 0)).unwrap();
        assert_eq!(ready_version(&layout, CURRENT), Some(Version(9, 0, 0)));

        set_auto_update_in(&root, Some(exe.clone()), true).unwrap();
        assert!(auto_update_in(&root));
        assert!(layout.pending().is_file(), "turning it on keeps the staged build");

        set_auto_update_in(&root, Some(exe), false).unwrap();
        assert!(!auto_update_in(&root));
        assert!(!layout.pending().exists());
        assert!(!staged.exists());
        assert_eq!(ready_version(&layout, CURRENT), None);

        fs::write(root.join(AUTO_UPDATE_FILE), "junk").unwrap();
        assert!(auto_update_in(&root), "an unreadable switch leaves updates on");
        let _ = fs::remove_dir_all(root);
    }

    fn body(version: &str, min: &str, url: &str, sha: &str, size: u64) -> String {
        format!(
            "{MANIFEST_MAGIC}\nversion={version}\nmin_version={min}\nurl={url}\nsha256={sha}\nsize={size}\nnotes=Fixes\n"
        )
    }

    fn sign(body: &str, key: &cng::TestKey) -> String {
        let sig = key.sign(&cng::sha256(body.as_bytes()).unwrap());
        format!("{body}signature={}\n", hex_encode(&sig))
    }

    fn good_body() -> String {
        body(
            "0.2.0",
            "0.0.1",
            "https://example.com/bowser.exe",
            &"ab".repeat(32),
            1000,
        )
    }

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(
            hex_encode(&sha256(b"").unwrap()),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex_encode(&sha256(b"abc").unwrap()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let long = vec![b'a'; 1_000_000];
        assert_eq!(
            hex_encode(&sha256(&long).unwrap()),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn version_parse_and_order() {
        assert_eq!(Version::parse("1.2.3"), Some(Version(1, 2, 3)));
        for bad in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "1.2.x",
            "1..3",
            "-1.2.3",
            "1.2.3-beta",
            "+1.2.3",
            "1.2.3 ",
            "9999999999.0.0",
        ] {
            assert_eq!(Version::parse(bad), None, "{bad:?}");
        }
        assert!(Version(0, 10, 0) > Version(0, 9, 9));
        assert!(Version(1, 0, 0) > Version(0, 99, 99));
        assert!(Version::current() >= Version(0, 0, 1));
    }

    /// The version the About page shows is `CARGO_PKG_VERSION`, so it has to move with every
    /// release: the pin names the release by tag, its `version` is that tag's number, and the
    /// source being released never carries a lower number than the pin it follows.
    #[test]
    fn the_shown_version_follows_the_pinned_release() {
        let pin = parse_github_pin(include_str!("../stable/release.json")).unwrap();
        // Releases up to v0.0.17 were built as 0.0.1, so their pin keeps that version;
        // from v0.0.18 on the tag and the version are the same number.
        if pin.version > Version(0, 0, 1) {
            assert_eq!(pin.tag, format!("v{}", pin.version), "tag and version name one release");
        }
        assert!(
            Version::current() >= pin.version,
            "Cargo.toml says {} but stable/release.json pins {}: bump Cargo.toml (and Cargo.lock) with the release",
            Version::current(),
            pin.version
        );
    }

    #[test]
    fn signed_manifest_verifies() {
        let key = cng::TestKey::generate();
        let text = sign(&good_body(), &key);
        let evaluated = evaluate(&text, Some(&key.public_xy), CURRENT).unwrap();
        assert!(evaluated.authentic);
        assert_eq!(evaluated.manifest.version, Version(0, 2, 0));
        assert_eq!(evaluated.manifest.size, 1000);
        assert_eq!(evaluated.manifest.notes, "Fixes");
    }

    #[test]
    fn tampered_manifest_is_rejected() {
        let key = cng::TestKey::generate();
        let text = sign(&good_body(), &key);
        for (from, to) in [
            ("version=0.2.0", "version=0.3.0"),
            (
                "https://example.com/bowser.exe",
                "https://evil.example/bowser.exe",
            ),
            ("size=1000", "size=1001"),
            ("notes=Fixes", "notes=Fixed"),
            ("min_version=0.0.1", "min_version=0.0.0"),
        ] {
            let tampered = text.replace(from, to);
            assert_ne!(tampered, text);
            assert_eq!(
                evaluate(&tampered, Some(&key.public_xy), CURRENT),
                Err(UpdateError::BadSignature),
                "{from}"
            );
        }
        // Flipped signature bit.
        let (head, sig) = text.trim_end().rsplit_once("signature=").unwrap();
        let mut sig_bytes = hex_decode::<64>(sig).unwrap();
        sig_bytes[10] ^= 1;
        let flipped = format!("{head}signature={}\n", hex_encode(&sig_bytes));
        assert_eq!(
            evaluate(&flipped, Some(&key.public_xy), CURRENT),
            Err(UpdateError::BadSignature)
        );
        // Extra line appended after the signature, and trailing junk body line.
        let appended = format!("{text}notes=x\n");
        assert_eq!(
            evaluate(&appended, Some(&key.public_xy), CURRENT),
            Err(UpdateError::BadSignature)
        );
    }

    #[test]
    fn wrong_key_unsigned_and_malformed_signatures_are_rejected() {
        let key = cng::TestKey::generate();
        let other = cng::TestKey::generate();
        let text = sign(&good_body(), &key);
        assert_eq!(
            evaluate(&text, Some(&other.public_xy), CURRENT),
            Err(UpdateError::BadSignature)
        );
        assert_eq!(
            evaluate(&good_body(), Some(&key.public_xy), CURRENT),
            Err(UpdateError::BadSignature)
        );
        let short = format!("{}signature={}\n", good_body(), "00".repeat(63));
        assert_eq!(
            evaluate(&short, Some(&key.public_xy), CURRENT),
            Err(UpdateError::BadSignature)
        );
        let zero = format!("{}signature={}\n", good_body(), "00".repeat(64));
        assert_eq!(
            evaluate(&zero, Some(&key.public_xy), CURRENT),
            Err(UpdateError::BadSignature)
        );
        let not_hex = format!("{}signature={}\n", good_body(), "zz".repeat(64));
        assert_eq!(
            evaluate(&not_hex, Some(&key.public_xy), CURRENT),
            Err(UpdateError::BadSignature)
        );
        assert_eq!(
            evaluate(&text.replace('\n', "\r\n"), Some(&key.public_xy), CURRENT),
            Err(UpdateError::Malformed("CR or NUL"))
        );
        let big = format!("{}{}", "x".repeat(MAX_MANIFEST_BYTES), text);
        assert_eq!(
            evaluate(&big, Some(&key.public_xy), CURRENT),
            Err(UpdateError::ManifestTooLarge)
        );
    }

    #[test]
    fn signed_but_policy_violating_manifests_are_refused() {
        let key = cng::TestKey::generate();
        let sha = "cd".repeat(32);
        let check = |b: String| evaluate(&sign(&b, &key), Some(&key.public_xy), CURRENT);
        assert_eq!(
            check(body("0.0.1", "0.0.1", "https://e.com/a.exe", &sha, 10)),
            Err(UpdateError::NotNewer)
        );
        assert_eq!(
            check(body("0.0.0", "0.0.0", "https://e.com/a.exe", &sha, 10)),
            Err(UpdateError::NotNewer)
        );
        assert_eq!(
            check(body("0.5.0", "0.4.0", "https://e.com/a.exe", &sha, 10)),
            Err(UpdateError::BelowMinVersion)
        );
        assert_eq!(
            check(body("0.5.0", "0.0.1", "http://e.com/a.exe", &sha, 10)),
            Err(UpdateError::InsecureUrl)
        );
        assert_eq!(
            check(body("0.5.0", "0.0.1", "ftp://e.com/a.exe", &sha, 10)),
            Err(UpdateError::InsecureUrl)
        );
        assert_eq!(
            check(body(
                "0.5.0",
                "0.0.1",
                "https://user:pw@e.com/a.exe",
                &sha,
                10
            )),
            Err(UpdateError::InsecureUrl)
        );
        assert_eq!(
            check(body(
                "0.5.0",
                "0.0.1",
                "https://e.com/a.exe",
                &sha,
                MAX_PACKAGE_BYTES + 1
            )),
            Err(UpdateError::Oversize)
        );
        assert_eq!(
            check(body("0.5.0", "0.0.1", "https://e.com/a.exe", &sha, 0)),
            Err(UpdateError::Oversize)
        );
        assert!(matches!(
            check(body("0.5.0", "0.0.1", "https://e.com/a.exe", "abcd", 10)),
            Err(UpdateError::Malformed(_))
        ));
        let dup = format!(
            "{}version=0.9.0\n",
            body("0.5.0", "0.0.1", "https://e.com/a.exe", &sha, 10)
        );
        assert!(matches!(
            check(dup),
            Err(UpdateError::Malformed("duplicate key"))
        ));
        let unknown = format!(
            "{}extra=1\n",
            body("0.5.0", "0.0.1", "https://e.com/a.exe", &sha, 10)
        );
        assert!(matches!(
            check(unknown),
            Err(UpdateError::Malformed("unknown key"))
        ));
        assert!(matches!(
            check("something-else\nversion=0.5.0\n".into()),
            Err(UpdateError::Malformed(_))
        ));
    }

    #[test]
    fn without_a_public_key_manifest_is_never_authentic() {
        let key = cng::TestKey::generate();
        let text = sign(&good_body(), &key);
        let evaluated = evaluate(&text, None, CURRENT).unwrap();
        assert!(
            !evaluated.authentic,
            "check-only mode must not be installable"
        );
        // No key and no signature line at all is still non-authentic or an error, never authentic.
        assert!(evaluate(&good_body(), None, CURRENT).is_err());
        assert_eq!(
            compiled_public_key(),
            None,
            "test builds must not embed a key"
        );
    }

    #[test]
    fn package_verification_catches_size_and_hash() {
        let manifest = evaluate(
            &sign(
                &body(
                    "0.2.0",
                    "0.0.1",
                    "https://e.com/a.exe",
                    &hex_encode(&sha256(b"hello").unwrap()),
                    5,
                ),
                &cng::TestKey::generate(),
            ),
            None,
            CURRENT,
        )
        .unwrap()
        .manifest;
        assert_eq!(
            verify_package(&manifest, 5, &sha256(b"hello").unwrap()),
            Ok(())
        );
        assert_eq!(
            verify_package(&manifest, 5, &sha256(b"hellp").unwrap()),
            Err(UpdateError::HashMismatch)
        );
        assert_eq!(
            verify_package(&manifest, 6, &sha256(b"hello").unwrap()),
            Err(UpdateError::SizeMismatch)
        );
        assert_eq!(
            verify_package(&manifest, MAX_PACKAGE_BYTES + 1, &sha256(b"hello").unwrap()),
            Err(UpdateError::Oversize)
        );
    }

    #[test]
    fn redirects_must_stay_on_https() {
        let base = Url::parse("https://example.com/dir/manifest.txt").unwrap();
        assert_eq!(
            resolve_redirect(&base, "/other.exe").unwrap().as_str(),
            "https://example.com/other.exe"
        );
        assert_eq!(
            resolve_redirect(&base, "https://cdn.example.net/x")
                .unwrap()
                .host_str(),
            Some("cdn.example.net")
        );
        for bad in [
            "http://example.com/x",
            "//evil.test/x#",
            "ftp://example.com/x",
            "file:///c:/x",
            "https://u:p@example.com/x",
        ] {
            let result = resolve_redirect(&base, bad);
            if bad == "//evil.test/x#" {
                assert!(result.is_ok(), "scheme-relative keeps https");
            } else {
                assert_eq!(result, Err(UpdateError::InsecureUrl), "{bad}");
            }
        }
        assert!(validate_https_url("http://example.com").is_err());
        assert!(validate_https_url("https://").is_err());
        assert!(validate_https_url("not a url").is_err());
    }

    #[test]
    fn check_runs_at_most_once_per_day() {
        let day = CHECK_INTERVAL_SECS;
        assert!(check_due(1_000_000, None));
        assert!(!check_due(1_000_000, Some(1_000_000)));
        assert!(!check_due(1_000_000 + day - 1, Some(1_000_000)));
        assert!(check_due(1_000_000 + day, Some(1_000_000)));
        assert!(
            check_due(1_000, Some(5_000_000)),
            "timestamps in the future must not suppress checks forever"
        );
    }

    #[test]
    fn config_requires_https_and_disables_when_empty() {
        let root = temp_dir("config");
        assert!(load_config(&root).is_none() || COMPILED_MANIFEST_URL.is_some());
        fs::write(
            root.join("update-url.txt"),
            "\n  https://example.com/m.txt \n",
        )
        .unwrap();
        assert_eq!(
            load_config(&root).unwrap().manifest_url.as_str(),
            "https://example.com/m.txt"
        );
        fs::write(root.join("update-url.txt"), "http://example.com/m.txt").unwrap();
        assert!(load_config(&root).is_none());
        fs::write(root.join("update-url.txt"), "").unwrap();
        assert!(load_config(&root).is_none() || COMPILED_MANIFEST_URL.is_some());
        let _ = fs::remove_dir_all(root);
    }

    fn trial(version: Version, attempts: u32, running: bool) -> Option<Trial> {
        Some(Trial {
            version,
            attempts,
            running,
        })
    }

    #[test]
    fn trial_decisions() {
        let new = Version(0, 2, 0);
        assert_eq!(decide_trial(None, new), TrialAction::None);
        // Fresh swap: the first launch is not yet a crash.
        assert_eq!(
            decide_trial(trial(new, 0, false), new),
            TrialAction::Count(0)
        );
        // That launch never reported back, so the next start counts it as a crash.
        assert_eq!(
            decide_trial(trial(new, 0, true), new),
            TrialAction::Count(1)
        );
        assert_eq!(
            decide_trial(trial(new, 1, true), new),
            TrialAction::Count(2)
        );
        assert_eq!(
            decide_trial(trial(new, 2, true), new),
            TrialAction::Rollback,
            "three crashed launches"
        );
        assert_eq!(
            decide_trial(trial(new, MAX_TRIAL_ATTEMPTS, false), new),
            TrialAction::Rollback
        );
        assert_eq!(
            decide_trial(trial(new, 1, true), Version(0, 0, 1)),
            TrialAction::Clear
        );
    }

    #[test]
    fn clean_exits_are_never_counted_as_crashes() {
        let new = Version(0, 2, 0);
        let f = fixture("clean");
        stage(&f, new, b"NEW BUILD");
        assert_eq!(apply_pending(&f.layout, CURRENT), ApplyResult::Swapped(new));
        // Many quick launches that each exit cleanly: none may roll back.
        for _ in 0..10 {
            assert_eq!(begin_launch(&f.layout, new), TrialAction::Count(0));
            mark_clean_exit(&f.layout, new);
        }
        assert_eq!(read_trial(&f.layout), trial(new, 0, false));
        let _ = fs::remove_dir_all(f.dir);
    }

    #[test]
    fn healthy_marker_only_clears_the_matching_build() {
        let f = fixture("healthy");
        let new = Version(0, 2, 0);
        write_trial(
            &f.layout,
            Trial {
                version: new,
                attempts: 1,
                running: true,
            },
        )
        .unwrap();
        mark_healthy(&f.layout, Version(0, 3, 0));
        assert!(
            read_trial(&f.layout).is_some(),
            "another build's trial stays"
        );
        mark_healthy(&f.layout, new);
        assert!(read_trial(&f.layout).is_none());
        let _ = fs::remove_dir_all(f.dir);
    }

    #[test]
    fn swap_lock_is_exclusive_across_threads() {
        let f = fixture("lock");
        let held = SwapLock::acquire(&f.layout, 0).expect("first holder");
        let layout = f.layout.clone();
        let contender = std::thread::spawn(move || SwapLock::acquire(&layout, 0).is_some())
            .join()
            .unwrap();
        assert!(!contender, "a second holder must be refused");
        drop(held);
        let layout = f.layout.clone();
        let later = std::thread::spawn(move || SwapLock::acquire(&layout, 0).is_some())
            .join()
            .unwrap();
        assert!(later, "released by the holder");
        let _ = fs::remove_dir_all(f.dir);
    }

    #[test]
    fn failed_swaps_are_counted_then_discarded() {
        let f = fixture("failcount");
        let new = Version(0, 2, 0);
        stage(&f, new, b"NEW BUILD");
        // A directory where the rollback copy should go makes the swap fail.
        fs::create_dir_all(f.layout.old_exe()).unwrap();
        assert!(matches!(
            apply_pending(&f.layout, CURRENT),
            ApplyResult::Failed(_)
        ));
        assert!(
            f.layout.pending().exists(),
            "first failure is kept for a retry"
        );
        assert_eq!(fs::read(&f.layout.exe).unwrap(), b"OLD BUILD");
        assert!(matches!(
            apply_pending(&f.layout, CURRENT),
            ApplyResult::Failed(_)
        ));
        assert!(
            !f.layout.pending().exists(),
            "discarded after the second failure"
        );
        assert!(!f.layout.staged_exe(new).exists());
        assert_eq!(apply_pending(&f.layout, CURRENT), ApplyResult::Nothing);
        let _ = fs::remove_dir_all(f.dir);
    }

    struct Fixture {
        dir: PathBuf,
        layout: Layout,
    }

    fn fixture(name: &str) -> Fixture {
        let dir = temp_dir(name);
        let install = dir.join("install");
        fs::create_dir_all(&install).unwrap();
        fs::write(install.join("bowser.exe"), b"OLD BUILD").unwrap();
        let layout = Layout::new(install.join("bowser.exe"), &dir.join("data"));
        fs::create_dir_all(&layout.updates).unwrap();
        Fixture { dir, layout }
    }

    fn stage(f: &Fixture, version: Version, content: &[u8]) {
        fs::write(f.layout.staged_exe(version), content).unwrap();
        let manifest = Manifest {
            version,
            min_version: CURRENT,
            url: Url::parse("https://e.com/a.exe").unwrap(),
            sha256: sha256(content).unwrap(),
            size: content.len() as u64,
            notes: String::new(),
        };
        write_pending(&f.layout, &manifest).unwrap();
    }

    #[test]
    fn swap_keeps_old_build_and_starts_trial() {
        let f = fixture("swap");
        let new = Version(0, 2, 0);
        stage(&f, new, b"NEW BUILD");
        assert_eq!(apply_pending(&f.layout, CURRENT), ApplyResult::Swapped(new));
        assert_eq!(fs::read(&f.layout.exe).unwrap(), b"NEW BUILD");
        assert_eq!(fs::read(f.layout.old_exe()).unwrap(), b"OLD BUILD");
        assert!(!f.layout.pending().exists() && !f.layout.staged_exe(new).exists());
        assert_eq!(read_trial(&f.layout), trial(new, 0, false));
        assert_eq!(apply_pending(&f.layout, new), ApplyResult::Nothing);
        let _ = fs::remove_dir_all(f.dir);
    }

    #[test]
    fn same_version_pin_replaces_a_different_build_only() {
        let f = fixture("pin-swap");
        stage(&f, CURRENT, b"PIN BUILD");
        assert_eq!(apply_pending(&f.layout, CURRENT), ApplyResult::Nothing);
        stage(&f, CURRENT, b"PIN BUILD");
        write_pending_replacing_build(
            &f.layout,
            &Manifest {
                version: CURRENT,
                min_version: CURRENT,
                url: Url::parse("https://github.com/grantcharlie1000-tech/Bowser/releases/download/v0.0.1/Bowser.exe").unwrap(),
                sha256: sha256(b"PIN BUILD").unwrap(),
                size: b"PIN BUILD".len() as u64,
                notes: String::new(),
            },
        )
        .unwrap();
        assert_eq!(apply_pending(&f.layout, CURRENT), ApplyResult::Swapped(CURRENT));
        assert_eq!(fs::read(&f.layout.exe).unwrap(), b"PIN BUILD");
        let _ = fs::remove_dir_all(f.dir);
    }

    #[test]
    fn github_pin_stays_on_this_repository_and_skips_a_rolled_back_hash() {
        let sha = "ab".repeat(32);
        let pin = format!(
            r#"{{"version":"0.0.1","revision":{PIN_REVISION},"commit":"abc","tag":"v0.0.1","repository":"{GITHUB_REPOSITORY}","asset":"{GITHUB_ASSET}","sha256":"{sha}","bytes":100}}"#
        );
        let parsed = parse_github_pin(&pin).unwrap();
        let running = [0u8; 32];
        assert!(matches!(
            decide_github_pin(&parsed, CURRENT, || Ok(running), &HashSet::new()).unwrap(),
            PinDecision::Replace(_)
        ));
        assert_eq!(
            decide_github_pin(&parsed, CURRENT, || Ok(parsed.sha256), &HashSet::new()).unwrap(),
            PinDecision::UpToDate
        );
        let mut rejected = HashSet::new();
        rejected.insert(parsed.sha256);
        assert!(matches!(
            decide_github_pin(&parsed, CURRENT, || Ok(running), &rejected),
            Err(UpdateError::Rejected)
        ));
        let other = parse_github_pin(&pin.replace(GITHUB_REPOSITORY, "evil/Bowser")).unwrap();
        assert!(decide_github_pin(&other, CURRENT, || Ok(running), &HashSet::new()).is_err());
        let older = parse_github_pin(&pin.replace("\"version\":\"0.0.1\"", "\"version\":\"0.0.0\"")).unwrap();
        assert!(matches!(
            decide_github_pin(&older, CURRENT, || Ok(running), &HashSet::new()),
            Err(UpdateError::NotNewer)
        ));
        let from = format!("\"revision\":{PIN_REVISION}");
        let stale = parse_github_pin(&pin.replace(&from, "\"revision\":1")).unwrap();
        assert!(matches!(
            decide_github_pin(&stale, CURRENT, || Ok(running), &HashSet::new()),
            Err(UpdateError::NotNewer)
        ));
    }

    #[test]
    fn the_running_build_is_hashed_only_when_a_comparison_is_needed() {
        let sha = "ab".repeat(32);
        let pin = format!(
            r#"{{"version":"0.0.1","revision":{PIN_REVISION},"commit":"abc","tag":"v0.0.1","repository":"{GITHUB_REPOSITORY}","asset":"{GITHUB_ASSET}","sha256":"{sha}","bytes":100}}"#
        );
        let parsed = parse_github_pin(&pin).unwrap();
        let hashed = std::cell::Cell::new(0);
        let running = |digest: [u8; 32]| {
            let hashed = &hashed;
            move || {
                hashed.set(hashed.get() + 1);
                Ok(digest)
            }
        };
        // A pin that is refused outright never reaches the hash.
        let mut rejected = HashSet::new();
        rejected.insert(parsed.sha256);
        assert!(decide_github_pin(&parsed, CURRENT, running([0; 32]), &rejected).is_err());
        let other = parse_github_pin(&pin.replace(GITHUB_REPOSITORY, "evil/Bowser")).unwrap();
        assert!(decide_github_pin(&other, CURRENT, running([0; 32]), &HashSet::new()).is_err());
        let older = parse_github_pin(&pin.replace("\"version\":\"0.0.1\"", "\"version\":\"0.0.0\"")).unwrap();
        assert!(decide_github_pin(&older, CURRENT, running([0; 32]), &HashSet::new()).is_err());
        assert_eq!(hashed.get(), 0);
        // A real comparison hashes once, and a failed hash is the check's error.
        assert!(matches!(
            decide_github_pin(&parsed, CURRENT, running([0; 32]), &HashSet::new()),
            Ok(PinDecision::Replace(_))
        ));
        assert_eq!(hashed.get(), 1);
        let failing = || Err(UpdateError::Io("cannot read".into()));
        assert!(decide_github_pin(&parsed, CURRENT, failing, &HashSet::new()).is_err());
    }

    #[test]
    fn tampered_or_obsolete_staged_files_are_not_applied() {
        let f = fixture("tamper");
        let new = Version(0, 2, 0);
        stage(&f, new, b"NEW BUILD");
        fs::write(f.layout.staged_exe(new), b"EVIL BUILD").unwrap();
        assert!(matches!(
            apply_pending(&f.layout, CURRENT),
            ApplyResult::Failed(_)
        ));
        assert_eq!(fs::read(&f.layout.exe).unwrap(), b"OLD BUILD");
        assert!(!f.layout.old_exe().exists());
        assert!(!f.layout.pending().exists(), "bad stage is discarded");

        stage(&f, new, b"NEW BUILD");
        assert_eq!(
            apply_pending(&f.layout, new),
            ApplyResult::Nothing,
            "same version is never re-applied"
        );
        assert_eq!(fs::read(&f.layout.exe).unwrap(), b"OLD BUILD");
        stage(&f, Version(0, 0, 1), b"DOWNGRADE");
        assert_eq!(
            apply_pending(&f.layout, Version(0, 1, 0)),
            ApplyResult::Nothing,
            "downgrade is never applied"
        );
        assert_eq!(fs::read(&f.layout.exe).unwrap(), b"OLD BUILD");

        fs::write(f.layout.pending(), "version=banana\n").unwrap();
        assert!(matches!(
            apply_pending(&f.layout, CURRENT),
            ApplyResult::Failed(_)
        ));
        let _ = fs::remove_dir_all(f.dir);
    }

    #[test]
    fn crash_loop_restores_previous_build_and_blocks_reoffer() {
        let f = fixture("rollback");
        let new = Version(0, 2, 0);
        stage(&f, new, b"NEW BUILD");
        assert_eq!(apply_pending(&f.layout, CURRENT), ApplyResult::Swapped(new));
        // Launches that crash without ever reaching "healthy" or a clean exit.
        for expected in 0..MAX_TRIAL_ATTEMPTS {
            assert_eq!(begin_launch(&f.layout, new), TrialAction::Count(expected));
        }
        assert_eq!(begin_launch(&f.layout, new), TrialAction::Rollback);
        rollback(&f.layout, new).unwrap();
        assert_eq!(fs::read(&f.layout.exe).unwrap(), b"OLD BUILD");
        assert_eq!(fs::read(f.layout.bad_exe()).unwrap(), b"NEW BUILD");
        assert!(read_trial(&f.layout).is_none());
        assert_eq!(rejected_version(&f.layout), Some(new));
        let _ = fs::remove_dir_all(f.dir);
    }

    #[test]
    fn rollback_without_a_copy_fails_safely() {
        let f = fixture("norollback");
        assert!(rollback(&f.layout, Version(0, 2, 0)).is_err());
        assert_eq!(fs::read(&f.layout.exe).unwrap(), b"OLD BUILD");
        let _ = fs::remove_dir_all(f.dir);
    }

    #[test]
    fn authenticode_reports_unsigned_for_plain_files() {
        let dir = temp_dir("authenticode");
        let file = dir.join("plain.exe");
        fs::write(&file, b"MZ not a real executable").unwrap();
        assert!(!matches!(authenticode(&file), Authenticode::Valid));
        let _ = fs::remove_dir_all(dir);
    }
}
