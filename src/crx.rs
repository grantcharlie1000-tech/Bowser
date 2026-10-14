//! CRX3 parser and verifier. Pure Rust over bytes already in memory: no network, no unpacking, no
//! WebView2. Signature maths and SHA-256 run on Windows CNG (the same primitives `updater.rs` uses),
//! so no crypto crate is added.
//!
//! Layout (Chromium `crx_verifier.cc`, re-read 2026-10-14): `Cr24`, LE u32 version (must be 3), LE u32
//! header length N, N bytes of `CrxFileHeader` protobuf, then the ZIP. Every proof signs
//! `"CRX3 SignedData\x00" || LE32(len(signed_header_data)) || signed_header_data || zip`. RSA proofs
//! are PKCS#1 v1.5 SHA-256 (the proto comment says PSS; the code governs) and ECDSA proofs are P-256
//! SHA-256 with a DER signature.
//!
//! Protobuf field numbers match `crx3.proto` as of Chromium main, fetched 2026-10-14:
//! `CrxFileHeader.sha256_with_rsa = 2`, `sha256_with_ecdsa = 3`, `signed_header_data = 10000`;
//! `AsymmetricKeyProof.public_key = 1`, `signature = 2`; `SignedData.crx_id = 1` (16 bytes).

use std::fmt;

/// Largest accepted header length N. Chromium has no cap; the proto says clients should reject an N
/// "too large". Web Store CRX3 headers can carry `verified_contents` (field 4), whose size grows with
/// the number of files in the extension, so the cap has to leave generous room. It does not bound
/// memory, because the caller already holds the bytes; it only bounds the parse.
pub const MAX_HEADER_LEN: usize = 16 << 20;
/// Cap on proofs per file, so a hostile header cannot make us run unbounded signature checks.
pub const MAX_PROOFS: usize = 16;

const MAGIC: &[u8; 4] = b"Cr24";
const FIXED_PREFIX_LEN: usize = 12;
const SIGNATURE_CONTEXT: &[u8] = b"CRX3 SignedData\x00";
const CRX_ID_LEN: usize = 16;
/// Chromium accepts 1024-bit developer keys (an extension keeps the key it was first packed with);
/// BoringSSL's own floor is 512.
const MIN_RSA_BITS: usize = 1024;
const MAX_RSA_BITS: usize = 8192;
/// ZIP structure tokens Chromium rejects inside the header (`PK\x05\x06`, `PK\x06\x07` and the
/// killswitched `PK\x06\x06`), plus the local-file-header token, so a header cannot smuggle a second
/// archive past a scanner that looks for the first ZIP signature.
const ZIP_TOKENS: [&[u8; 4]; 4] = [b"PK\x03\x04", b"PK\x05\x06", b"PK\x06\x07", b"PK\x06\x06"];

const FIELD_SHA256_WITH_RSA: u32 = 2;
const FIELD_SHA256_WITH_ECDSA: u32 = 3;
const FIELD_SIGNED_HEADER_DATA: u32 = 10000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrxError {
    /// The caller-supplied extension id is not 32 characters of `a`-`p`.
    InvalidRequestedId,
    /// Input ended before the fixed prefix, the declared header, or the archive.
    Truncated,
    BadMagic,
    UnsupportedVersion(u32),
    /// Declared header length exceeds [`MAX_HEADER_LEN`] (or is zero).
    HeaderTooLarge(u64),
    HeaderContainsZipToken,
    MalformedHeader(&'static str),
    MissingSignedHeaderData,
    MalformedSignedData(&'static str),
    NoProofs,
    TooManyProofs,
    MalformedProof(&'static str),
    BadPublicKey(&'static str),
    /// The key in a proof is not of the type its header field promises.
    KeyTypeMismatch,
    SignatureInvalid,
    EmptyArchive,
    /// The declared `crx_id` is not the requested id.
    IdMismatch,
    /// No verified proof was made by the key that derives the declared `crx_id`.
    DeveloperProofMissing,
    /// No verified proof was made by a pinned publisher key.
    PublisherProofMissing,
    /// CNG could not hash or import a key.
    CryptoUnavailable,
}

impl fmt::Display for CrxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequestedId => f.write_str("extension id must be 32 characters a-p"),
            Self::Truncated => f.write_str("CRX is truncated"),
            Self::BadMagic => f.write_str("not a CRX file"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported CRX version {v}"),
            Self::HeaderTooLarge(n) => write!(f, "CRX header length {n} is out of bounds"),
            Self::HeaderContainsZipToken => f.write_str("CRX header contains ZIP structure bytes"),
            Self::MalformedHeader(why) => write!(f, "malformed CRX header: {why}"),
            Self::MissingSignedHeaderData => f.write_str("CRX header has no signed_header_data"),
            Self::MalformedSignedData(why) => write!(f, "malformed signed_header_data: {why}"),
            Self::NoProofs => f.write_str("CRX has no signature proofs"),
            Self::TooManyProofs => f.write_str("CRX has too many signature proofs"),
            Self::MalformedProof(why) => write!(f, "malformed proof: {why}"),
            Self::BadPublicKey(why) => write!(f, "unusable public key: {why}"),
            Self::KeyTypeMismatch => f.write_str("proof key type does not match its field"),
            Self::SignatureInvalid => f.write_str("CRX signature does not verify"),
            Self::EmptyArchive => f.write_str("CRX has no archive"),
            Self::IdMismatch => f.write_str("CRX id is not the requested extension id"),
            Self::DeveloperProofMissing => f.write_str("no proof by the key that owns the CRX id"),
            Self::PublisherProofMissing => f.write_str("no proof by a pinned publisher key"),
            Self::CryptoUnavailable => f.write_str("system crypto is unavailable"),
        }
    }
}

impl std::error::Error for CrxError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    Rsa,
    EcdsaP256,
}

/// A proof whose signature verified over the archive. Only [`verify_crx3`] constructs one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedProof {
    kind: KeyKind,
    key_sha256: [u8; 32],
    derived_id: String,
}

impl VerifiedProof {
    pub fn kind(&self) -> KeyKind {
        self.kind
    }

    /// SHA-256 of the proof's DER SubjectPublicKeyInfo: what a publisher pin is compared against.
    pub fn key_sha256(&self) -> &[u8; 32] {
        &self.key_sha256
    }

    /// The extension id this key derives.
    pub fn derived_id(&self) -> &str {
        &self.derived_id
    }
}

/// A CRX whose proofs all verified. Only [`verify_crx3`] constructs one, so holding a value proves
/// the checks ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedCrx<'a> {
    extension_id: String,
    archive: &'a [u8],
    developer_public_key: &'a [u8],
    proofs: Vec<VerifiedProof>,
}

impl<'a> VerifiedCrx<'a> {
    /// The id the CRX declares: equal to the requested id and derived from the developer key.
    pub fn extension_id(&self) -> &str {
        &self.extension_id
    }

    /// The raw ZIP bytes the proofs cover.
    pub fn archive(&self) -> &'a [u8] {
        self.archive
    }

    /// DER SubjectPublicKeyInfo of the verified developer key, the one that derives the id.
    pub fn developer_public_key(&self) -> &'a [u8] {
        self.developer_public_key
    }

    pub fn proofs(&self) -> &[VerifiedProof] {
        &self.proofs
    }

    /// Additionally requires a verified proof by a key whose SPKI SHA-256 is in `pinned`. The caller
    /// owns the (versioned) allowlist; an empty list never passes.
    ///
    /// Any one matching pin succeeds, as in Chromium's publisher-key check. It is not Chromium's
    /// `required_key_hashes`, which needs every listed key to have signed.
    #[must_use = "an unchecked result means the publisher pin was not enforced"]
    pub fn require_pinned_publisher(&self, pinned: &[[u8; 32]]) -> Result<(), CrxError> {
        if self.proofs.iter().any(|p| pinned.contains(&p.key_sha256)) {
            Ok(())
        } else {
            Err(CrxError::PublisherProofMissing)
        }
    }
}

/// `a`-`p` encoding of the first 16 bytes of SHA-256 over a DER SubjectPublicKeyInfo.
pub fn extension_id_from_public_key(spki: &[u8]) -> Option<String> {
    Some(id_from_digest(&cng::sha256(spki)?))
}

fn id_from_digest(digest: &[u8; 32]) -> String {
    id_from_bytes(&digest[..CRX_ID_LEN])
}

fn id_from_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push((b'a' + (b >> 4)) as char);
        out.push((b'a' + (b & 15)) as char);
    }
    out
}

fn is_valid_id(id: &str) -> bool {
    id.len() == CRX_ID_LEN * 2 && id.bytes().all(|b| (b'a'..=b'p').contains(&b))
}

