//! Favicon decoding through Windows Imaging Component and a small LRU cache.

use crate::tabs::FaviconKey;

const MAX_CACHE_ENTRIES: usize = 64;

/// A tightly packed, top-down BGRA image with premultiplied color channels.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FaviconBitmap {
    pub width: u32,
    pub height: u32,
    pub bgra_premultiplied: Vec<u8>,
}

/// Identifies favicon bytes by the first 64 bits of their SHA-256 digest. The cache also compares the
/// full digest, so equal keys never share a bitmap.
pub fn key_for(bytes: &[u8]) -> FaviconKey {
    let digest = sha256(bytes);
    FaviconKey(u64::from_be_bytes(
        digest[..8].try_into().expect("digest is 32 bytes"),
    ))
}

/// A bounded least-recently-used cache. The most recently accessed item is last.
#[derive(Default)]
pub struct FaviconCache {
    entries: Vec<CacheEntry>,
}

struct CacheEntry {
    key: FaviconKey,
    /// Full SHA-256 of the source bytes. A key match alone is not a hit.
    digest: [u8; 32],
    bitmap: FaviconBitmap,
}

impl FaviconCache {
    /// Stores `bitmap` as the decoding of `source`. Only an entry with the same key and digest is
    /// replaced, so a colliding favicon cannot evict another site's bitmap.
    pub fn insert(&mut self, key: FaviconKey, source: &[u8], bitmap: FaviconBitmap) {
        let digest = sha256(source);
        if let Some(index) = self.position(key, &digest) {
            self.entries.remove(index);
        }
        if self.entries.len() == MAX_CACHE_ENTRIES {
            self.entries.remove(0);
        }
        self.entries.push(CacheEntry {
            key,
            digest,
            bitmap,
        });
    }

    /// Returns the bitmap for `source` only when both its key and its full digest match.
    pub fn get(&mut self, key: FaviconKey, source: &[u8]) -> Option<&FaviconBitmap> {
        let digest = sha256(source);
        let index = self.position(key, &digest)?;
        let entry = self.entries.remove(index);
        self.entries.push(entry);
        self.entries.last().map(|entry| &entry.bitmap)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn remove(&mut self, key: FaviconKey) {
        self.entries.retain(|entry| entry.key != key);
    }

    fn position(&self, key: FaviconKey, digest: &[u8; 32]) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.key == key && entry.digest == *digest)
    }
}

const SHA256_ROUND_CONSTANTS: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const SHA256_INITIAL_STATE: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// FIPS 180-4 SHA-256. Implemented here so favicon keys and digests need no crate.
fn sha256(message: &[u8]) -> [u8; 32] {
    let mut state = SHA256_INITIAL_STATE;
    let mut blocks = message.chunks_exact(64);
    for block in blocks.by_ref() {
        sha256_compress(&mut state, block);
    }

    // Padding: a 0x80 byte, zeros, then the bit length. It needs one extra block when the
    // remainder leaves fewer than 9 bytes free.
    let remainder = blocks.remainder();
    let padded_len = if remainder.len() < 56 { 64 } else { 128 };
    let mut tail = [0_u8; 128];
    tail[..remainder.len()].copy_from_slice(remainder);
    tail[remainder.len()] = 0x80;
    let bit_len = (message.len() as u64).wrapping_mul(8);
    tail[padded_len - 8..padded_len].copy_from_slice(&bit_len.to_be_bytes());
    for block in tail[..padded_len].chunks_exact(64) {
        sha256_compress(&mut state, block);
    }

    let mut digest = [0_u8; 32];
    for (out, word) in digest.chunks_exact_mut(4).zip(state) {
        out.copy_from_slice(&word.to_be_bytes());
    }
    digest
}

