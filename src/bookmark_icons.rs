//! Site icons for bookmarks, so the bookmarks bar and the Bookmarks panel show each site's own
//! picture instead of a letter tile.
//!
//! - [`icon_png`]: the icon to send for a host, from the profile's favicon cache, as a small PNG.
//!   The cache holds whatever the tab reported (often a large PNG or an ICO); anything that is not
//!   already a small PNG is decoded and re-encoded at 32 px, so it always fits the page's limits.
//! - [`fetch_missing`]: a bookmarked site that has never been opened (imported bookmarks) has no
//!   cached icon. Its `/favicon.ico` (then `/favicon.png`, `/apple-touch-icon.png`) is fetched in the
//!   background with the feed reader's hardened GET (`crate::feeds::fetch`: public hosts only, no
//!   cookies, no credentials, size and time limits), stored in the favicon cache, and the window is
//!   told to redraw the bar and the panel. Each host is asked at most once per run. Nothing is
//!   fetched in a guest profile or while the profile keeps no history (no new record of sites).

use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{Condvar, Mutex, OnceLock},
};

/// The window message a finished batch of fetches posts to the browser window.
pub const WM_BOOKMARK_ICONS: u32 = 0x8000 /* WM_APP */ + 0x5E4;
/// Side of the re-encoded icon: crisp at 200% for a 16 px glyph.
const ICON_PX: u32 = 32;
/// A cached PNG up to this size is sent as it is (the bar accepts 8 KiB of data URI).
const SMALL_PNG_BYTES: usize = 6 * 1024;
/// Hosts fetched at the same time.
const WORKERS: usize = 3;
/// Most hosts asked for in one run of Bowser.
const MAX_FETCHES_PER_RUN: usize = 600;

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

// ---------------------------------------------------------------------------------------------
// Re-encoding

/// The icon for `host` from the favicon cache as a PNG of at most [`SMALL_PNG_BYTES`] (decoded and
/// re-encoded at 32 px when it is not one already), or `None` when there is none.
pub fn icon_png(host: &str) -> Option<Vec<u8>> {
    let bytes = crate::favicon::load_favicon_bytes(host).ok().flatten()?;
    type ReencodedIcon = (usize, Option<Vec<u8>>);
    thread_local! {
        /// Re-encodings by host and source length, so a snapshot does not decode the same icon again.
        static MEMO: std::cell::RefCell<HashMap<String, ReencodedIcon>> = std::cell::RefCell::new(HashMap::new());
    }
    if let Some(hit) = MEMO.with(|memo| memo.borrow().get(host).filter(|(len, _)| *len == bytes.len()).map(|(_, png)| png.clone())) {
        return hit;
    }
    let png = small_png(&bytes);
    MEMO.with(|memo| {
        let mut memo = memo.borrow_mut();
        if memo.len() > 2048 {
            memo.clear();
        }
        memo.insert(host.to_owned(), (bytes.len(), png.clone()));
    });
    png
}

/// `bytes` (PNG, ICO or any image WIC reads) as a small PNG: a PNG that is small already as it is,
/// anything else decoded at [`ICON_PX`] and encoded again.
pub fn small_png(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.starts_with(&PNG_SIGNATURE) && bytes.len() <= SMALL_PNG_BYTES {
        return Some(bytes.to_vec());
    }
    let bitmap = crate::favicon::decode(bytes, ICON_PX)?;
    let rgba = straight_rgba(&bitmap.bgra_premultiplied);
    let png = encode_png(bitmap.width, bitmap.height, &rgba)?;
    (png.len() <= SMALL_PNG_BYTES).then_some(png)
}