/// Parses `crx`, verifies every proof over the archive, and requires the declared `crx_id` to equal
/// `expected_id` and to be derived by a verified proof's key. Any proof that does not verify rejects
/// the whole file, as in Chromium.
#[must_use = "an unchecked result means the CRX was not verified"]
pub fn verify_crx3<'a>(crx: &'a [u8], expected_id: &str) -> Result<VerifiedCrx<'a>, CrxError> {
    if !is_valid_id(expected_id) {
        return Err(CrxError::InvalidRequestedId);
    }
    let (header, archive) = split_frame(crx)?;
    let parsed = parse_header(header)?;
    if parsed.proofs.is_empty() {
        return Err(CrxError::NoProofs);
    }
    let signed = parsed
        .signed_header_data
        .ok_or(CrxError::MissingSignedHeaderData)?;
    let declared_id = id_from_bytes(parse_crx_id(signed)?);
    if declared_id != expected_id {
        return Err(CrxError::IdMismatch);
    }

    // Every proof signs the same bytes, so hash once.
    let digest = signed_digest(signed, archive)?;
    let mut verified = Vec::with_capacity(parsed.proofs.len());
    let mut developer_public_key = None;
    for proof in &parsed.proofs {
        let key = parse_spki(proof.public_key)?;
        if key.kind() != proof.kind {
            return Err(CrxError::KeyTypeMismatch);
        }
        if !key.verify(&digest, proof.signature) {
            return Err(CrxError::SignatureInvalid);
        }
        let key_sha256 = cng::sha256(proof.public_key).ok_or(CrxError::CryptoUnavailable)?;
        let derived_id = id_from_digest(&key_sha256);
        if derived_id == declared_id {
            developer_public_key.get_or_insert(proof.public_key);
        }
        verified.push(VerifiedProof {
            kind: proof.kind,
            derived_id,
            key_sha256,
        });
    }
    let developer_public_key = developer_public_key.ok_or(CrxError::DeveloperProofMissing)?;
    Ok(VerifiedCrx {
        extension_id: declared_id,
        archive,
        developer_public_key,
        proofs: verified,
    })
}

/// [`verify_crx3`] for a file the caller does not know the id of (one the user picked): reads the id
/// the file declares, then runs the full verification against it. The declared id is never trusted on
/// its own: `verify_crx3` still requires a verified proof by the key that derives it, so a CRX that
/// names someone else's id, or whose archive or header was altered, is rejected as before.
#[must_use = "an unchecked result means the CRX was not verified"]
pub fn verify_crx3_declared(crx: &[u8]) -> Result<VerifiedCrx<'_>, CrxError> {
    let (header, _) = split_frame(crx)?;
    let parsed = parse_header(header)?;
    if parsed.proofs.is_empty() {
        return Err(CrxError::NoProofs);
    }
    let signed = parsed
        .signed_header_data
        .ok_or(CrxError::MissingSignedHeaderData)?;
    let declared_id = id_from_bytes(parse_crx_id(signed)?);
    verify_crx3(crx, &declared_id)
}

/// Splits `crx` into the header bytes and the archive, checking magic, version and bounds. The header
/// length is checked against [`MAX_HEADER_LEN`] and the input before anything is sliced or parsed.
fn split_frame(crx: &[u8]) -> Result<(&[u8], &[u8]), CrxError> {
    if crx.len() < FIXED_PREFIX_LEN {
        return Err(CrxError::Truncated);
    }
    if &crx[..4] != MAGIC {
        return Err(CrxError::BadMagic);
    }
    let version = u32::from_le_bytes([crx[4], crx[5], crx[6], crx[7]]);
    if version != 3 {
        return Err(CrxError::UnsupportedVersion(version));
    }
    let header_len = u32::from_le_bytes([crx[8], crx[9], crx[10], crx[11]]);
    if header_len == 0 || header_len as usize > MAX_HEADER_LEN {
        return Err(CrxError::HeaderTooLarge(u64::from(header_len)));
    }
    let (header, archive) = crx[FIXED_PREFIX_LEN..]
        .split_at_checked(header_len as usize)
        .ok_or(CrxError::Truncated)?;
    if ZIP_TOKENS.iter().any(|t| contains(header, *t)) {
        return Err(CrxError::HeaderContainsZipToken);
    }
    if archive.is_empty() {
        return Err(CrxError::EmptyArchive);
    }
    Ok((header, archive))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

// ---------------------------------------------------------------------------------------------
// Bounded protobuf decoding: only the fields CRX3 verification needs
// ---------------------------------------------------------------------------------------------

enum WireValue<'a> {
    Varint,
    Fixed,
    Bytes(&'a [u8]),
}

fn read_varint(buf: &mut &[u8]) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..10).map(|i| i * 7) {
        let (&byte, rest) = buf.split_first()?;
        *buf = rest;
        // The tenth byte may only carry the top bit of a u64; anything more would be shifted out.
        if shift == 63 && byte & 0x7e != 0 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

/// Reads the next `(field number, value)` pair, or `Ok(None)` at a clean end. Lengths are checked
/// against the remaining input before any slice is taken.
fn next_field<'a>(buf: &mut &'a [u8]) -> Result<Option<(u32, WireValue<'a>)>, &'static str> {
    if buf.is_empty() {
        return Ok(None);
    }
    let tag = read_varint(buf).ok_or("bad tag")?;
    let number = u32::try_from(tag >> 3).map_err(|_| "field number too large")?;
    if number == 0 || number > (1 << 29) - 1 {
        return Err("invalid field number");
    }
    let value = match tag & 7 {
        0 => {
            read_varint(buf).ok_or("bad varint")?;
            WireValue::Varint
        }
        1 => {
            *buf = buf.get(8..).ok_or("truncated fixed64")?;
            WireValue::Fixed
        }
        2 => {
            let len = read_varint(buf).ok_or("bad length")?;
            let len = usize::try_from(len).map_err(|_| "length too large")?;
            let (bytes, rest) = buf.split_at_checked(len).ok_or("length exceeds message")?;
            *buf = rest;
            WireValue::Bytes(bytes)
        }
        5 => {
            *buf = buf.get(4..).ok_or("truncated fixed32")?;
            WireValue::Fixed
        }
        _ => return Err("unsupported wire type"),
    };
    Ok(Some((number, value)))
}

struct RawProof<'a> {
    kind: KeyKind,
    public_key: &'a [u8],
    signature: &'a [u8],
}

struct ParsedHeader<'a> {
    proofs: Vec<RawProof<'a>>,
    signed_header_data: Option<&'a [u8]>,
}

fn parse_header(header: &[u8]) -> Result<ParsedHeader<'_>, CrxError> {
    let mut rest = header;
    let mut parsed = ParsedHeader {
        proofs: Vec::new(),
        signed_header_data: None,
    };
    while let Some((number, value)) = next_field(&mut rest).map_err(CrxError::MalformedHeader)? {
        match (number, value) {
            (FIELD_SHA256_WITH_RSA | FIELD_SHA256_WITH_ECDSA, WireValue::Bytes(message)) => {
                if parsed.proofs.len() >= MAX_PROOFS {
                    return Err(CrxError::TooManyProofs);
                }
                let kind = if number == FIELD_SHA256_WITH_RSA {
                    KeyKind::Rsa
                } else {
                    KeyKind::EcdsaP256
                };
                parsed.proofs.push(parse_proof(kind, message)?);
            }
            (FIELD_SIGNED_HEADER_DATA, WireValue::Bytes(data)) => {
                if parsed.signed_header_data.replace(data).is_some() {
                    return Err(CrxError::MalformedHeader("duplicate signed_header_data"));
                }
            }
            (FIELD_SHA256_WITH_RSA | FIELD_SHA256_WITH_ECDSA | FIELD_SIGNED_HEADER_DATA, _) => {
                return Err(CrxError::MalformedHeader(
                    "known field has the wrong wire type",
                ));
            }
            _ => {}
        }
    }
    Ok(parsed)
}

fn parse_proof(kind: KeyKind, message: &[u8]) -> Result<RawProof<'_>, CrxError> {
    let mut rest = message;
    let (mut public_key, mut signature) = (None, None);
    while let Some((number, value)) = next_field(&mut rest).map_err(CrxError::MalformedProof)? {
        let slot = match number {
            1 => &mut public_key,
            2 => &mut signature,
            _ => continue,
        };
        let WireValue::Bytes(bytes) = value else {
            return Err(CrxError::MalformedProof("field has the wrong wire type"));
        };
        if slot.replace(bytes).is_some() {
            return Err(CrxError::MalformedProof("duplicate field"));
        }
    }
    match (public_key, signature) {
        (Some(public_key), Some(signature)) if !public_key.is_empty() && !signature.is_empty() => {
            Ok(RawProof {
                kind,
                public_key,
                signature,
            })
        }
        _ => Err(CrxError::MalformedProof("missing public_key or signature")),
    }
}