fn sha256_compress(state: &mut [u32; 8], block: &[u8]) {
    let mut schedule = [0_u32; 64];
    for (word, bytes) in schedule.iter_mut().zip(block.chunks_exact(4)) {
        *word = u32::from_be_bytes(bytes.try_into().expect("chunks are 4 bytes"));
    }
    for index in 16..64 {
        let w15 = schedule[index - 15];
        let w2 = schedule[index - 2];
        let sigma0 = w15.rotate_right(7) ^ w15.rotate_right(18) ^ (w15 >> 3);
        let sigma1 = w2.rotate_right(17) ^ w2.rotate_right(19) ^ (w2 >> 10);
        schedule[index] = schedule[index - 16]
            .wrapping_add(sigma0)
            .wrapping_add(schedule[index - 7])
            .wrapping_add(sigma1);
    }

    let mut v = *state;
    for (&round_constant, &word) in SHA256_ROUND_CONSTANTS.iter().zip(schedule.iter()) {
        let [a, b, c, d, e, f, g, h] = v;
        let big_sigma1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let choose = (e & f) ^ (!e & g);
        let temp1 = h
            .wrapping_add(big_sigma1)
            .wrapping_add(choose)
            .wrapping_add(round_constant)
            .wrapping_add(word);
        let big_sigma0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let majority = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = big_sigma0.wrapping_add(majority);
        v = [
            temp1.wrapping_add(temp2),
            a,
            b,
            c,
            d.wrapping_add(temp1),
            e,
            f,
            g,
        ];
    }
    for (value, add) in state.iter_mut().zip(v) {
        *value = value.wrapping_add(add);
    }
}

/// Decodes supported raster image bytes into a centered, aspect-preserving square.
/// SVG is not supported. Invalid data and any WIC/COM failure return `None`.
#[cfg(windows)]
pub fn decode(bytes: &[u8], target_px: u32) -> Option<FaviconBitmap> {
    wic::decode(bytes, target_px)
}

/// WIC is only available on Windows.
#[cfg(not(windows))]
pub fn decode(_bytes: &[u8], _target_px: u32) -> Option<FaviconBitmap> {
    None
}

#[cfg(windows)]
mod wic {
    use std::cell::RefCell;

    use windows::Win32::{
        Graphics::Imaging::{
            CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
            WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom,
            WICDecodeMetadataCacheOnLoad,
        },
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
    };

    use super::FaviconBitmap;

    thread_local! {
        static FACTORY: RefCell<Option<IWICImagingFactory>> = const { RefCell::new(None) };
    }

    pub(super) fn factory() -> Option<IWICImagingFactory> {
        FACTORY.with(|cell| {
            if let Some(factory) = cell.borrow().as_ref() {
                return Some(factory.clone());
            }
            // COM initialization belongs to the caller. CoCreateInstance fails cleanly if this
            // thread has no usable apartment (including RPC_E_CHANGED_MODE scenarios).
            let created = unsafe {
                CoCreateInstance::<_, IWICImagingFactory>(
                    &CLSID_WICImagingFactory,
                    None,
                    CLSCTX_INPROC_SERVER,
                )
                .ok()
            }?;
            *cell.borrow_mut() = Some(created.clone());
            Some(created)
        })
    }

    pub(super) fn decode(bytes: &[u8], target_px: u32) -> Option<FaviconBitmap> {
        if bytes.is_empty() || bytes.len() > 256 * 1024 || target_px == 0 || target_px > 512 {
            return None;
        }
        let factory = factory()?;
        // The stream retains this borrowed byte buffer only for the duration of the decode.
        let stream = unsafe { factory.CreateStream().ok()? };
        unsafe { stream.InitializeFromMemory(bytes).ok()? };
        let decoder = unsafe {
            factory
                .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnLoad)
                .ok()?
        };
        let frame_count = unsafe { decoder.GetFrameCount().ok()? };
        if frame_count == 0 || frame_count > 32 {
            return None;
        }

        let mut best: Option<(
            windows::Win32::Graphics::Imaging::IWICBitmapFrameDecode,
            u32,
            u32,
        )> = None;
        for index in 0..frame_count {
            let frame = unsafe { decoder.GetFrame(index).ok()? };
            let (mut width, mut height) = (0, 0);
            unsafe { frame.GetSize(&mut width, &mut height).ok()? };
            if width == 0 || height == 0 || width > 512 || height > 512 {
                return None;
            }
            let size = width.max(height);
            let score = (size < target_px, size.abs_diff(target_px));
            let replace = best.as_ref().is_none_or(|(_, old_width, old_height)| {
                let old_size = (*old_width).max(*old_height);
                score < (old_size < target_px, old_size.abs_diff(target_px))
            });
            if replace {
                best = Some((frame, width, height));
            }
        }
        let (frame, width, height) = best?;
        let ratio = (target_px as f64 / width as f64).min(target_px as f64 / height as f64);
        let out_width = ((width as f64 * ratio).round() as u32).clamp(1, target_px);
        let out_height = ((height as f64 * ratio).round() as u32).clamp(1, target_px);
        // Premultiply before scaling. Filtering straight-alpha pixels would blend transparent
        // black into the colour at every edge and darken it.
        let converter = unsafe { factory.CreateFormatConverter().ok()? };
        unsafe {
            converter
                .Initialize(
                    &frame,
                    &GUID_WICPixelFormat32bppPBGRA,
                    WICBitmapDitherTypeNone,
                    None,
                    0.0,
                    WICBitmapPaletteTypeCustom,
                )
                .ok()?;
        }
        let scaler = unsafe { factory.CreateBitmapScaler().ok()? };
        unsafe {
            scaler
                .Initialize(
                    &converter,
                    out_width,
                    out_height,
                    WICBitmapInterpolationModeFant,
                )
                .ok()?;
        }

