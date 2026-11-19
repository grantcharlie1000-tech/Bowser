//! The backup cipher on Windows CNG (bcrypt.dll): PBKDF2-HMAC-SHA256 for the key, AES-256-GCM for
//! the contents, the system RNG for salt and nonce. Known-answer tests are in `super::tests`.

use std::ptr::null_mut;

use windows::Win32::Security::Cryptography::{
    BCRYPT_AES_ALGORITHM, BCRYPT_ALG_HANDLE, BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO,
    BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO_VERSION, BCRYPT_CHAIN_MODE_GCM, BCRYPT_CHAINING_MODE, BCRYPT_FLAGS,
    BCRYPT_HANDLE, BCRYPT_HMAC_SHA256_ALG_HANDLE, BCRYPT_KEY_HANDLE, BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS,
    BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptCloseAlgorithmProvider, BCryptDecrypt, BCryptDeriveKeyPBKDF2,
    BCryptDestroyKey, BCryptEncrypt, BCryptGenRandom, BCryptGenerateSymmetricKey, BCryptOpenAlgorithmProvider,
    BCryptSetProperty,
};
use zeroize::Zeroizing;

use super::BackupError;

/// `STATUS_AUTH_TAG_MISMATCH`: the tag did not verify (wrong key or changed data).
const STATUS_AUTH_TAG_MISMATCH: i32 = 0xC000_A002_u32 as i32;

pub(super) fn random(out: &mut [u8]) -> Result<(), BackupError> {
    let status = unsafe { BCryptGenRandom(None, out, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    if status.0 < 0 { Err(BackupError::Crypto) } else { Ok(()) }
}

/// 32-byte PBKDF2-HMAC-SHA256 of `password` and `salt`.
pub(super) fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32) -> Result<Zeroizing<[u8; 32]>, BackupError> {
    let mut key = Zeroizing::new([0u8; 32]);
    // An empty password is passed as an empty slice: CNG takes a null pointer with length 0.
    let status = unsafe {
        BCryptDeriveKeyPBKDF2(BCRYPT_HMAC_SHA256_ALG_HANDLE, Some(password), Some(salt), u64::from(iterations), &mut key[..], 0)
    };
    if status.0 < 0 { Err(BackupError::Crypto) } else { Ok(key) }
}

/// An AES-256-GCM key, closed on drop.
struct GcmKey {
    alg: BCRYPT_ALG_HANDLE,
    key: BCRYPT_KEY_HANDLE,
}

impl GcmKey {
    fn new(secret: &[u8; 32]) -> Result<Self, BackupError> {
        let mut alg = BCRYPT_ALG_HANDLE(null_mut());
        let status =
            unsafe { BCryptOpenAlgorithmProvider(&mut alg, BCRYPT_AES_ALGORITHM, None, BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0)) };
        if status.0 < 0 {
            return Err(BackupError::Crypto);
        }
        // "ChainingModeGCM" with its terminating NUL, as UTF-16LE bytes.
        let mode: Vec<u8> = unsafe { BCRYPT_CHAIN_MODE_GCM.as_wide() }
            .iter()
            .chain(std::iter::once(&0u16))
            .flat_map(|unit| unit.to_le_bytes())
            .collect();
        let status = unsafe { BCryptSetProperty(BCRYPT_HANDLE(alg.0), BCRYPT_CHAINING_MODE, &mode, 0) };
        if status.0 < 0 {
            let _ = unsafe { BCryptCloseAlgorithmProvider(alg, 0) };
            return Err(BackupError::Crypto);
        }
        let mut key = BCRYPT_KEY_HANDLE(null_mut());
        let status = unsafe { BCryptGenerateSymmetricKey(alg, &mut key, None, secret, 0) };
        if status.0 < 0 {
            let _ = unsafe { BCryptCloseAlgorithmProvider(alg, 0) };
            return Err(BackupError::Crypto);
        }
        Ok(Self { alg, key })
    }
}

impl Drop for GcmKey {
    fn drop(&mut self) {
        unsafe {
            let _ = BCryptDestroyKey(self.key);
            let _ = BCryptCloseAlgorithmProvider(self.alg, 0);
        }
    }
}

fn mode_info(nonce: &[u8; 12], aad: &[u8], tag: &mut [u8; 16]) -> BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO {
    BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO {
        cbSize: std::mem::size_of::<BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO>() as u32,
        dwInfoVersion: BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO_VERSION,
        pbNonce: nonce.as_ptr().cast_mut(),
        cbNonce: nonce.len() as u32,
        pbAuthData: if aad.is_empty() { null_mut() } else { aad.as_ptr().cast_mut() },
        cbAuthData: aad.len() as u32,
        pbTag: tag.as_mut_ptr(),
        cbTag: tag.len() as u32,
        ..Default::default()
    }
}

/// Encrypts `plain`; returns the ciphertext (same length) and the 16-byte tag.
pub(super) fn aes_gcm_seal(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    plain: &[u8],
) -> Result<(Vec<u8>, [u8; 16]), BackupError> {
    let key = GcmKey::new(key)?;
    let mut tag = [0u8; 16];
    let info = mode_info(nonce, aad, &mut tag);
    let mut out = vec![0u8; plain.len()];
    let mut written = 0u32;
    let input = (!plain.is_empty()).then_some(plain);
    let output = (!out.is_empty()).then_some(&mut out[..]);
    let status = unsafe {
        BCryptEncrypt(key.key, input, Some(std::ptr::from_ref(&info).cast()), None, output, &mut written, BCRYPT_FLAGS(0))
    };
    if status.0 < 0 || written as usize != plain.len() {
        return Err(BackupError::Crypto);
    }
    Ok((out, tag))
}

/// Decrypts and verifies; a tag that does not match is [`BackupError::Password`].
pub(super) fn aes_gcm_open(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    cipher: &[u8],
    tag: &[u8; 16],
) -> Result<Zeroizing<Vec<u8>>, BackupError> {
    let key = GcmKey::new(key)?;
    let mut tag = *tag;
    let info = mode_info(nonce, aad, &mut tag);
    let mut out = Zeroizing::new(vec![0u8; cipher.len()]);
    let mut written = 0u32;
    let input = (!cipher.is_empty()).then_some(cipher);
    let output = (!out.is_empty()).then_some(&mut out[..]);
    let status = unsafe {
        BCryptDecrypt(key.key, input, Some(std::ptr::from_ref(&info).cast()), None, output, &mut written, BCRYPT_FLAGS(0))
    };
    if status.0 == STATUS_AUTH_TAG_MISMATCH {
        return Err(BackupError::Password);
    }
    if status.0 < 0 || written as usize != cipher.len() {
        return Err(BackupError::Crypto);
    }
    Ok(out)
}