/// `SignedData { bytes crx_id = 1; }`, which must be exactly 16 bytes.
fn parse_crx_id(signed_header_data: &[u8]) -> Result<&[u8], CrxError> {
    let mut rest = signed_header_data;
    let mut crx_id = None;
    while let Some((number, value)) =
        next_field(&mut rest).map_err(CrxError::MalformedSignedData)?
    {
        if number != 1 {
            continue;
        }
        let WireValue::Bytes(bytes) = value else {
            return Err(CrxError::MalformedSignedData(
                "crx_id has the wrong wire type",
            ));
        };
        if crx_id.replace(bytes).is_some() {
            return Err(CrxError::MalformedSignedData("duplicate crx_id"));
        }
    }
    match crx_id {
        Some(id) if id.len() == CRX_ID_LEN => Ok(id),
        _ => Err(CrxError::MalformedSignedData("crx_id must be 16 bytes")),
    }
}

// ---------------------------------------------------------------------------------------------
// Public keys (DER SubjectPublicKeyInfo) and signatures
// ---------------------------------------------------------------------------------------------

enum PublicKey<'a> {
    Rsa {
        modulus: &'a [u8],
        exponent: &'a [u8],
    },
    EcdsaP256 {
        xy: [u8; 64],
    },
}

impl PublicKey<'_> {
    fn kind(&self) -> KeyKind {
        match self {
            Self::Rsa { .. } => KeyKind::Rsa,
            Self::EcdsaP256 { .. } => KeyKind::EcdsaP256,
        }
    }

    fn verify(&self, digest: &[u8; 32], signature: &[u8]) -> bool {
        match self {
            Self::Rsa { modulus, exponent } => {
                cng::rsa_pkcs1_sha256_verify(modulus, exponent, digest, signature)
            }
            Self::EcdsaP256 { xy } => ecdsa_der_to_p1363(signature)
                .is_some_and(|sig| cng::ecdsa_p256_verify(xy, digest, &sig)),
        }
    }
}

const OID_RSA_ENCRYPTION: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];
const OID_EC_PUBLIC_KEY: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];
const OID_PRIME256V1: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];

const TAG_INTEGER: u8 = 0x02;
const TAG_BIT_STRING: u8 = 0x03;
const TAG_NULL: u8 = 0x05;
const TAG_OID: u8 = 0x06;
const TAG_SEQUENCE: u8 = 0x30;

/// Reads one DER TLV, returning `(tag, content, rest)`. Definite minimal lengths only, no
/// high-tag-number form, and the content must fit in `input`.
fn der_read(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first()?;
    if tag & 0x1f == 0x1f {
        return None;
    }
    let (&first, rest) = rest.split_first()?;
    let (len, rest) = if first < 0x80 {
        (usize::from(first), rest)
    } else {
        let count = usize::from(first & 0x7f);
        if count == 0 || count > 3 {
            return None;
        }
        let (len_bytes, rest) = rest.split_at_checked(count)?;
        if len_bytes[0] == 0 {
            return None;
        }
        let len = len_bytes
            .iter()
            .fold(0usize, |acc, &b| acc << 8 | usize::from(b));
        if len < 0x80 {
            return None;
        }
        (len, rest)
    };
    let (content, rest) = rest.split_at_checked(len)?;
    Some((tag, content, rest))
}

/// Reads a TLV with the expected tag that must be the whole of `input`.
fn der_exact(input: &[u8], tag: u8) -> Option<&[u8]> {
    match der_read(input)? {
        (t, content, []) if t == tag => Some(content),
        _ => None,
    }
}

/// Magnitude of a non-negative, minimally encoded INTEGER (a lone zero stays `[0]`).
fn der_unsigned(content: &[u8]) -> Option<&[u8]> {
    let (&first, rest) = content.split_first()?;
    if first & 0x80 != 0 {
        return None;
    }
    if first == 0 && !rest.is_empty() {
        // A leading zero is only legal to keep the value non-negative.
        return (rest[0] & 0x80 != 0).then_some(rest);
    }
    Some(content)
}

fn parse_spki(spki: &[u8]) -> Result<PublicKey<'_>, CrxError> {
    const BAD: CrxError = CrxError::BadPublicKey("malformed SubjectPublicKeyInfo");
    let body = der_exact(spki, TAG_SEQUENCE).ok_or(BAD)?;
    let (tag, algorithm, rest) = der_read(body).ok_or(BAD)?;
    if tag != TAG_SEQUENCE {
        return Err(BAD);
    }
    let (tag, bits, rest) = der_read(rest).ok_or(BAD)?;
    if tag != TAG_BIT_STRING || !rest.is_empty() {
        return Err(BAD);
    }
    let (&unused_bits, key_bytes) = bits.split_first().ok_or(BAD)?;
    if unused_bits != 0 {
        return Err(BAD);
    }
    let (tag, oid, params) = der_read(algorithm).ok_or(BAD)?;
    if tag != TAG_OID {
        return Err(BAD);
    }
    if oid == OID_RSA_ENCRYPTION {
        // Parameters must be an explicit NULL.
        if !matches!(der_read(params), Some((TAG_NULL, [], []))) {
            return Err(BAD);
        }
        parse_rsa_public_key(key_bytes)
    } else if oid == OID_EC_PUBLIC_KEY {
        match der_read(params) {
            Some((TAG_OID, curve, [])) if curve == OID_PRIME256V1 => {}
            _ => return Err(CrxError::BadPublicKey("only P-256 is supported")),
        }
        // Uncompressed point: 0x04 || X || Y.
        match key_bytes {
            [0x04, xy @ ..] if xy.len() == 64 => Ok(PublicKey::EcdsaP256 {
                xy: xy.try_into().map_err(|_| BAD)?,
            }),
            _ => Err(CrxError::BadPublicKey(
                "expected an uncompressed P-256 point",
            )),
        }
    } else {
        Err(CrxError::BadPublicKey("unsupported key algorithm"))
    }
}

fn parse_rsa_public_key(der_key: &[u8]) -> Result<PublicKey<'_>, CrxError> {
    const BAD: CrxError = CrxError::BadPublicKey("malformed RSA public key");
    let body = der_exact(der_key, TAG_SEQUENCE).ok_or(BAD)?;
    let (tag, modulus, rest) = der_read(body).ok_or(BAD)?;
    if tag != TAG_INTEGER {
        return Err(BAD);
    }
    let (tag, exponent, rest) = der_read(rest).ok_or(BAD)?;
    if tag != TAG_INTEGER || !rest.is_empty() {
        return Err(BAD);
    }
    let modulus = der_unsigned(modulus).ok_or(BAD)?;
    let exponent = der_unsigned(exponent).ok_or(BAD)?;
    if modulus[0] == 0 || modulus[modulus.len() - 1] & 1 == 0 {
        return Err(BAD);
    }
    let bits = modulus.len() * 8 - modulus[0].leading_zeros() as usize;
    if !(MIN_RSA_BITS..=MAX_RSA_BITS).contains(&bits) {
        return Err(CrxError::BadPublicKey(
            "RSA modulus must be 1024 to 8192 bits",
        ));
    }
    // An odd exponent of at least 3 and at most 32 bits.
    if exponent.len() > 4
        || exponent[0] == 0
        || exponent[exponent.len() - 1] & 1 == 0
        || (exponent.len() == 1 && exponent[0] < 3)
    {
        return Err(BAD);
    }
    Ok(PublicKey::Rsa { modulus, exponent })
}

/// Converts an ASN.1 DER `ECDSA-Sig-Value` into fixed-width r||s for CNG.
fn ecdsa_der_to_p1363(signature: &[u8]) -> Option<[u8; 64]> {
    let body = der_exact(signature, TAG_SEQUENCE)?;
    let (tag_r, r, rest) = der_read(body)?;
    let (tag_s, s, rest) = der_read(rest)?;
    if tag_r != TAG_INTEGER || tag_s != TAG_INTEGER || !rest.is_empty() {
        return None;
    }
    let mut out = [0u8; 64];
    let (r_slot, s_slot) = out.split_at_mut(32);
    for (value, slot) in [(r, r_slot), (s, s_slot)] {
        let magnitude = der_unsigned(value)?;
        // Zero is not a valid r or s.
        let start = magnitude.iter().position(|&b| b != 0)?;
        let magnitude = &magnitude[start..];
        if magnitude.len() > 32 {
            return None;
        }
        slot[32 - magnitude.len()..].copy_from_slice(magnitude);
    }
    Some(out)
}