        let stride = target_px.checked_mul(4)?;
        let len = (stride as usize).checked_mul(target_px as usize)?;
        let mut pixels = vec![0; len];
        let x = (target_px - out_width) / 2;
        let y = (target_px - out_height) / 2;
        // Each destination slice starts at the centered x/y offset. WIC writes only the
        // scaled rows, using the full square stride, into the pre-zeroed output allocation.
        let row_start = (y as usize).checked_mul(stride as usize)?;
        let offset = (x as usize).checked_mul(4)?;
        let start = row_start.checked_add(offset)?;
        let dst = pixels.get_mut(start..)?;
        unsafe { scaler.CopyPixels(std::ptr::null(), stride, dst).ok()? };

        Some(FaviconBitmap {
            width: target_px,
            height: target_px,
            bgra_premultiplied: pixels,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bitmap(value: u8) -> FaviconBitmap {
        FaviconBitmap {
            width: 1,
            height: 1,
            bgra_premultiplied: vec![value; 4],
        }
    }

    fn first_byte(cache: &mut FaviconCache, key: FaviconKey, source: &[u8]) -> Option<u8> {
        cache
            .get(key, source)
            .map(|bitmap| bitmap.bgra_premultiplied[0])
    }

    /// WIC needs a COM apartment on the calling thread, and test threads start without one. Without
    /// this, WIC creation fails quietly and every decode test passes without running.
    #[cfg(windows)]
    fn wic_for_test() {
        use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        assert!(
            wic::factory().is_some(),
            "WIC factory must be available on Windows"
        );
    }

    #[test]
    fn sha256_matches_published_vectors() {
        let hex = |digest: [u8; 32]| {
            digest
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        assert_eq!(
            hex(sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // 56 bytes: the padding spills into a second block.
        assert_eq!(
            hex(sha256(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(
            hex(sha256(&vec![b'a'; 1_000_000])),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn key_for_is_the_leading_digest_bits_and_content_based() {
        assert_eq!(key_for(b"").0, 0xe3b0c44298fc1c14);
        assert_eq!(key_for(b"favicon"), key_for(b"favicon"));
        assert_ne!(key_for(b"favicon"), key_for(b"other"));
    }

    #[test]
    fn cache_is_bounded_and_evicts_least_recently_used() {
        let mut cache = FaviconCache::default();
        for n in 0..65_u8 {
            cache.insert(FaviconKey(u64::from(n)), &[n], bitmap(n));
        }
        assert!(cache.get(FaviconKey(0), &[0]).is_none());
        assert_eq!(first_byte(&mut cache, FaviconKey(1), &[1]), Some(1));
        cache.get(FaviconKey(2), &[2]);
        cache.insert(FaviconKey(65), &[65], bitmap(65));
        assert!(cache.get(FaviconKey(3), &[3]).is_none());
        assert!(cache.get(FaviconKey(1), &[1]).is_some());
        assert!(cache.get(FaviconKey(2), &[2]).is_some());
    }

    #[test]
    fn cache_replacing_removing_and_clearing() {
        let mut cache = FaviconCache::default();
        cache.insert(FaviconKey(1), b"one", bitmap(1));
        cache.insert(FaviconKey(1), b"one", bitmap(2));
        assert_eq!(first_byte(&mut cache, FaviconKey(1), b"one"), Some(2));
        cache.remove(FaviconKey(1));
        assert!(cache.get(FaviconKey(1), b"one").is_none());
        cache.insert(FaviconKey(2), b"two", bitmap(2));
        cache.clear();
        assert!(cache.get(FaviconKey(2), b"two").is_none());
    }

    #[test]
    fn colliding_keys_never_return_another_favicons_bitmap() {
        // Simulates a forged 64-bit key collision by giving two different favicons the same key.
        // The key is an explicit argument, so no test-only hook is needed.
        let shared = FaviconKey(7);
        let mut cache = FaviconCache::default();
        cache.insert(shared, b"genuine icon", bitmap(1));

        assert!(cache.get(shared, b"forged icon").is_none());
        assert_eq!(first_byte(&mut cache, shared, b"genuine icon"), Some(1));

        cache.insert(shared, b"forged icon", bitmap(2));
        assert_eq!(first_byte(&mut cache, shared, b"forged icon"), Some(2));
        assert_eq!(first_byte(&mut cache, shared, b"genuine icon"), Some(1));

        cache.remove(shared);
        assert!(cache.get(shared, b"genuine icon").is_none());
        assert!(cache.get(shared, b"forged icon").is_none());
    }

    #[cfg(windows)]
    #[test]
    fn decodes_embedded_png_to_premultiplied_bgra() {
        // 1x1 RGBA PNG containing a half-alpha white pixel (255, 255, 255, 128).
        const PNG: &[u8] = &[
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 1, 115, 82, 71, 66, 0, 174, 206, 28, 233, 0,
            0, 0, 4, 103, 65, 77, 65, 0, 0, 177, 143, 11, 252, 97, 5, 0, 0, 0, 9, 112, 72, 89, 115,
            0, 0, 14, 195, 0, 0, 14, 195, 1, 199, 111, 168, 100, 0, 0, 0, 13, 73, 68, 65, 84, 24,
            87, 99, 248, 255, 255, 127, 3, 0, 9, 124, 3, 126, 97, 126, 189, 29, 0, 0, 0, 0, 73, 69,
            78, 68, 174, 66, 96, 130,
        ];
        wic_for_test();
        let decoded = decode(PNG, 16).expect("valid PNG");
        assert_eq!((decoded.width, decoded.height), (16, 16));
        assert_eq!(decoded.bgra_premultiplied.len(), 16 * 16 * 4);
        let pixel = &decoded.bgra_premultiplied[(8 * 16 + 8) * 4..][..4];
        assert_eq!(pixel, &[128, 128, 128, 128]);
    }

    #[cfg(windows)]
    #[test]
    fn scaling_keeps_half_transparent_edges_white() {
        // 2x1 PNG: transparent black, then opaque white. Scaling straight alpha would turn the
        // half-alpha seam grey. Premultiplied white must keep every channel equal to its alpha.
        const PNG: &[u8] = &[
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 1,
            8, 6, 0, 0, 0, 244, 34, 127, 138, 0, 0, 0, 1, 115, 82, 71, 66, 0, 174, 206, 28, 233, 0,
            0, 0, 4, 103, 65, 77, 65, 0, 0, 177, 143, 11, 252, 97, 5, 0, 0, 0, 9, 112, 72, 89, 115,
            0, 0, 14, 195, 0, 0, 14, 195, 1, 199, 111, 168, 100, 0, 0, 0, 15, 73, 68, 65, 84, 24,
            87, 99, 0, 129, 255, 255, 255, 255, 7, 0, 9, 255, 3, 253, 216, 84, 90, 198, 0, 0, 0, 0,
            73, 69, 78, 68, 174, 66, 96, 130,
        ];
        wic_for_test();
        let decoded = decode(PNG, 16).expect("valid PNG");
        let mut partial = 0;
        for pixel in decoded.bgra_premultiplied.chunks_exact(4) {
            let alpha = pixel[3];
            if alpha == 0 || alpha == 255 {
                continue;
            }
            partial += 1;
            for channel in &pixel[..3] {
                assert!(
                    channel.abs_diff(alpha) <= 1,
                    "edge pixel {pixel:?} is not premultiplied white"
                );
            }
        }
        assert!(partial > 0, "scaling produced no half-transparent pixels");
    }

    #[cfg(windows)]
    #[test]
    fn rejects_bad_and_oversized_input() {
        wic_for_test();
        assert!(decode(b"garbage", 16).is_none());
        assert!(decode(&[137, 80, 78], 16).is_none());
        assert!(decode(&vec![0; 256 * 1024 + 1], 16).is_none());
    }
}
