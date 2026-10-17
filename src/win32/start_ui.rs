//! The start page over a blank new tab (docs/design/start-page.md).
//!
//! `split_ui.rs` shows the split view's one HTML host over a blank normal tab (the same host that
//! shows the private landing, so a new tab costs no extra WebView2) and asks this module two things:
//! when a fresh page snapshot is due ([`due`]) and what to do with the page's commands ([`run_queued`]).
//! The data, the commands and the snapshot are `crate::start_page` (`service.rs`, tested without a
//! window); this module adds only what needs one: opening an address, the image picker and re-encode,
//! the Notes panel and the settings page.
//!
//! A new tab must not wait on this page. [`due`] reads nothing when the tab shows an unchanged page:
//! it compares five file stamps (at most every [`STAMP_EVERY`]) and the settings, and rebuilds only
//! when one changed or a command ran. Every function runs on the UI thread.

use std::{
    cell::RefCell,
    collections::HashMap,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree},
        UI::Shell::{FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH},
    },
    core::PCWSTR,
};

use crate::{
    app::{App, AppRef},
    chrome_host::{
        host::ChromeHost,
        protocol::PanelFields,
        start_page::{self as wire, Ack},
    },
    internal_page::Section,
    profile,
    start_page::{
        StartSettings,
        service::{self, Effect, Page, Paths, Stamp},
    },
    tabs::TabId,
};

/// How often an unchanged page looks at its files again (a new tab always checks once).
const STAMP_EVERY: Duration = Duration::from_millis(750);
/// The largest picture read for a speed dial image; the re-encode keeps what is shown small.
const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;
/// How old the process must be before the first start page creates its WebView2 host, so a cold launch
/// finishes the toolbar and tab strip first (PRODUCT.md: nothing may slow cold launch). Until then a
/// blank tab shows the plain new-tab surface it always had.
const LAUNCH_DELAY: Duration = Duration::from_millis(1500);
/// A wake-up is pending / has fired (set from the timer thread, read on the UI thread).
static WAKING: AtomicBool = AtomicBool::new(false);
static WAKE: AtomicBool = AtomicBool::new(false);

/// Whether a blank normal tab shows the start page. The "new tab page" choice of Settings > Tabs
/// (start page, homepage, blank or an address) gates it: only "Blank page" turns it off.
pub(crate) fn enabled(app: &AppRef) -> bool {
    crate::tab_options::start_page_shown(&App::tab_options(app))
}