/// Premultiplied BGRA (what WIC decodes to) as straight RGBA (what PNG stores).
pub fn straight_rgba(bgra: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bgra.len());
    for px in bgra.chunks_exact(4) {
        let (b, g, r, a) = (px[0], px[1], px[2], px[3]);
        let un = |c: u8| if a == 0 { 0 } else { ((u32::from(c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8 };
        out.extend_from_slice(&[un(r), un(g), un(b), a]);
    }
    out
}

/// A minimal PNG (8-bit RGBA, no filtering, zlib "stored" blocks): small icons need no compression
/// and this keeps the encoder a few lines with nothing to get wrong. `None` for a bad size.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    if width == 0 || height == 0 || width > 256 || height > 256 || rgba.len() != (width * height * 4) as usize {
        return None;
    }
    let row = (width * 4) as usize;
    let mut raw = Vec::with_capacity((row + 1) * height as usize);
    for line in rgba.chunks_exact(row) {
        raw.push(0); // filter: none
        raw.extend_from_slice(line);
    }
    let mut zlib = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65_535).peekable();
    while let Some(block) = chunks.next() {
        zlib.push(u8::from(chunks.peek().is_none()));
        let len = block.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut out = PNG_SIGNATURE.to_vec();
    for (kind, data) in [(b"IHDR", &ihdr[..]), (b"IDAT", &zlib[..]), (b"IEND", &[][..])] {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    Some(out)
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { 0xedb8_8320 ^ (crc >> 1) } else { crc >> 1 };
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in bytes {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

// ---------------------------------------------------------------------------------------------
// Fetching what is missing

struct Job {
    dir: PathBuf,
    host: String,
    window: isize,
}

struct Queue {
    jobs: Mutex<VecDeque<Job>>,
    ready: Condvar,
}

fn queue() -> &'static Queue {
    static QUEUE: OnceLock<Queue> = OnceLock::new();
    QUEUE.get_or_init(|| {
        for n in 0..WORKERS {
            let _ = std::thread::Builder::new().name(format!("bowser-bookmark-icons-{n}")).spawn(worker);
        }
        Queue { jobs: Mutex::new(VecDeque::new()), ready: Condvar::new() }
    })
}

/// Hosts already asked for (or queued) in this run.
fn asked() -> std::sync::MutexGuard<'static, HashSet<String>> {
    static ASKED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    ASKED.get_or_init(|| Mutex::new(HashSet::new())).lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Queues a fetch of the icon of every host in `hosts` that has none cached yet and was not asked
/// for in this run; `window` (the browser window's handle) is told when icons arrive. Cheap to
/// call on every snapshot: it returns at once.
pub fn fetch_missing<'a>(window: isize, hosts: impl IntoIterator<Item = &'a str>) {
    let Ok(profile) = crate::profile::active() else { return };
    if profile.is_guest() || !profile.history_enabled() {
        return;
    }
    let dir = profile.favicons_dir();
    let mut asked = asked();
    let mut fresh = Vec::new();
    for host in hosts {
        let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
        if host.is_empty() || asked.len() >= MAX_FETCHES_PER_RUN || asked.contains(&host) {
            continue;
        }
        if crate::favicon::load_favicon_bytes(&host).ok().flatten().is_some() {
            continue;
        }
        asked.insert(host.clone());
        fresh.push(Job { dir: dir.clone(), host, window });
    }
    drop(asked);
    if fresh.is_empty() {
        return;
    }
    let queue = queue();
    queue.jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).extend(fresh);
    queue.ready.notify_all();
}