/// SHA-256 of the context string, the LE32 length of `signed_header_data`, the data, then the archive.
fn signed_digest(signed_header_data: &[u8], archive: &[u8]) -> Result<[u8; 32], CrxError> {
    let len = u32::try_from(signed_header_data.len())
        .map_err(|_| CrxError::MalformedSignedData("too long"))?;
    let mut hasher = cng::Sha256::new().ok_or(CrxError::CryptoUnavailable)?;
    let hashed = hasher.update(SIGNATURE_CONTEXT)
        && hasher.update(&len.to_le_bytes())
        && hasher.update(signed_header_data)
        && hasher.update(archive);
    if !hashed {
        return Err(CrxError::CryptoUnavailable);
    }
    hasher.finish().ok_or(CrxError::CryptoUnavailable)
}

// ---------------------------------------------------------------------------------------------
// Windows CNG: SHA-256, RSA PKCS#1 v1.5 and ECDSA P-256 verification
// ---------------------------------------------------------------------------------------------

mod cng {
    use std::{ffi::c_void, ptr::null_mut};

    use windows::{
        Win32::Security::Cryptography::{
            BCRYPT_ALG_HANDLE, BCRYPT_ECCKEY_BLOB, BCRYPT_ECCPUBLIC_BLOB,
            BCRYPT_ECDSA_P256_ALGORITHM, BCRYPT_ECDSA_PUBLIC_P256_MAGIC, BCRYPT_FLAGS,
            BCRYPT_HASH_HANDLE, BCRYPT_KEY_HANDLE, BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS,
            BCRYPT_PAD_PKCS1, BCRYPT_PKCS1_PADDING_INFO, BCRYPT_RSA_ALGORITHM,
            BCRYPT_RSAPUBLIC_BLOB, BCRYPT_SHA256_ALGORITHM, BCryptCloseAlgorithmProvider,
            BCryptCreateHash, BCryptDestroyHash, BCryptDestroyKey, BCryptFinishHash,
            BCryptHashData, BCryptImportKeyPair, BCryptOpenAlgorithmProvider,
            BCryptVerifySignature,
        },
        core::PCWSTR,
    };

    /// `BCRYPT_RSAPUBLIC_MAGIC`, "RSA1".
    const RSA_PUBLIC_MAGIC: u32 = 0x3141_5352;

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
            // CNG takes a u32 length; feed large archives in bounded chunks.
            data.chunks(1 << 24)
                .all(|chunk| unsafe { BCryptHashData(self.hash, chunk, 0) }.0 >= 0)
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