/// How long this process has been running.
fn process_age() -> Duration {
    use windows::Win32::{
        Foundation::FILETIME,
        System::{SystemInformation::GetSystemTimeAsFileTime, Threading::{GetCurrentProcess, GetProcessTimes}},
    };
    let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    let (mut created, mut exited, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: plain out-parameters on this process's own handle.
    let ok = unsafe { GetProcessTimes(GetCurrentProcess(), &mut created, &mut exited, &mut kernel, &mut user) }.is_ok();
    if !ok {
        return Duration::MAX;
    }
    let now = ticks(unsafe { GetSystemTimeAsFileTime() });
    Duration::from_nanos(now.saturating_sub(ticks(created)).saturating_mul(100))
}

/// Whether the window may show its first start page yet: the process is old enough (see
/// [`LAUNCH_DELAY`]). Before that it wakes the window once, from a short-lived thread, when it is.
pub(crate) fn ready(owner: HWND) -> bool {
    let age = process_age();
    if age >= LAUNCH_DELAY {
        return true;
    }
    if !WAKING.swap(true, Ordering::AcqRel) {
        let raw = owner.0 as isize;
        let wait = LAUNCH_DELAY - age;
        std::thread::spawn(move || {
            std::thread::sleep(wait);
            WAKE.store(true, Ordering::Release);
            crate::win32::post(HWND(raw as *mut _), super::WM_SPLIT_COMMAND, WPARAM(0), LPARAM(0));
        });
    }
    false
}

#[derive(Default)]
struct Built {
    stamp: Stamp,
    settings: Option<StartSettings>,
    requested: Option<u64>,
    guest: bool,
    page: Page,
    checked: Option<Instant>,
}

#[derive(Default)]
struct Runtime {
    /// The folder the page shows (`None`: the group its settings name). A new tab starts over.
    requested: Option<u64>,
    last_tab: Option<TabId>,
    /// A command ran or the settings changed: the next [`due`] rebuilds.
    dirty: bool,
    /// The answer to the last refused command, sent once with the snapshot that follows it.
    ack: Option<String>,
    built: Built,
    /// Chosen thumbnails to post after the page.
    pending_thumbs: Vec<String>,
    /// The host has the snapshot [`due`] last returned (it accepted it, or already showed it). While it
    /// has not, [`due`] returns the page again, so a post that failed is retried on the next sync.
    delivered: bool,
}

thread_local! {
    static RUNTIMES: RefCell<HashMap<isize, Runtime>> = RefCell::new(HashMap::new());
    static QUEUE: RefCell<Vec<(isize, PanelFields)>> = const { RefCell::new(Vec::new()) };
}

fn with<T>(owner: HWND, f: impl FnOnce(&mut Runtime) -> T) -> Option<T> {
    RUNTIMES.with(|runtimes| {
        let mut runtimes = runtimes.try_borrow_mut().ok()?;
        Some(f(runtimes.entry(owner.0 as isize).or_default()))
    })
}

/// The profile's files, and whether this window is a guest's (which keeps nothing).
fn paths() -> (Option<Paths>, bool) {
    match profile::active() {
        Ok(profile) => {
            let guest = profile.is_guest();
            (Some(Paths { dir: profile.root().to_owned(), history: profile.history_file(), privacy: profile.privacy_stats_file() }), guest)
        }
        Err(_) => (None, true),
    }
}

/// Marks every start page of `owner` stale (settings were saved, or the speed dials changed).
pub(crate) fn settings_changed(owner: HWND) {
    let _ = with(owner, |rt| rt.dirty = true);
    super::window::split_ui_invalidate(owner);
}

/// The snapshot to post now, or `None` when what the page shows has not changed. `appeared`: the page
/// was not showing this tab a moment ago (a new tab, or another tab's page before it).
pub(crate) fn due(owner: HWND, app: &AppRef, tab: TabId, appeared: bool) -> Option<String> {
    let settings = App::start_settings(app);
    let now = Instant::now();
    // The common case (a chrome sync while the page is up and nothing changed) costs a few comparisons.
    let unchanged = with(owner, |rt| {
        !appeared
            && rt.delivered
            && !rt.dirty
            && rt.last_tab == Some(tab)
            && rt.built.settings.as_ref() == Some(&settings)
            && rt.built.requested == rt.requested
            && rt.built.checked.is_some_and(|at| now.duration_since(at) < STAMP_EVERY)
    });
    if unchanged == Some(true) {
        return None;
    }
    let (paths, guest) = paths();
    let (needs, requested) = with(owner, |rt| {
        if rt.last_tab != Some(tab) {
            // Another tab's page: it opens on the group the settings name, with no old answer.
            rt.last_tab = Some(tab);
            rt.requested = None;
            rt.ack = None;
            rt.dirty = true;
        }
        let mut needs = rt.dirty || appeared || !rt.delivered || rt.built.settings.as_ref() != Some(&settings) || rt.built.guest != guest || rt.built.requested != rt.requested;
        if !needs && rt.built.checked.is_none_or(|at| now.duration_since(at) >= STAMP_EVERY) {
            let fresh = paths.as_ref().map(service::stamp).unwrap_or_default();
            rt.built.checked = Some(now);
            needs = fresh != rt.built.stamp;
        }
        (needs, rt.requested)
    })?;
    if !needs {
        return None;
    }
    let stamp = paths.as_ref().map(service::stamp).unwrap_or_default();
    let (rebuild, ack) = with(owner, |rt| {
        let ack = rt.ack.take();
        let rebuild = rt.dirty
            || ack.is_some()
            || rt.built.page.json.is_empty()
            || rt.built.settings.as_ref() != Some(&settings)
            || rt.built.guest != guest
            || rt.built.requested != requested
            || rt.built.stamp != stamp;
        (rebuild, ack)
    })?;
    if rebuild {
        let answer = ack.map_or(Ack::None, Ack::Failed);
        let page = service::page(paths.as_ref(), guest, &settings, requested, answer);
        let _ = with(owner, |rt| rt.built = Built { stamp, settings: Some(settings), requested, guest, page, checked: Some(now) });
    }
    with(owner, |rt| {
        rt.dirty = false;
        rt.delivered = false;
        rt.pending_thumbs = rt.built.page.thumbs.clone();
        rt.built.page.json.clone()
    })
}

/// The page accepted its snapshot: the chosen images follow, each in a message under the page's limit.
pub(crate) fn posted(owner: HWND, host: &ChromeHost) {
    let thumbs = with(owner, |rt| {
        rt.delivered = true;
        std::mem::take(&mut rt.pending_thumbs)
    })
    .unwrap_or_default();
    for message in thumbs {
        if host.post_json(&message).is_err() {
            break;
        }
    }
}

/// The page already shows the snapshot [`due`] returned (the host skipped an identical post).
pub(crate) fn settled(owner: HWND) {
    let _ = with(owner, |rt| {
        rt.delivered = true;
        rt.pending_thumbs.clear();
    });
}

/// A command from the page: queued, and run from `WM_SPLIT_COMMAND` (never inside WebView2's callback).
pub(crate) fn queue(owner: HWND, fields: PanelFields) {
    let full = QUEUE.with(|queue| {
        let mut queue = queue.borrow_mut();
        if queue.len() >= 32 {
            return true;
        }
        queue.push((owner.0 as isize, fields));
        false
    });
    if !full {
        crate::win32::post(owner, super::WM_SPLIT_COMMAND, WPARAM(0), LPARAM(0));
    }
}

/// Runs the queued page commands for `owner`.
pub(crate) fn run_queued(owner: HWND, app: &AppRef) {
    // The launch delay is over: show the page the window has been waiting to show.
    if WAKE.swap(false, Ordering::AcqRel) {
        super::window::split_ui_invalidate(owner);
    }
    let commands: Vec<PanelFields> = QUEUE.with(|queue| {
        let mut queue = queue.borrow_mut();
        let (mine, rest): (Vec<_>, Vec<_>) = queue.drain(..).partition(|(id, _)| *id == owner.0 as isize);
        *queue = rest;
        mine.into_iter().map(|(_, fields)| fields).collect()
    });
    for fields in commands {
        // Only while the page is showing: a command that arrives after the tab navigated is stale.
        let Some(action) = wire::map_command(&fields) else { continue };
        run(owner, app, action);
    }
}

fn fail(owner: HWND, message: &str) {
    let _ = with(owner, |rt| {
        rt.ack = Some(message.to_owned());
        rt.dirty = true;
    });
}

fn run(owner: HWND, app: &AppRef, action: wire::Action) {
    use wire::Action;
    let (paths, guest) = paths();
    let Some(paths) = paths else { return };
    // A guest window keeps nothing: it can open what is shown (nothing is), and reach Settings.
    let readonly = matches!(action, Action::Open { .. } | Action::OpenUrl { .. } | Action::Settings | Action::Refresh);
    if guest && !readonly {
        fail(owner, "Speed dials are not kept in a guest window.");
        super::window::split_ui_invalidate(owner);
        return;
    }
    let mut requested = with(owner, |rt| rt.requested).flatten();
    let result = service::apply(&paths, &mut requested, action);
    match result {
        Ok(effect) => {
            let _ = with(owner, |rt| {
                rt.requested = requested;
                rt.dirty = true;
            });
            carry_out(owner, app, &paths, effect);
        }
        Err(message) => fail(owner, &message),
    }
    super::window::split_ui_invalidate(owner);
}

fn carry_out(owner: HWND, app: &AppRef, paths: &Paths, effect: Effect) {
    match effect {
        Effect::None => {}
        Effect::Open { url, new_tab } => {
            if new_tab {
                App::new_tab(app);
            }
            App::navigate_current(app, &url);
        }
        Effect::PickThumb(id) => {
            let Some(path) = pick_image(owner) else { return };
            let outcome = std::fs::metadata(&path)
                .ok()
                .filter(|meta| meta.len() <= MAX_IMAGE_BYTES)
                .and_then(|_| std::fs::read(&path).ok())
                .and_then(|bytes| reencode_jpeg(&bytes))
                .ok_or_else(|| "That image could not be used. Choose a PNG, JPEG, GIF or BMP picture.".to_owned())
                .and_then(|jpeg| service::set_thumbnail(paths, id, &jpeg));
            if let Err(message) = outcome {
                fail(owner, &message);
            }
        }
        Effect::SaveNote { title, body } => {
            let Ok(profile) = profile::active() else { return };
            if let Err(message) = service::save_note(&profile.notes_file(), &title, &body) {
                fail(owner, &message);
            }
        }
        Effect::Settings => super::settings_page::open(owner, app, Some(Section::StartPage), false),
    }
}

// ----- the image a speed dial shows -----

/// The system open dialog for a picture. `None` when cancelled or when it cannot be created.
fn pick_image(owner: HWND) -> Option<PathBuf> {
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let title: Vec<u16> = "Choose a picture for this speed dial".encode_utf16().chain(Some(0)).collect();
        let name: Vec<u16> = "Pictures".encode_utf16().chain(Some(0)).collect();
        let spec: Vec<u16> = "*.png;*.jpg;*.jpeg;*.gif;*.bmp;*.tif;*.tiff".encode_utf16().chain(Some(0)).collect();
        let types = [COMDLG_FILTERSPEC { pszName: PCWSTR(name.as_ptr()), pszSpec: PCWSTR(spec.as_ptr()) }];
        dialog.SetFileTypes(&types).ok()?;
        dialog.SetOptions(FOS_FILEMUSTEXIST | FOS_FORCEFILESYSTEM).ok()?;
        dialog.SetTitle(PCWSTR(title.as_ptr())).ok()?;
        dialog.Show(Some(owner)).ok()?;
        let item = dialog.GetResult().ok()?;
        let display = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = display.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(display.0.cast_const().cast()));
        path
    }
}