fn worker() {
    #[cfg(windows)]
    {
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
        // WIC (to check that the answer is an image and re-encode it) needs COM on this thread.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    }
    let queue = queue();
    let mut since_notice = 0usize;
    loop {
        let job = {
            let mut jobs = queue.jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            loop {
                if let Some(job) = jobs.pop_front() {
                    break job;
                }
                jobs = queue.ready.wait(jobs).unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        };
        if let Some(png) = fetch_icon(&job.host) {
            if crate::favicon::store_favicon_at(&job.dir, &job.host, &png).is_ok() {
                since_notice += 1;
            }
        }
        // One redraw per handful of icons, and one when the queue runs dry.
        let idle = queue.jobs.lock().map(|jobs| jobs.is_empty()).unwrap_or(true);
        if since_notice > 0 && (idle || since_notice >= 8) {
            since_notice = 0;
            notify(job.window);
        }
    }
}

/// The site's own icon as a small PNG: its conventional icon addresses, first that is an image.
fn fetch_icon(host: &str) -> Option<Vec<u8>> {
    for candidate in crate::favicon::favicon_url_candidates(&format!("https://{host}/")) {
        let Ok(url) = url::Url::parse(&candidate) else { continue };
        let Ok(fetched) = crate::feeds::fetch::fetch(&url) else { continue };
        if fetched.bytes.is_empty() || fetched.bytes.len() > 256 * 1024 {
            continue;
        }
        if let Some(png) = small_png(&fetched.bytes) {
            return Some(png);
        }
    }
    None
}

fn notify(window: isize) {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::{Foundation::{HWND, LPARAM, WPARAM}, UI::WindowsAndMessaging::PostMessageW};
        let _ = PostMessageW(Some(HWND(window as *mut _)), WM_BOOKMARK_ICONS, WPARAM(0), LPARAM(0));
    }
    #[cfg(not(windows))]
    let _ = window;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The encoder's output is a PNG any reader takes: signature, IHDR, one IDAT whose stored
    /// blocks hold the rows, valid CRCs and Adler-32.
    #[test]
    fn encoded_icons_are_valid_small_pngs() {
        let rgba: Vec<u8> = (0..32 * 32).flat_map(|n| [n as u8, 0x40, 0x80, 0xff]).collect();
        let png = encode_png(32, 32, &rgba).unwrap();
        assert!(png.starts_with(&PNG_SIGNATURE));
        assert!(png.len() <= SMALL_PNG_BYTES, "{} bytes", png.len());
        // Every chunk's CRC checks out and the chunks are IHDR, IDAT, IEND.
        let mut at = 8;
        let mut kinds = Vec::new();
        while at < png.len() {
            let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
            let body = &png[at + 4..at + 8 + len];
            let crc = u32::from_be_bytes(png[at + 8 + len..at + 12 + len].try_into().unwrap());
            assert_eq!(crc32(body), crc);
            kinds.push(String::from_utf8(body[..4].to_vec()).unwrap());
            if &body[..4] == b"IDAT" {
                let data = &body[4..];
                let raw_len = u16::from_le_bytes([data[3], data[4]]) as usize;
                let raw = &data[7..7 + raw_len];
                assert_eq!(raw.len(), 32 * (1 + 32 * 4));
                assert_eq!(&raw[1..5], &rgba[..4]);
                assert_eq!(u32::from_be_bytes(data[7 + raw_len..].try_into().unwrap()), adler32(raw));
            }
            at += 12 + len;
        }
        assert_eq!(kinds, ["IHDR", "IDAT", "IEND"]);
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926, "the standard check value");
        assert!(encode_png(0, 1, &[]).is_none());
        assert!(encode_png(2, 2, &[0; 3]).is_none());
    }

    #[test]
    fn premultiplied_pixels_come_back_straight() {
        // Half-transparent white premultiplied is (128, 128, 128, 128).
        assert_eq!(straight_rgba(&[128, 128, 128, 128]), [255, 255, 255, 128]);
        assert_eq!(straight_rgba(&[0, 0, 0, 0]), [0, 0, 0, 0]);
        assert_eq!(straight_rgba(&[10, 20, 30, 255]), [30, 20, 10, 255], "BGRA becomes RGBA");
    }

    #[test]
    fn a_small_png_is_sent_as_it_is_and_a_large_one_is_not_sent_raw() {
        let small = encode_png(16, 16, &[200; 16 * 16 * 4]).unwrap();
        assert_eq!(small_png(&small), Some(small.clone()));
        let mut large = small.clone();
        large.resize(SMALL_PNG_BYTES + 1, 0);
        assert_ne!(small_png(&large).as_deref(), Some(&large[..]), "never more than the bar accepts");
    }
}