    /// Verifies an RSASSA-PKCS1-v1_5 SHA-256 signature over an already-computed digest. `modulus`
    /// and `exponent` are big-endian magnitudes without leading zeros.
    pub fn rsa_pkcs1_sha256_verify(
        modulus: &[u8],
        exponent: &[u8],
        digest: &[u8; 32],
        signature: &[u8],
    ) -> bool {
        // PKCS#1 v1.5 signatures are exactly one modulus long; CNG would also insist on it.
        if signature.len() != modulus.len() {
            return false;
        }
        let (Ok(bits), Ok(exp_len), Ok(mod_len)) = (
            u32::try_from(modulus.len() * 8),
            u32::try_from(exponent.len()),
            u32::try_from(modulus.len()),
        ) else {
            return false;
        };
        let Some(alg) = open(BCRYPT_RSA_ALGORITHM) else {
            return false;
        };
        let mut blob = Vec::with_capacity(24 + exponent.len() + modulus.len());
        for word in [RSA_PUBLIC_MAGIC, bits, exp_len, mod_len, 0, 0] {
            blob.extend_from_slice(&word.to_le_bytes());
        }
        blob.extend_from_slice(exponent);
        blob.extend_from_slice(modulus);
        let mut key = BCRYPT_KEY_HANDLE(null_mut());
        let imported =
            unsafe { BCryptImportKeyPair(alg, None, BCRYPT_RSAPUBLIC_BLOB, &mut key, &blob, 0) };
        let info = BCRYPT_PKCS1_PADDING_INFO {
            pszAlgId: BCRYPT_SHA256_ALGORITHM,
        };
        let verified = imported.0 >= 0
            && unsafe {
                BCryptVerifySignature(
                    key,
                    Some(&info as *const _ as *const c_void),
                    digest,
                    signature,
                    BCRYPT_PAD_PKCS1,
                )
            }
            .0 >= 0;
        unsafe {
            if imported.0 >= 0 {
                let _ = BCryptDestroyKey(key);
            }
            let _ = BCryptCloseAlgorithmProvider(alg, 0);
        }
        verified
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

    /// Test-only key pair so tests exercise the real verifiers with in-process keys.
    #[cfg(test)]
    pub struct TestKey {
        alg: BCRYPT_ALG_HANDLE,
        key: BCRYPT_KEY_HANDLE,
    }

    #[cfg(test)]
    impl TestKey {
        fn generate(algorithm: PCWSTR, bits: u32) -> Self {
            use windows::Win32::Security::Cryptography::{
                BCryptFinalizeKeyPair, BCryptGenerateKeyPair,
            };
            let alg = open(algorithm).expect("provider");
            let mut key = BCRYPT_KEY_HANDLE(null_mut());
            unsafe {
                assert!(BCryptGenerateKeyPair(alg, &mut key, bits, 0).0 >= 0);
                assert!(BCryptFinalizeKeyPair(key, 0).0 >= 0);
            }
            Self { alg, key }
        }

        fn export(&self, blob_type: PCWSTR) -> Vec<u8> {
            use windows::Win32::Security::Cryptography::BCryptExportKey;
            let mut blob = vec![0u8; 2048];
            let mut written = 0u32;
            unsafe {
                assert!(
                    BCryptExportKey(self.key, None, blob_type, Some(&mut blob), &mut written, 0).0
                        >= 0
                );
            }
            blob.truncate(written as usize);
            blob
        }

        /// Returns the key plus big-endian modulus and exponent.
        pub fn generate_rsa(bits: u32) -> (Self, Vec<u8>, Vec<u8>) {
            let key = Self::generate(BCRYPT_RSA_ALGORITHM, bits);
            let blob = key.export(BCRYPT_RSAPUBLIC_BLOB);
            let word = |i: usize| u32::from_le_bytes(blob[i * 4..i * 4 + 4].try_into().unwrap());
            let (exp_len, mod_len) = (word(2) as usize, word(3) as usize);
            let exponent = blob[24..24 + exp_len].to_vec();
            let modulus = blob[24 + exp_len..24 + exp_len + mod_len].to_vec();
            (key, modulus, exponent)
        }

        /// Returns the key plus the 64-byte X||Y public point.
        pub fn generate_ec() -> (Self, [u8; 64]) {
            let key = Self::generate(BCRYPT_ECDSA_P256_ALGORITHM, 256);
            let blob = key.export(BCRYPT_ECCPUBLIC_BLOB);
            assert_eq!(blob.len(), 72);
            let mut xy = [0u8; 64];
            xy.copy_from_slice(&blob[8..]);
            (key, xy)
        }

        fn sign(
            &self,
            padding: Option<*const c_void>,
            flags: BCRYPT_FLAGS,
            digest: &[u8; 32],
        ) -> Vec<u8> {
            use windows::Win32::Security::Cryptography::BCryptSignHash;
            let mut sig = vec![0u8; 1024];
            let mut written = 0u32;
            unsafe {
                assert!(
                    BCryptSignHash(
                        self.key,
                        padding,
                        digest,
                        Some(&mut sig),
                        &mut written,
                        flags
                    )
                    .0 >= 0
                );
            }
            sig.truncate(written as usize);
            sig
        }

        pub fn sign_rsa(&self, digest: &[u8; 32]) -> Vec<u8> {
            let info = BCRYPT_PKCS1_PADDING_INFO {
                pszAlgId: BCRYPT_SHA256_ALGORITHM,
            };
            self.sign(
                Some(&info as *const _ as *const c_void),
                BCRYPT_PAD_PKCS1,
                digest,
            )
        }

        pub fn sign_ec(&self, digest: &[u8; 32]) -> [u8; 64] {
            let sig = self.sign(None, BCRYPT_FLAGS(0), digest);
            sig.try_into().expect("P1363 signature is 64 bytes")
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const ARCHIVE: &[u8] = b"PK\x03\x04 pretend zip body for the extension archive";

    // ---- DER and protobuf encoders for building synthetic CRX3 files ----

    fn der(tag: u8, content: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        match content.len() {
            n if n < 0x80 => out.push(n as u8),
            n if n < 0x100 => out.extend_from_slice(&[0x81, n as u8]),
            n => out.extend_from_slice(&[0x82, (n >> 8) as u8, n as u8]),
        }
        out.extend_from_slice(content);
        out
    }

    fn der_uint(magnitude: &[u8]) -> Vec<u8> {
        let mut m = magnitude;
        while m.len() > 1 && m[0] == 0 {
            m = &m[1..];
        }
        let mut content = Vec::new();
        if m[0] & 0x80 != 0 {
            content.push(0);
        }
        content.extend_from_slice(m);
        der(0x02, &content)
    }

    const OID_RSA: &[u8] = &[
        0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01,
    ];
    const OID_EC: &[u8] = &[0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];
    const OID_P256: &[u8] = &[0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];

    fn rsa_spki(modulus: &[u8], exponent: &[u8]) -> Vec<u8> {
        let alg = der(0x30, &[OID_RSA, &[0x05, 0x00]].concat());
        let inner = der(0x30, &[der_uint(modulus), der_uint(exponent)].concat());
        let mut bits = vec![0u8];
        bits.extend_from_slice(&inner);
        der(0x30, &[alg, der(0x03, &bits)].concat())
    }

    fn ec_spki(xy: &[u8; 64]) -> Vec<u8> {
        let alg = der(0x30, &[OID_EC, OID_P256].concat());
        let mut bits = vec![0u8, 0x04];
        bits.extend_from_slice(xy);
        der(0x30, &[alg, der(0x03, &bits)].concat())
    }

    fn ec_der_signature(p1363: &[u8; 64]) -> Vec<u8> {
        der(
            0x30,
            &[der_uint(&p1363[..32]), der_uint(&p1363[32..])].concat(),
        )
    }

    fn varint(mut v: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let byte = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(byte);
                return out;
            }
            out.push(byte | 0x80);
        }
    }

    fn field(number: u32, data: &[u8]) -> Vec<u8> {
        let mut out = varint(u64::from(number) << 3 | 2);
        out.extend(varint(data.len() as u64));
        out.extend_from_slice(data);
        out
    }

    fn proof_message(spki: &[u8], signature: &[u8]) -> Vec<u8> {
        [field(1, spki), field(2, signature)].concat()
    }

    /// An in-process signer. Wraps a CNG key and knows its SPKI and header field.
    struct Signer {
        key: cng::TestKey,
        kind: KeyKind,
        spki: Vec<u8>,
    }

    impl Signer {
        fn rsa() -> Self {
            Self::rsa_bits(2048)
        }

        fn rsa_bits(bits: u32) -> Self {
            let (key, modulus, exponent) = cng::TestKey::generate_rsa(bits);
            Self {
                key,
                kind: KeyKind::Rsa,
                spki: rsa_spki(&modulus, &exponent),
            }
        }

        fn ec() -> Self {
            let (key, xy) = cng::TestKey::generate_ec();
            Self {
                key,
                kind: KeyKind::EcdsaP256,
                spki: ec_spki(&xy),
            }
        }

        fn id16(&self) -> [u8; 16] {
            cng::sha256(&self.spki).unwrap()[..16].try_into().unwrap()
        }

        fn id(&self) -> String {
            id_from_bytes(&self.id16())
        }

        fn key_sha256(&self) -> [u8; 32] {
            cng::sha256(&self.spki).unwrap()
        }

        fn header_field(&self) -> u32 {
            match self.kind {
                KeyKind::Rsa => FIELD_SHA256_WITH_RSA,
                KeyKind::EcdsaP256 => FIELD_SHA256_WITH_ECDSA,
            }
        }

        fn sign(&self, digest: &[u8; 32]) -> Vec<u8> {
            match self.kind {
                KeyKind::Rsa => self.key.sign_rsa(digest),
                KeyKind::EcdsaP256 => ec_der_signature(&self.key.sign_ec(digest)),
            }
        }

        /// A header field carrying this key's proof over `archive`.
        fn proof_over(&self, signed: &[u8], archive: &[u8]) -> Vec<u8> {
            let digest = independent_digest(signed, archive);
            field(
                self.header_field(),
                &proof_message(&self.spki, &self.sign(&digest)),
            )
        }
    }

    /// Signed bytes built by plain concatenation, independent of the production code.
    fn independent_digest(signed: &[u8], archive: &[u8]) -> [u8; 32] {
        let mut bytes = b"CRX3 SignedData\x00".to_vec();
        bytes.extend_from_slice(&(signed.len() as u32).to_le_bytes());
        bytes.extend_from_slice(signed);
        bytes.extend_from_slice(archive);
        cng::sha256(&bytes).unwrap()
    }

    fn signed_data(id: &[u8; 16]) -> Vec<u8> {
        field(1, id)
    }

    fn assemble(version: u32, header: &[u8], archive: &[u8]) -> Vec<u8> {
        let mut out = b"Cr24".to_vec();
        out.extend_from_slice(&version.to_le_bytes());
        out.extend_from_slice(&(header.len() as u32).to_le_bytes());
        out.extend_from_slice(header);
        out.extend_from_slice(archive);
        out
    }

    /// A CRX3 with one developer proof over `ARCHIVE`.
    fn simple_crx(dev: &Signer) -> Vec<u8> {
        let signed = signed_data(&dev.id16());
        let header = [
            dev.proof_over(&signed, ARCHIVE),
            field(FIELD_SIGNED_HEADER_DATA, &signed),
        ]
        .concat();
        assemble(3, &header, ARCHIVE)
    }

    /// A CRX3 carrying `archive`, signed in-process by a fresh RSA developer key, for other modules'
    /// tests. Returns the CRX bytes, the extension id it declares and the developer SPKI. Pass the
    /// bytes to [`verify_crx3`] with that id: there is no other way to get a [`VerifiedCrx`].
    pub(crate) fn signed_crx_for_tests(archive: &[u8]) -> (Vec<u8>, String, Vec<u8>) {
        let dev = Signer::rsa();
        let signed = signed_data(&dev.id16());
        let header = [
            dev.proof_over(&signed, archive),
            field(FIELD_SIGNED_HEADER_DATA, &signed),
        ]
        .concat();
        (assemble(3, &header, archive), dev.id(), dev.spki.clone())
    }

    fn header_of(crx: &[u8]) -> &[u8] {
        let n = u32::from_le_bytes(crx[8..12].try_into().unwrap()) as usize;
        &crx[12..12 + n]
    }

    // ---- known answers ----

    #[test]
    fn sha256_matches_known_answer() {
        let digest = cng::sha256(b"abc").unwrap();
        let expected: [u8; 32] = [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ];
        assert_eq!(digest, expected);
    }

    #[test]
    fn extension_id_is_first_sixteen_digest_bytes_mapped_to_a_p() {
        // SHA-256("abc") starts ba 78 16 bf 8f 01 cf ea 41 41 40 de 5d ae 22 23.
        assert_eq!(
            extension_id_from_public_key(b"abc").unwrap(),
            "lkhibglpipabmpokebebeanofnkocccd"
        );
    }

    #[test]
    fn signed_digest_covers_context_length_data_and_archive() {
        let signed = signed_data(&[7u8; 16]);
        assert_eq!(
            signed_digest(&signed, ARCHIVE).unwrap(),
            independent_digest(&signed, ARCHIVE)
        );
    }

    // ---- accept ----

    #[test]
    fn accepts_rsa_developer_proof() {
        let dev = Signer::rsa();
        let crx = simple_crx(&dev);
        let verified = verify_crx3(&crx, &dev.id()).unwrap();
        assert_eq!(verified.extension_id(), dev.id());
        assert_eq!(verified.archive(), ARCHIVE);
        assert_eq!(verified.developer_public_key(), dev.spki.as_slice());
        assert_eq!(verified.proofs().len(), 1);
        assert_eq!(verified.proofs()[0].kind(), KeyKind::Rsa);
        assert_eq!(verified.proofs()[0].derived_id(), dev.id());
        assert_eq!(verified.proofs()[0].key_sha256(), &dev.key_sha256());
    }

    #[test]
    fn developer_public_key_is_the_one_that_owns_the_id_not_the_publisher() {
        let dev = Signer::rsa();
        let publisher = Signer::ec();
        let signed = signed_data(&dev.id16());
        // Publisher proof first, so "first proof" would be the wrong answer.
        let header = [
            publisher.proof_over(&signed, ARCHIVE),
            dev.proof_over(&signed, ARCHIVE),
            field(FIELD_SIGNED_HEADER_DATA, &signed),
        ]
        .concat();
        let crx = assemble(3, &header, ARCHIVE);
        let verified = verify_crx3(&crx, &dev.id()).unwrap();
        assert_eq!(verified.developer_public_key(), dev.spki.as_slice());
        assert_eq!(
            extension_id_from_public_key(verified.developer_public_key()).unwrap(),
            dev.id()
        );
    }

    #[test]
    fn accepts_ecdsa_developer_proof() {
        let dev = Signer::ec();
        let crx = simple_crx(&dev);
        let verified = verify_crx3(&crx, &dev.id()).unwrap();
        assert_eq!(verified.proofs()[0].kind(), KeyKind::EcdsaP256);
    }

    #[test]
    fn publisher_pin_accepts_pinned_key_and_rejects_unpinned_or_empty() {
        let dev = Signer::rsa();
        let publisher = Signer::ec();
        let other = Signer::ec();
        let signed = signed_data(&dev.id16());
        let header = [
            dev.proof_over(&signed, ARCHIVE),
            publisher.proof_over(&signed, ARCHIVE),
            field(FIELD_SIGNED_HEADER_DATA, &signed),
        ]
        .concat();
        let crx = assemble(3, &header, ARCHIVE);
        let verified = verify_crx3(&crx, &dev.id()).unwrap();
        assert_eq!(verified.proofs().len(), 2);

        // Any one matching pin is enough (Chromium's publisher-key check), not all of them.
        assert_eq!(
            verified.require_pinned_publisher(&[other.key_sha256(), publisher.key_sha256()]),
            Ok(())
        );
        assert_eq!(
            verified.require_pinned_publisher(&[other.key_sha256()]),
            Err(CrxError::PublisherProofMissing)
        );
        assert_eq!(
            verified.require_pinned_publisher(&[]),
            Err(CrxError::PublisherProofMissing)
        );
    }

    #[test]
    fn developer_only_crx_fails_publisher_pin() {
        let dev = Signer::rsa();
        let other = Signer::ec();
        let crx = simple_crx(&dev);
        let verified = verify_crx3(&crx, &dev.id()).unwrap();
        assert_eq!(
            verified.require_pinned_publisher(&[other.key_sha256()]),
            Err(CrxError::PublisherProofMissing)
        );
    }

    #[test]
    fn unknown_header_fields_are_skipped() {
        let dev = Signer::ec();
        let signed = signed_data(&dev.id16());
        let header = [
            field(4, b"verified contents are not our business"),
            dev.proof_over(&signed, ARCHIVE),
            field(FIELD_SIGNED_HEADER_DATA, &signed),
            varint(77 << 3).into_iter().chain(varint(5)).collect(),
        ]
        .concat();
        let crx = assemble(3, &header, ARCHIVE);
        assert!(verify_crx3(&crx, &dev.id()).is_ok());
    }

    // ---- reject: archive, ids, proofs ----

    #[test]
    fn rejects_one_tampered_archive_byte() {
        let dev = Signer::rsa();
        let mut crx = simple_crx(&dev);
        let last = crx.len() - 1;
        crx[last] ^= 1;
        assert_eq!(
            verify_crx3(&crx, &dev.id()),
            Err(CrxError::SignatureInvalid)
        );
    }

    #[test]
    fn rejects_tampered_ecdsa_archive_byte() {
        let dev = Signer::ec();
        let mut crx = simple_crx(&dev);
        let first_archive_byte = 12 + header_of(&crx).len();
        crx[first_archive_byte + 10] ^= 0x40;
        assert_eq!(
            verify_crx3(&crx, &dev.id()),
            Err(CrxError::SignatureInvalid)
        );
    }

    #[test]
    fn rejects_tampered_signature_byte() {
        let dev = Signer::rsa();
        let mut crx = simple_crx(&dev);
        // The RSA signature is the tail of the proof; flip a byte near the end of the header.
        let end_of_header = 12 + header_of(&crx).len();
        let signed_field_len = field(FIELD_SIGNED_HEADER_DATA, &signed_data(&dev.id16())).len();
        crx[end_of_header - signed_field_len - 5] ^= 1;
        assert_eq!(
            verify_crx3(&crx, &dev.id()),
            Err(CrxError::SignatureInvalid)
        );
    }

    #[test]
    fn rejects_wrong_requested_id() {
        let dev = Signer::rsa();
        let other = Signer::rsa();
        let crx = simple_crx(&dev);
        assert_eq!(verify_crx3(&crx, &other.id()), Err(CrxError::IdMismatch));
    }

    // ---- verify against the id the file declares ----

    #[test]
    fn declared_id_verifier_reads_the_id_and_still_verifies_the_signature() {
        for dev in [Signer::rsa(), Signer::ec()] {
            let crx = simple_crx(&dev);
            let verified = verify_crx3_declared(&crx).unwrap();
            assert_eq!(verified.extension_id(), dev.id());
            assert_eq!(verified.archive(), ARCHIVE);
            assert_eq!(verified.developer_public_key(), dev.spki.as_slice());
        }
    }

    #[test]
    fn declared_id_verifier_rejects_a_tampered_archive_or_declared_id() {
        let dev = Signer::rsa();
        let mut archive_hit = simple_crx(&dev);
        let last = archive_hit.len() - 1;
        archive_hit[last] ^= 1;
        assert_eq!(verify_crx3_declared(&archive_hit), Err(CrxError::SignatureInvalid));

        // The declared id is the last header byte: signed_header_data is the last field.
        let mut id_hit = simple_crx(&dev);
        let last_header_byte = 12 + header_of(&id_hit).len() - 1;
        id_hit[last_header_byte] ^= 1;
        assert_eq!(verify_crx3_declared(&id_hit), Err(CrxError::SignatureInvalid));
    }

    #[test]
    fn declared_id_verifier_does_not_trust_an_id_its_signer_does_not_own() {
        // A valid signature by `dev` over data that names somebody else's id.
        let dev = Signer::rsa();
        let victim = Signer::rsa();
        let signed = signed_data(&victim.id16());
        let header = [
            dev.proof_over(&signed, ARCHIVE),
            field(FIELD_SIGNED_HEADER_DATA, &signed),
        ]
        .concat();
        let crx = assemble(3, &header, ARCHIVE);
        assert_eq!(verify_crx3_declared(&crx), Err(CrxError::DeveloperProofMissing));
    }

    #[test]
    fn declared_id_verifier_passes_structural_errors_through() {
        assert_eq!(verify_crx3_declared(b"Cr24"), Err(CrxError::Truncated));
        let dev = Signer::rsa();
        let mut crx = simple_crx(&dev);
        crx[0] = b'X';
        assert_eq!(verify_crx3_declared(&crx), Err(CrxError::BadMagic));
        let no_signed_data = assemble(3, &dev.proof_over(&signed_data(&dev.id16()), ARCHIVE), ARCHIVE);
        assert_eq!(verify_crx3_declared(&no_signed_data), Err(CrxError::MissingSignedHeaderData));
        assert_eq!(verify_crx3_declared(&assemble(3, &field(4, b"x"), ARCHIVE)), Err(CrxError::NoProofs));
    }

    #[test]
    fn rejects_malformed_requested_id() {
        let dev = Signer::rsa();
        let crx = simple_crx(&dev);
        for bad in [
            "",
            "short",
            &dev.id().to_uppercase(),
            &format!("{}q", &dev.id()[1..]),
        ] {
            assert_eq!(
                verify_crx3(&crx, bad),
                Err(CrxError::InvalidRequestedId),
                "{bad}"
            );
        }
    }

    #[test]
    fn rejects_valid_proof_by_a_key_that_does_not_own_the_crx_id() {
        let owner = Signer::rsa();
        let impostor = Signer::rsa();
        let signed = signed_data(&owner.id16());
        let header = [
            impostor.proof_over(&signed, ARCHIVE),
            field(FIELD_SIGNED_HEADER_DATA, &signed),
        ]
        .concat();
        let crx = assemble(3, &header, ARCHIVE);
        assert_eq!(
            verify_crx3(&crx, &owner.id()),
            Err(CrxError::DeveloperProofMissing)
        );
    }

    #[test]
    fn rejects_when_any_proof_is_invalid_even_if_developer_proof_is_good() {
        let dev = Signer::rsa();
        let publisher = Signer::ec();
        let signed = signed_data(&dev.id16());
        let wrong_archive = proof_message(
            &publisher.spki,
            &publisher.sign(&independent_digest(&signed, b"another archive")),
        );
        let header = [
            dev.proof_over(&signed, ARCHIVE),
            field(FIELD_SHA256_WITH_ECDSA, &wrong_archive),
            field(FIELD_SIGNED_HEADER_DATA, &signed),
        ]
        .concat();
        let crx = assemble(3, &header, ARCHIVE);
        assert_eq!(
            verify_crx3(&crx, &dev.id()),
            Err(CrxError::SignatureInvalid)
        );
    }

    #[test]
    fn rejects_proof_that_does_not_cover_the_archive() {
        let dev = Signer::rsa();
        let signed = signed_data(&dev.id16());
        // Signs context + length + signed_header_data but leaves the archive out.
        let digest = independent_digest(&signed, b"");
        let proof = field(
            FIELD_SHA256_WITH_RSA,
            &proof_message(&dev.spki, &dev.sign(&digest)),
        );
        let header = [proof, field(FIELD_SIGNED_HEADER_DATA, &signed)].concat();
        let crx = assemble(3, &header, ARCHIVE);
        assert_eq!(
            verify_crx3(&crx, &dev.id()),
            Err(CrxError::SignatureInvalid)
        );
    }

    #[test]
    fn rejects_proof_that_signs_the_archive_without_the_context_prefix() {
        let dev = Signer::ec();
        let signed = signed_data(&dev.id16());
        let digest = cng::sha256(ARCHIVE).unwrap();
        let proof = field(
            FIELD_SHA256_WITH_ECDSA,
            &proof_message(&dev.spki, &dev.sign(&digest)),
        );
        let header = [proof, field(FIELD_SIGNED_HEADER_DATA, &signed)].concat();
        let crx = assemble(3, &header, ARCHIVE);
        assert_eq!(
            verify_crx3(&crx, &dev.id()),
            Err(CrxError::SignatureInvalid)
        );
    }

    #[test]
    fn rejects_ecdsa_key_in_the_rsa_field_and_rsa_key_in_the_ecdsa_field() {
        for (dev, wrong_field) in [
            (Signer::ec(), FIELD_SHA256_WITH_RSA),
            (Signer::rsa(), FIELD_SHA256_WITH_ECDSA),
        ] {
            let signed = signed_data(&dev.id16());
            let digest = independent_digest(&signed, ARCHIVE);
            let proof = field(wrong_field, &proof_message(&dev.spki, &dev.sign(&digest)));
            let header = [proof, field(FIELD_SIGNED_HEADER_DATA, &signed)].concat();
            let crx = assemble(3, &header, ARCHIVE);
            assert_eq!(verify_crx3(&crx, &dev.id()), Err(CrxError::KeyTypeMismatch));
        }
    }

    #[test]
    fn accepts_1024_bit_rsa_developer_key_and_rejects_512_bit() {
        // Chromium accepts 1024-bit developer keys (older packers used them); BoringSSL's floor is 512.
        let dev = Signer::rsa_bits(1024);
        let crx = simple_crx(&dev);
        assert!(verify_crx3(&crx, &dev.id()).is_ok());

        let weak = Signer::rsa_bits(512);
        let crx = simple_crx(&weak);
        assert!(matches!(
            verify_crx3(&crx, &weak.id()),
            Err(CrxError::BadPublicKey(_))
        ));
    }

    #[test]
    fn rejects_rsa_signature_of_the_wrong_length() {
        let dev = Signer::rsa();
        let signed = signed_data(&dev.id16());
        let digest = independent_digest(&signed, ARCHIVE);
        let mut sig = dev.sign(&digest);
        sig.pop();
        let header = [
            field(FIELD_SHA256_WITH_RSA, &proof_message(&dev.spki, &sig)),
            field(FIELD_SIGNED_HEADER_DATA, &signed),
        ]
        .concat();
        let crx = assemble(3, &header, ARCHIVE);
        assert_eq!(
            verify_crx3(&crx, &dev.id()),
            Err(CrxError::SignatureInvalid)
        );
    }

    #[test]
    fn rejects_malformed_ecdsa_signature_encodings() {
        let dev = Signer::ec();
        let signed = signed_data(&dev.id16());
        let digest = independent_digest(&signed, ARCHIVE);
        let good = dev.sign(&digest);
        let mut trailing = good.clone();
        trailing.push(0);
        let raw_p1363 = dev.key.sign_ec(&digest).to_vec();
        for bad in [trailing, raw_p1363, vec![0x30, 0x00]] {
            let header = [
                field(FIELD_SHA256_WITH_ECDSA, &proof_message(&dev.spki, &bad)),
                field(FIELD_SIGNED_HEADER_DATA, &signed),
            ]
            .concat();
            let crx = assemble(3, &header, ARCHIVE);
            assert_eq!(
                verify_crx3(&crx, &dev.id()),
                Err(CrxError::SignatureInvalid)
            );
        }
    }

    #[test]
    fn rejects_garbage_public_key() {
        let dev = Signer::rsa();
        let signed = signed_data(&dev.id16());
        let proof = field(FIELD_SHA256_WITH_RSA, &proof_message(b"not a key", b"sig"));
        let header = [proof, field(FIELD_SIGNED_HEADER_DATA, &signed)].concat();
        let crx = assemble(3, &header, ARCHIVE);
        assert!(matches!(
            verify_crx3(&crx, &dev.id()),
            Err(CrxError::BadPublicKey(_))
        ));
    }

    #[test]
    fn rejects_no_proofs() {
        let dev = Signer::rsa();
        let signed = signed_data(&dev.id16());
        let header = field(FIELD_SIGNED_HEADER_DATA, &signed);
        let crx = assemble(3, &header, ARCHIVE);
        assert_eq!(verify_crx3(&crx, &dev.id()), Err(CrxError::NoProofs));
    }

    #[test]
    fn rejects_proof_missing_key_or_signature() {
        let dev = Signer::rsa();
        let signed = signed_data(&dev.id16());
        for message in [field(1, &dev.spki), field(2, b"sig"), Vec::new()] {
            let header = [
                field(FIELD_SHA256_WITH_RSA, &message),
                field(FIELD_SIGNED_HEADER_DATA, &signed),
            ]
            .concat();
            let crx = assemble(3, &header, ARCHIVE);
            assert!(
                matches!(
                    verify_crx3(&crx, &dev.id()),
                    Err(CrxError::MalformedProof(_))
                ),
                "{message:?}"
            );
        }
    }

    #[test]
    fn rejects_too_many_proofs() {
        let dev = Signer::ec();
        let signed = signed_data(&dev.id16());
        let proof = dev.proof_over(&signed, ARCHIVE);
        let mut header = field(FIELD_SIGNED_HEADER_DATA, &signed);
        for _ in 0..=MAX_PROOFS {
            header.extend_from_slice(&proof);
        }
        let crx = assemble(3, &header, ARCHIVE);
        assert_eq!(verify_crx3(&crx, &dev.id()), Err(CrxError::TooManyProofs));
    }

    // ---- reject: signed_header_data ----

    #[test]
    fn rejects_missing_signed_header_data() {
        let dev = Signer::rsa();
        let signed = signed_data(&dev.id16());
        let header = dev.proof_over(&signed, ARCHIVE);
        let crx = assemble(3, &header, ARCHIVE);
        assert_eq!(
            verify_crx3(&crx, &dev.id()),
            Err(CrxError::MissingSignedHeaderData)
        );
    }

    #[test]
    fn rejects_crx_id_that_is_not_sixteen_bytes() {
        let dev = Signer::rsa();
        for signed in [
            field(1, &dev.id16()[..15]),
            field(1, &[dev.id16().to_vec(), vec![0]].concat()),
            Vec::new(),
        ] {
            let header = [
                dev.proof_over(&signed, ARCHIVE),
                field(FIELD_SIGNED_HEADER_DATA, &signed),
            ]
            .concat();
            let crx = assemble(3, &header, ARCHIVE);
            assert!(
                matches!(
                    verify_crx3(&crx, &dev.id()),
                    Err(CrxError::MalformedSignedData(_))
                ),
                "{signed:?}"
            );
        }
    }

    #[test]
    fn rejects_duplicate_signed_header_data() {
        let dev = Signer::rsa();
        let signed = signed_data(&dev.id16());
        let header = [
            dev.proof_over(&signed, ARCHIVE),
            field(FIELD_SIGNED_HEADER_DATA, &signed),
            field(FIELD_SIGNED_HEADER_DATA, &signed),
        ]
        .concat();
        let crx = assemble(3, &header, ARCHIVE);
        assert!(matches!(
            verify_crx3(&crx, &dev.id()),
            Err(CrxError::MalformedHeader(_))
        ));
    }

    // ---- reject: framing ----

    #[test]
    fn rejects_zip_tokens_in_the_header() {
        let dev = Signer::rsa();
        let signed = signed_data(&dev.id16());
        for token in ZIP_TOKENS {
            // An unknown field keeps the header otherwise valid and correctly signed, so only the
            // token check can reject it.
            let header = [
                dev.proof_over(&signed, ARCHIVE),
                field(FIELD_SIGNED_HEADER_DATA, &signed),
                field(
                    9,
                    &[b"padding".as_slice(), token.as_slice(), b"tail"].concat(),
                ),
            ]
            .concat();
            let crx = assemble(3, &header, ARCHIVE);
            assert_eq!(
                verify_crx3(&crx, &dev.id()),
                Err(CrxError::HeaderContainsZipToken),
                "{token:?}"
            );
        }
    }

    #[test]
    fn rejects_version_2_and_other_versions() {
        let dev = Signer::rsa();
        let mut crx = simple_crx(&dev);
        for version in [2u32, 0, 4, u32::MAX] {
            crx[4..8].copy_from_slice(&version.to_le_bytes());
            assert_eq!(
                verify_crx3(&crx, &dev.id()),
                Err(CrxError::UnsupportedVersion(version))
            );
        }
    }

    #[test]
    fn rejects_bad_magic() {
        let dev = Signer::rsa();
        let mut crx = simple_crx(&dev);
        crx[0] = b'X';
        assert_eq!(verify_crx3(&crx, &dev.id()), Err(CrxError::BadMagic));
    }

    #[test]
    fn header_cap_is_sixteen_mebibytes() {
        assert_eq!(MAX_HEADER_LEN, 16 << 20);
    }

    #[test]
    fn header_of_a_few_kilobytes_with_verified_contents_still_verifies() {
        let dev = Signer::rsa();
        let signed = signed_data(&dev.id16());
        // Web Store files carry verified_contents (field 4), which grows with the file count.
        let header = [
            field(4, &vec![0xa5u8; 6000]),
            dev.proof_over(&signed, ARCHIVE),
            field(FIELD_SIGNED_HEADER_DATA, &signed),
        ]
        .concat();
        assert!(header.len() > 4096);
        let crx = assemble(3, &header, ARCHIVE);
        assert!(verify_crx3(&crx, &dev.id()).is_ok());
    }

    #[test]
    fn rejects_oversize_header_without_reading_or_allocating_it() {
        let dev = Signer::rsa();
        // Only the 12-byte prefix is present: the length alone must be enough to reject.
        for n in [(16u32 << 20) + 1, u32::MAX, 0x8000_0000] {
            let mut prefix = b"Cr24".to_vec();
            prefix.extend_from_slice(&3u32.to_le_bytes());
            prefix.extend_from_slice(&n.to_le_bytes());
            assert_eq!(
                verify_crx3(&prefix, &dev.id()),
                Err(CrxError::HeaderTooLarge(u64::from(n)))
            );
        }
    }

    #[test]
    fn header_at_exactly_the_cap_is_a_length_check_not_a_size_rejection() {
        let dev = Signer::rsa();
        let mut prefix = b"Cr24".to_vec();
        prefix.extend_from_slice(&3u32.to_le_bytes());
        prefix.extend_from_slice(&(16u32 << 20).to_le_bytes());
        // Within the cap but the bytes are not there.
        assert_eq!(verify_crx3(&prefix, &dev.id()), Err(CrxError::Truncated));
    }

    #[test]
    fn rejects_zero_length_header() {
        let dev = Signer::rsa();
        let crx = assemble(3, b"", ARCHIVE);
        assert_eq!(
            verify_crx3(&crx, &dev.id()),
            Err(CrxError::HeaderTooLarge(0))
        );
    }

    #[test]
    fn rejects_truncated_input_at_every_prefix_length() {
        let dev = Signer::ec();
        let crx = simple_crx(&dev);
        let archive_start = 12 + header_of(&crx).len();
        for len in 0..=archive_start {
            assert!(
                verify_crx3(&crx[..len], &dev.id()).is_err(),
                "prefix of {len} bytes must not verify"
            );
        }
        assert_eq!(verify_crx3(&crx[..11], &dev.id()), Err(CrxError::Truncated));
        assert_eq!(
            verify_crx3(&crx[..archive_start - 1], &dev.id()),
            Err(CrxError::Truncated)
        );
        assert_eq!(
            verify_crx3(&crx[..archive_start], &dev.id()),
            Err(CrxError::EmptyArchive)
        );
    }

    // ---- protobuf decoder bounds ----

    fn crx_with_raw_header(header: &[u8], id: &str) -> Result<(), CrxError> {
        let crx = assemble(3, header, ARCHIVE);
        verify_crx3(&crx, id).map(|_| ())
    }

    #[test]
    fn protobuf_length_beyond_the_header_is_rejected() {
        let id = id_from_bytes(&[0u8; 16]);
        // Field 2, length-delimited, claims 1000 bytes with none present.
        let header = [varint(2 << 3 | 2), varint(1000)].concat();
        assert!(matches!(
            crx_with_raw_header(&header, &id),
            Err(CrxError::MalformedHeader(_))
        ));
        // A length near u64::MAX must not wrap or allocate.
        let header = [varint(2 << 3 | 2), varint(u64::MAX)].concat();
        assert!(matches!(
            crx_with_raw_header(&header, &id),
            Err(CrxError::MalformedHeader(_))
        ));
    }

    #[test]
    fn protobuf_unterminated_overlong_varint_is_rejected() {
        let id = id_from_bytes(&[0u8; 16]);
        assert!(matches!(
            crx_with_raw_header(&[0x80; 11], &id),
            Err(CrxError::MalformedHeader(_))
        ));
        assert!(matches!(
            crx_with_raw_header(&[0x10, 0x80], &id),
            Err(CrxError::MalformedHeader(_))
        ));
    }

    #[test]
    fn protobuf_groups_field_zero_and_known_fields_with_wrong_wire_type_are_rejected() {
        let id = id_from_bytes(&[0u8; 16]);
        for header in [
            vec![(5 << 3) | 3],             // start-group
            vec![(5 << 3) | 4],             // end-group
            vec![0x00, 0x01],               // field number 0
            vec![(5 << 3) | 7],             // reserved wire type
            vec![(2 << 3), 0x01],           // field 2 as varint
            vec![(3 << 3) | 5, 1, 2, 3, 4], // field 3 as fixed32
            vec![(5 << 3) | 1, 1, 2, 3],    // fixed64 cut short
        ] {
            assert!(
                matches!(
                    crx_with_raw_header(&header, &id),
                    Err(CrxError::MalformedHeader(_))
                ),
                "{header:?}"
            );
        }
    }

    #[test]
    fn protobuf_fixed_width_and_varint_unknown_fields_are_skipped_in_order() {
        let dev = Signer::rsa();
        let signed = signed_data(&dev.id16());
        let header = [
            vec![(5 << 3) | 1, 1, 2, 3, 4, 5, 6, 7, 8],
            vec![(6 << 3) | 5, 1, 2, 3, 4],
            [varint(7 << 3), varint(u64::MAX)].concat(),
            dev.proof_over(&signed, ARCHIVE),
            field(FIELD_SIGNED_HEADER_DATA, &signed),
        ]
        .concat();
        let crx = assemble(3, &header, ARCHIVE);
        assert!(verify_crx3(&crx, &dev.id()).is_ok());
    }

    // ---- DER helpers ----

    #[test]
    fn der_reader_rejects_non_minimal_and_truncated_lengths() {
        assert!(der_read(&[0x30, 0x81, 0x05, 0, 0, 0, 0, 0]).is_none()); // long form for a short length
        assert!(der_read(&[0x30, 0x82, 0x00, 0x90]).is_none()); // leading zero length byte
        assert!(der_read(&[0x30, 0x05, 1, 2]).is_none()); // content shorter than declared
        assert!(der_read(&[0x30, 0x84, 0xff, 0xff, 0xff, 0xff]).is_none()); // absurd length
        assert!(der_read(&[0x1f, 0x01, 0x00]).is_none()); // high tag number
        assert!(der_read(&[]).is_none());
    }

    #[test]
    fn ecdsa_der_conversion_pads_short_integers_and_rejects_oversize_and_zero() {
        let mut p1363 = [0u8; 64];
        p1363[31] = 5; // r = 5, leading zeros stripped by DER
        p1363[32] = 0x80; // s has its high bit set, so DER prepends 0x00
        p1363[63] = 1;
        assert_eq!(ecdsa_der_to_p1363(&ec_der_signature(&p1363)), Some(p1363));

        let too_big = der(0x30, &[der_uint(&[1u8; 33]), der_uint(&[1])].concat());
        assert_eq!(ecdsa_der_to_p1363(&too_big), None);
        let zero = der(0x30, &[der_uint(&[0]), der_uint(&[1])].concat());
        assert_eq!(ecdsa_der_to_p1363(&zero), None);
        let negative = der(0x30, &[der(0x02, &[0x80]), der_uint(&[1])].concat());
        assert_eq!(ecdsa_der_to_p1363(&negative), None);
    }
}