/// Widest and tallest a chosen picture is kept: a tile is at most 232 CSS px wide at 200% scaling.
const THUMB_MAX_W: u32 = 480;
const THUMB_MAX_H: u32 = 300;

/// Decodes a picture (PNG, JPEG, GIF, BMP, TIFF), fits it inside [`THUMB_MAX_W`] x [`THUMB_MAX_H`]
/// without enlarging it, lays it on white (so transparency is not black) and encodes a JPEG. Nothing
/// is sent anywhere; the result is a small file in the profile.
pub(crate) fn reencode_jpeg(bytes: &[u8]) -> Option<Vec<u8>> {
    use windows::Win32::{
        Graphics::Imaging::{
            CLSID_WICImagingFactory, GUID_ContainerFormatJpeg, GUID_WICPixelFormat24bppBGR, GUID_WICPixelFormat32bppPBGRA,
            IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapEncoderNoCache, WICBitmapInterpolationModeFant,
            WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnLoad,
        },
        System::Com::{IStream, STREAM_SEEK_END, STREAM_SEEK_SET},
        UI::Shell::SHCreateMemStream,
    };
    if bytes.is_empty() || bytes.len() as u64 > MAX_IMAGE_BYTES {
        return None;
    }
    unsafe {
        let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let source = factory.CreateStream().ok()?;
        source.InitializeFromMemory(bytes).ok()?;
        let decoder = factory.CreateDecoderFromStream(&source, std::ptr::null(), WICDecodeMetadataCacheOnLoad).ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let (mut width, mut height) = (0u32, 0u32);
        frame.GetSize(&mut width, &mut height).ok()?;
        if width == 0 || height == 0 || width > 20_000 || height > 20_000 {
            return None;
        }
        let (out_w, out_h) = fit_within(width, height, THUMB_MAX_W, THUMB_MAX_H);
        let scaler = factory.CreateBitmapScaler().ok()?;
        scaler.Initialize(&frame, out_w, out_h, WICBitmapInterpolationModeFant).ok()?;
        // Premultiplied so the white underlay below is a plain add.
        let converter = factory.CreateFormatConverter().ok()?;
        converter.Initialize(&scaler, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom).ok()?;
        let stride = out_w * 4;
        let mut pixels = vec![0u8; (stride * out_h) as usize];
        converter.CopyPixels(std::ptr::null(), stride, &mut pixels).ok()?;
        let bgr = flatten_on_white(&pixels);

        let target: IStream = SHCreateMemStream(None)?;
        let encoder = factory.CreateEncoder(&GUID_ContainerFormatJpeg, std::ptr::null()).ok()?;
        encoder.Initialize(&target, WICBitmapEncoderNoCache).ok()?;
        let mut out = None;
        let mut options = None;
        encoder.CreateNewFrame(&mut out, &mut options).ok()?;
        let out = out?;
        out.Initialize(options.as_ref()).ok()?;
        out.SetSize(out_w, out_h).ok()?;
        let mut format = GUID_WICPixelFormat24bppBGR;
        out.SetPixelFormat(&mut format).ok()?;
        if format != GUID_WICPixelFormat24bppBGR {
            return None;
        }
        out.WritePixels(out_h, out_w * 3, &bgr).ok()?;
        out.Commit().ok()?;
        encoder.Commit().ok()?;
        let mut end = 0u64;
        target.Seek(0, STREAM_SEEK_END, Some(&mut end)).ok()?;
        target.Seek(0, STREAM_SEEK_SET, None).ok()?;
        if end == 0 || end > crate::start_page::MAX_THUMB_BYTES as u64 {
            return None;
        }
        let mut jpeg = vec![0u8; end as usize];
        let mut read = 0u32;
        let status = target.Read(jpeg.as_mut_ptr().cast(), end as u32, Some(&mut read));
        (status.is_ok() && read as u64 == end).then_some(jpeg)
    }
}

/// `(w, h)` scaled down, keeping the aspect ratio, to fit inside `max_w` x `max_h`; never enlarged.
pub(crate) fn fit_within(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    if w <= max_w && h <= max_h {
        return (w, h);
    }
    let scale = (f64::from(max_w) / f64::from(w)).min(f64::from(max_h) / f64::from(h));
    (((f64::from(w) * scale).round() as u32).clamp(1, max_w), ((f64::from(h) * scale).round() as u32).clamp(1, max_h))
}

/// Premultiplied BGRA laid on white, as packed 24-bit BGR rows.
pub(crate) fn flatten_on_white(bgra: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bgra.len() / 4 * 3);
    for px in bgra.chunks_exact(4) {
        let add = 255 - px[3];
        out.extend([px[0].saturating_add(add), px[1].saturating_add(add), px[2].saturating_add(add)]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_fit_inside_the_tile_budget_and_are_never_enlarged() {
        assert_eq!(fit_within(100, 50, THUMB_MAX_W, THUMB_MAX_H), (100, 50));
        assert_eq!(fit_within(960, 600, THUMB_MAX_W, THUMB_MAX_H), (480, 300));
        assert_eq!(fit_within(4000, 1000, THUMB_MAX_W, THUMB_MAX_H), (480, 120));
        assert_eq!(fit_within(1000, 4000, THUMB_MAX_W, THUMB_MAX_H), (75, 300));
        assert_eq!(fit_within(10_000, 1, THUMB_MAX_W, THUMB_MAX_H), (480, 1));
    }

    #[test]
    fn transparent_pixels_lie_on_white() {
        // Premultiplied: fully transparent (0,0,0,0), half red, opaque blue.
        let out = flatten_on_white(&[0, 0, 0, 0, 0, 0, 127, 128, 255, 0, 0, 255]);
        assert_eq!(out, [255, 255, 255, 127, 127, 254, 255, 0, 0]);
    }

    /// A 24-bit BMP of a horizontal gradient, as a user's picture file.
    fn bmp(width: u32, height: u32) -> Vec<u8> {
        let row = (width * 3).div_ceil(4) * 4;
        let size = 54 + row * height;
        let mut bytes = Vec::new();
        bytes.extend(b"BM");
        bytes.extend(size.to_le_bytes());
        bytes.extend([0, 0, 0, 0]);
        bytes.extend(54u32.to_le_bytes());
        bytes.extend(40u32.to_le_bytes());
        bytes.extend(width.to_le_bytes());
        bytes.extend(height.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(24u16.to_le_bytes());
        bytes.extend([0; 24]);
        for y in 0..height {
            for x in 0..width {
                bytes.extend([(x * 255 / width.max(1)) as u8, (y * 255 / height.max(1)) as u8, 128]);
            }
            bytes.extend(std::iter::repeat_n(0u8, (row - width * 3) as usize));
        }
        bytes
    }

    #[test]
    fn a_picture_becomes_a_small_jpeg() {
        use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
        let init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let jpeg = reencode_jpeg(&bmp(960, 600));
        if init.is_ok() {
            unsafe { CoUninitialize() };
        }
        let jpeg = jpeg.expect("WIC converts a BMP");
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8], "a JPEG");
        assert!(jpeg.len() <= crate::start_page::MAX_THUMB_BYTES, "{}", jpeg.len());
        assert!(reencode_jpeg(b"not a picture").is_none());
        assert!(reencode_jpeg(&[]).is_none());
    }
}
