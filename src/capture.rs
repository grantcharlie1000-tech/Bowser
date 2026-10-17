//! Page capture (thread T7): the DevTools requests a capture needs, reading their answers strictly,
//! stitching a long page from slices, saving a file and making a clipboard bitmap.
//!
//! The browser takes the pictures (`Page.captureScreenshot`); nothing here touches the network. A
//! visible-area capture is one call. A selection is one call with a clip around the selection's box.
//! A full page is cut into slices of at most [`crate::page_tools::SLICE_HEIGHT`] pixels (a single
//! screenshot taller than a GPU texture fails), each captured on its own and written one after the
//! other into a single PNG or JPEG, so a 30000 px page never has to sit in memory as one bitmap.
//!
//! Every answer comes from a page's browser process, so it is parsed strictly: a bounded base64
//! string whose decoded bytes start with a PNG or JPEG signature, and numbers that are finite.

use std::path::{Path, PathBuf};

use crate::{
    blocker::json::{base64_decode, field, string_value, whole_object},
    page_tools::{CaptureFormat, MAX_SLICE_BASE64},
};

/// The only DevTools methods a capture may call. The engine refuses any other.
pub const METHOD_METRICS: &str = "Page.getLayoutMetrics";
pub const METHOD_SHOT: &str = "Page.captureScreenshot";

/// JPEG quality for captures.
const JPEG_QUALITY: u8 = 90;
/// Largest picture a clipboard copy makes (the bitmap is held in memory, 3 bytes a pixel).
pub const MAX_CLIPBOARD_BYTES: u64 = 160 * 1024 * 1024;
/// Tallest picture a JPEG can be.
const MAX_JPEG_SIDE: u32 = 65_000;

/// A rectangle of the page in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clip {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// The parameters of `Page.captureScreenshot`: the whole visible area, or `clip` of the page (which
/// may lie outside the viewport).
pub fn shot_params(format: CaptureFormat, clip: Option<Clip>) -> String {
    let mut out = format!("{{\"format\":\"{}\",\"fromSurface\":true", if format == CaptureFormat::Jpeg { "jpeg" } else { "png" });
    if format == CaptureFormat::Jpeg {
        out.push_str(&format!(",\"quality\":{JPEG_QUALITY}"));
    }
    if let Some(clip) = clip {
        out.push_str(&format!(
            ",\"captureBeyondViewport\":true,\"clip\":{{\"x\":{},\"y\":{},\"width\":{},\"height\":{},\"scale\":1}}",
            clip.x, clip.y, clip.width, clip.height
        ));
    }
    out.push('}');
    out
}

/// The page's content size in CSS pixels from a `Page.getLayoutMetrics` answer.
pub fn parse_metrics(answer: &str) -> Option<(f64, f64)> {
    let members = whole_object(answer)?;
    for name in ["cssContentSize", "contentSize"] {
        let Some(raw) = field(&members, name) else { continue };
        let Some(inner) = whole_object(raw) else { continue };
        let number = |key: &str| field(&inner, key).and_then(|text| text.trim().parse::<f64>().ok()).filter(|n| n.is_finite());
        if let (Some(width), Some(height)) = (number("width"), number("height")) {
            return Some((width, height));
        }
    }
    None
}

/// The picture a `Page.captureScreenshot` answer carries: base64 of a PNG or a JPEG, nothing else.
pub fn parse_shot(answer: &str) -> Option<Vec<u8>> {
    let members = whole_object(answer)?;
    let data = string_value(field(&members, "data")?)?;
    if data.is_empty() || data.len() > MAX_SLICE_BASE64 {
        return None;
    }
    let bytes = base64_decode(&data)?;
    kind_of(&bytes).map(|_| bytes)
}

/// Which image a byte string is, by its signature.
pub fn kind_of(bytes: &[u8]) -> Option<CaptureFormat> {
    if bytes.len() > 8 && bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some(CaptureFormat::Png)
    } else if bytes.len() > 4 && bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(CaptureFormat::Jpeg)
    } else {
        None
    }
}

/// The script that reports the page's selection: `{"c":1,"t":text,"x","y","w","h","a":address,"n":title}`
/// with whole-number page coordinates, or `{"c":0}` when nothing is selected. Read-only.
pub const SELECTION_SCRIPT: &str = "(function(){try{var s=getSelection();if(!s||s.rangeCount===0||s.isCollapsed){return JSON.stringify({c:0});}var r=s.getRangeAt(0).getBoundingClientRect();return JSON.stringify({c:1,t:s.toString().slice(0,20000),x:Math.round(r.left+scrollX),y:Math.round(r.top+scrollY),w:Math.round(r.width),h:Math.round(r.height),a:String(location.href).slice(0,2048),n:String(document.title).slice(0,200)});}catch(e){return JSON.stringify({c:0});}})()";

/// Largest answer read back from [`SELECTION_SCRIPT`] (20000 characters of text, escaped).
pub const SELECTION_ANSWER_LIMIT: usize = 130_000;

/// What the page has selected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub text: String,
    /// The selection's box in page coordinates: left, top, width, height.
    pub rect: (i64, i64, i64, i64),
    pub address: String,
    pub title: String,
}

/// An answer that is not [`SELECTION_SCRIPT`]'s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadAnswer;

/// Reads [`SELECTION_SCRIPT`]'s answer. `Ok(None)` is "nothing selected"; `Err(BadAnswer)` is an answer
/// that is not the script's (nothing from the page is trusted beyond text and whole numbers).
pub fn parse_selection(raw: &str) -> Result<Option<Selection>, BadAnswer> {
    if raw.len() > SELECTION_ANSWER_LIMIT {
        return Err(BadAnswer);
    }
    let inner = string_value(raw.trim()).ok_or(BadAnswer)?;
    let value = crate::translate::json::parse(&inner).ok_or(BadAnswer)?;
    match value.get("c").and_then(|c| c.as_int()) {
        Some(0) => return Ok(None),
        Some(1) => {}
        _ => return Err(BadAnswer),
    }
    let int = |key: &str| value.get(key).and_then(|v| v.as_int()).ok_or(BadAnswer);
    let text = |key: &str| value.get(key).and_then(|v| v.as_str()).map(str::to_owned).ok_or(BadAnswer);
    Ok(Some(Selection {
        text: text("t")?,
        rect: (int("x")?, int("y")?, int("w")?, int("h")?),
        address: text("a")?,
        title: text("n")?,
    }))
}

/// Where captures go when no folder is chosen: Pictures\Bowser.
pub fn default_dir() -> Option<PathBuf> {
    use windows::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_Pictures, KF_FLAG_DEFAULT, SHGetKnownFolderPath},
    };
    let known = unsafe {
        SHGetKnownFolderPath(&FOLDERID_Pictures, KF_FLAG_DEFAULT, None).ok().and_then(|raw| {
            let text = raw.to_string().ok();
            CoTaskMemFree(Some(raw.0.cast_const().cast()));
            text
        })
    };
    known
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join("Pictures")))
        .filter(|path| path.is_absolute())
        .map(|pictures| pictures.join("Bowser"))
}

/// The folder captures are saved in under `setting` (`pt_capture_dir`): the typed one, else the
/// default. Never a path [`crate::page_tools::valid_dir`] refuses.
pub fn resolve_dir(setting: &str) -> Option<PathBuf> {
    if setting.is_empty() {
        default_dir()
    } else if crate::page_tools::valid_dir(setting) {
        Some(PathBuf::from(setting))
    } else {
        default_dir()
    }
}

/// Writes `bytes` to a new file in `dir` named from `stem` (made unique), creating the folder. A file
/// that appears between the name check and the write is never overwritten.
pub fn save_file(dir: &Path, stem: &str, format: CaptureFormat, bytes: &[u8]) -> Result<PathBuf, String> {
    use std::io::Write;
    std::fs::create_dir_all(dir).map_err(|error| format!("The capture folder could not be made: {error}"))?;
    let mut taken = std::collections::HashSet::new();
    loop {
        let path = crate::page_tools::unique_path(dir, stem, format, |candidate| candidate.exists() || taken.contains(candidate));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                return file
                    .write_all(bytes)
                    .and_then(|()| file.sync_all())
                    .map(|()| path.clone())
                    .map_err(|error| {
                        let _ = std::fs::remove_file(&path);
                        format!("The capture could not be saved: {error}")
                    });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                taken.insert(path);
            }
            Err(error) => return Err(format!("The capture could not be saved: {error}")),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Pictures: stitching slices and the clipboard bitmap (Windows Imaging Component)
// ---------------------------------------------------------------------------------------------

use windows::{
    Win32::{
        Graphics::Imaging::{
            CLSID_WICImagingFactory, GUID_ContainerFormatJpeg, GUID_ContainerFormatPng, GUID_WICPixelFormat24bppBGR, IWICBitmapFrameEncode,
            IWICBitmapSource, IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapEncoderNoCache, WICBitmapPaletteTypeCustom,
            WICDecodeMetadataCacheOnLoad,
        },
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IStream, STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET, StructuredStorage::IPropertyBag2},
        UI::Shell::SHCreateMemStream,
    },
    core::Interface,
};

/// A decoded picture as 24-bit BGR rows (top down, each row padded to 4 bytes).
struct Decoded {
    width: u32,
    height: u32,
    stride: u32,
    pixels: Vec<u8>,
}

fn factory() -> Option<IWICImagingFactory> {
    unsafe { CoCreateInstance::<_, IWICImagingFactory>(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok() }
}

fn decode(factory: &IWICImagingFactory, bytes: &[u8], max_side: u32) -> Option<Decoded> {
    let source = unsafe { factory.CreateStream().ok()? };
    unsafe { source.InitializeFromMemory(bytes).ok()? };
    let decoder = unsafe { factory.CreateDecoderFromStream(&source, std::ptr::null(), WICDecodeMetadataCacheOnLoad).ok()? };
    let frame = unsafe { decoder.GetFrame(0).ok()? };
    let (mut width, mut height) = (0u32, 0u32);
    unsafe { frame.GetSize(&mut width, &mut height).ok()? };
    if width == 0 || height == 0 || width > max_side || height > max_side {
        return None;
    }
    let converter = unsafe { factory.CreateFormatConverter().ok()? };
    let source: IWICBitmapSource = frame.cast().ok()?;
    unsafe { converter.Initialize(&source, &GUID_WICPixelFormat24bppBGR, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom).ok()? };
    let stride = (width * 3).div_ceil(4) * 4;
    let mut pixels = vec![0u8; stride as usize * height as usize];
    unsafe { converter.CopyPixels(std::ptr::null(), stride, &mut pixels).ok()? };
    Some(Decoded { width, height, stride, pixels })
}

fn read_stream(stream: &IStream) -> Option<Vec<u8>> {
    let mut stat = STATSTG::default();
    unsafe { stream.Stat(&mut stat, STATFLAG_NONAME).ok()? };
    let len = usize::try_from(stat.cbSize).ok()?;
    if len == 0 {
        return None;
    }
    unsafe { stream.Seek(0, STREAM_SEEK_SET, None).ok()? };
    let mut out = vec![0u8; len];
    let mut at = 0usize;
    while at < len {
        let mut read = 0u32;
        unsafe { stream.Read(out[at..].as_mut_ptr().cast(), (len - at).min(1 << 20) as u32, Some(&mut read)).ok().ok()? };
        if read == 0 {
            break;
        }
        at += read as usize;
    }
    out.truncate(at);
    (!out.is_empty()).then_some(out)
}

/// Joins `slices` (each a PNG or JPEG of the same width, top to bottom) into one picture in `format`.
/// One slice is returned as it is when it is already in that format. COM must be initialised on the
/// calling thread, as it is on the window thread.
pub fn stitch(slices: &[Vec<u8>], format: CaptureFormat) -> Result<Vec<u8>, String> {
    let failed = || "The captured page could not be put together.".to_owned();
    if slices.is_empty() {
        return Err(failed());
    }
    if slices.len() == 1 && kind_of(&slices[0]) == Some(format) {
        return Ok(slices[0].clone());
    }
    let factory = factory().ok_or_else(failed)?;
    // First pass: sizes only, so the output is sized before any pixel is written.
    let mut width = 0u32;
    let mut total = 0u64;
    let mut decoded_sizes = Vec::with_capacity(slices.len());
    for slice in slices {
        let source = unsafe { factory.CreateStream().map_err(|_| failed())? };
        unsafe { source.InitializeFromMemory(slice).map_err(|_| failed())? };
        let decoder = unsafe { factory.CreateDecoderFromStream(&source, std::ptr::null(), WICDecodeMetadataCacheOnLoad).map_err(|_| failed())? };
        let frame = unsafe { decoder.GetFrame(0).map_err(|_| failed())? };
        let (mut w, mut h) = (0u32, 0u32);
        unsafe { frame.GetSize(&mut w, &mut h).map_err(|_| failed())? };
        if w == 0 || h == 0 || (width != 0 && w != width) {
            return Err(failed());
        }
        width = w;
        total += u64::from(h);
        decoded_sizes.push(h);
    }
    if total > u64::from(if format == CaptureFormat::Jpeg { MAX_JPEG_SIDE } else { 200_000 }) || width > 65_000 {
        return Err("This page is too large to save as one picture at this display scale. Try PNG, or capture the visible area.".to_owned());
    }
    let sink: IStream = unsafe { SHCreateMemStream(None) }.ok_or_else(failed)?;
    let container = if format == CaptureFormat::Jpeg { &GUID_ContainerFormatJpeg } else { &GUID_ContainerFormatPng };
    let encoder = unsafe { factory.CreateEncoder(container, std::ptr::null()).map_err(|_| failed())? };
    unsafe { encoder.Initialize(&sink, WICBitmapEncoderNoCache).map_err(|_| failed())? };
    let mut target: Option<IWICBitmapFrameEncode> = None;
    let mut options: Option<IPropertyBag2> = None;
    unsafe { encoder.CreateNewFrame(&mut target, &mut options).map_err(|_| failed())? };
    let target = target.ok_or_else(failed)?;
    unsafe {
        target.Initialize(options.as_ref()).map_err(|_| failed())?;
        target.SetSize(width, total as u32).map_err(|_| failed())?;
        let mut pixel_format = GUID_WICPixelFormat24bppBGR;
        target.SetPixelFormat(&mut pixel_format).map_err(|_| failed())?;
    }
    // Second pass: one slice at a time, so only one decoded slice is ever in memory.
    for (slice, expected) in slices.iter().zip(decoded_sizes) {
        let picture = decode(&factory, slice, 70_000).ok_or_else(failed)?;
        if picture.width != width || picture.height != expected {
            return Err(failed());
        }
        unsafe { target.WritePixels(picture.height, picture.stride, &picture.pixels).map_err(|_| failed())? };
    }
    unsafe {
        target.Commit().map_err(|_| failed())?;
        encoder.Commit().map_err(|_| failed())?;
    }
    read_stream(&sink).ok_or_else(failed)
}

/// A device-independent bitmap (`CF_DIB`: header and pixels, bottom-up, 24 bits) of a captured picture,
/// for the clipboard. `Err` says why when the picture is too large or cannot be read.
pub fn dib_of(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let failed = || "The capture could not be read for the clipboard.".to_owned();
    kind_of(bytes).ok_or_else(failed)?;
    let factory = factory().ok_or_else(failed)?;
    let picture = decode(&factory, bytes, 70_000).ok_or_else(failed)?;
    if u64::from(picture.stride) * u64::from(picture.height) > MAX_CLIPBOARD_BYTES {
        return Err("This capture is too large to copy. Save it to a file instead.".to_owned());
    }
    let image_size = picture.stride * picture.height;
    let mut out = Vec::with_capacity(40 + image_size as usize);
    out.extend_from_slice(&40u32.to_le_bytes()); // biSize
    out.extend_from_slice(&(picture.width as i32).to_le_bytes());
    out.extend_from_slice(&(picture.height as i32).to_le_bytes()); // positive: rows run bottom to top
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&24u16.to_le_bytes()); // bits per pixel
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&image_size.to_le_bytes());
    out.extend_from_slice(&[0; 16]); // resolution, colours used and important
    for row in (0..picture.height).rev() {
        let start = (row * picture.stride) as usize;
        out.extend_from_slice(&picture.pixels[start..start + picture.stride as usize]);
    }
    Ok(out)
}

/// The width and height of a PNG or JPEG, read with WIC.
pub fn size_of(bytes: &[u8]) -> Option<(u32, u32)> {
    let factory = factory()?;
    let source = unsafe { factory.CreateStream().ok()? };
    unsafe { source.InitializeFromMemory(bytes).ok()? };
    let decoder = unsafe { factory.CreateDecoderFromStream(&source, std::ptr::null(), WICDecodeMetadataCacheOnLoad).ok()? };
    let frame = unsafe { decoder.GetFrame(0).ok()? };
    let (mut w, mut h) = (0u32, 0u32);
    unsafe { frame.GetSize(&mut w, &mut h).ok()? };
    Some((w, h))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::chrome_host::protocol::base64_encode;

    pub(crate) fn com() {
        use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    }

    /// A PNG or JPEG of `width` x `height` pixels, each row the colour `shade(row)`, made with WIC.
    pub(crate) fn picture(width: u32, height: u32, format: CaptureFormat, shade: impl Fn(u32) -> u8) -> Vec<u8> {
        com();
        let factory = factory().expect("WIC");
        let stride = (width * 3).div_ceil(4) * 4;
        let mut pixels = vec![0u8; (stride * height) as usize];
        for row in 0..height {
            let v = shade(row);
            for x in 0..width {
                let at = (row * stride + x * 3) as usize;
                pixels[at..at + 3].copy_from_slice(&[v, v, v]);
            }
        }
        let sink: IStream = unsafe { SHCreateMemStream(None) }.expect("stream");
        let container = if format == CaptureFormat::Jpeg { &GUID_ContainerFormatJpeg } else { &GUID_ContainerFormatPng };
        let encoder = unsafe { factory.CreateEncoder(container, std::ptr::null()).unwrap() };
        unsafe { encoder.Initialize(&sink, WICBitmapEncoderNoCache).unwrap() };
        let mut target: Option<IWICBitmapFrameEncode> = None;
        let mut options: Option<IPropertyBag2> = None;
        unsafe { encoder.CreateNewFrame(&mut target, &mut options).unwrap() };
        let target = target.unwrap();
        unsafe {
            target.Initialize(options.as_ref()).unwrap();
            target.SetSize(width, height).unwrap();
            let mut f = GUID_WICPixelFormat24bppBGR;
            target.SetPixelFormat(&mut f).unwrap();
            target.WritePixels(height, stride, &pixels).unwrap();
            target.Commit().unwrap();
            encoder.Commit().unwrap();
        }
        read_stream(&sink).expect("bytes")
    }

    /// What `Page.captureScreenshot` answers with.
    pub(crate) fn answer(bytes: &[u8]) -> String {
        format!("{{\"data\":\"{}\"}}", base64_encode(bytes))
    }

    #[test]
    fn screenshot_parameters_follow_the_format_and_the_clip() {
        assert_eq!(shot_params(CaptureFormat::Png, None), "{\"format\":\"png\",\"fromSurface\":true}");
        assert_eq!(
            shot_params(CaptureFormat::Jpeg, Some(Clip { x: 0, y: 6000, width: 1280, height: 3000 })),
            "{\"format\":\"jpeg\",\"fromSurface\":true,\"quality\":90,\"captureBeyondViewport\":true,\"clip\":{\"x\":0,\"y\":6000,\"width\":1280,\"height\":3000,\"scale\":1}}"
        );
    }

    #[test]
    fn layout_metrics_are_read_from_the_css_content_size() {
        let answer = r#"{"layoutViewport":{"pageX":0},"contentSize":{"x":0,"y":0,"width":1000,"height":2000},"cssContentSize":{"x":0,"y":0,"width":1280.5,"height":5123.25}}"#;
        assert_eq!(parse_metrics(answer), Some((1280.5, 5123.25)));
        assert_eq!(parse_metrics(r#"{"contentSize":{"width":800,"height":600}}"#), Some((800.0, 600.0)));
        assert_eq!(parse_metrics(r#"{"cssContentSize":{"width":"x","height":1}}"#), None);
        assert_eq!(parse_metrics("[]"), None);
        assert_eq!(parse_metrics(""), None);
    }

    #[test]
    fn a_screenshot_answer_must_be_a_real_png_or_jpeg() {
        let png = picture(8, 4, CaptureFormat::Png, |_| 10);
        assert_eq!(parse_shot(&answer(&png)), Some(png.clone()));
        let jpeg = picture(8, 4, CaptureFormat::Jpeg, |_| 10);
        assert_eq!(parse_shot(&answer(&jpeg)), Some(jpeg));
        assert_eq!(kind_of(&png), Some(CaptureFormat::Png));
        for bad in [
            r#"{"data":""}"#.to_owned(),
            r#"{"data":"AAAA"}"#.to_owned(),
            r#"{"nodata":"AAAA"}"#.to_owned(),
            answer(b"<html>not an image</html>"),
            r#"{"data":"%%%%"}"#.to_owned(),
            "null".to_owned(),
        ] {
            assert_eq!(parse_shot(&bad), None, "{bad}");
        }
    }

    #[test]
    fn a_selection_answer_is_text_and_whole_numbers_only() {
        let raw = |inner: &str| format!("{:?}", inner);
        let ok = r#"{"c":1,"t":"Hello\nworld","x":10,"y":-4,"w":300,"h":40,"a":"https://example.test/p","n":"A page"}"#;
        let parsed = parse_selection(&raw(ok)).unwrap().unwrap();
        assert_eq!(parsed.text, "Hello\nworld");
        assert_eq!(parsed.rect, (10, -4, 300, 40));
        assert_eq!(parsed.address, "https://example.test/p");
        assert_eq!(parse_selection(&raw(r#"{"c":0}"#)), Ok(None));
        for bad in [r#"{"c":2}"#, r#"{"c":1,"t":5,"x":1,"y":1,"w":1,"h":1,"a":"","n":""}"#, r#"{"c":1,"t":"x","x":1.5,"y":1,"w":1,"h":1,"a":"","n":""}"#, "[]", "{"] {
            assert_eq!(parse_selection(&raw(bad)), Err(BadAnswer), "{bad}");
        }
        assert_eq!(parse_selection("not json"), Err(BadAnswer));
        assert_eq!(parse_selection(&"x".repeat(SELECTION_ANSWER_LIMIT + 1)), Err(BadAnswer));
    }

    #[test]
    fn a_file_is_saved_with_a_unique_name_and_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("bowser-capture-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let first = save_file(&dir.join("deep"), "shot", CaptureFormat::Png, b"one").unwrap();
        let second = save_file(&dir.join("deep"), "shot", CaptureFormat::Png, b"two").unwrap();
        assert_eq!(first.file_name().unwrap(), "shot.png");
        assert_eq!(second.file_name().unwrap(), "shot (2).png");
        assert_eq!(std::fs::read(&first).unwrap(), b"one");
        assert_eq!(std::fs::read(&second).unwrap(), b"two");
        let jpg = save_file(&dir.join("deep"), "shot", CaptureFormat::Jpeg, b"j").unwrap();
        assert_eq!(jpg.file_name().unwrap(), "shot.jpg");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_invalid_folder_setting_falls_back_to_the_default() {
        assert_eq!(resolve_dir("C:\\Users\\x\\caps"), Some(PathBuf::from("C:\\Users\\x\\caps")));
        let fallback = resolve_dir("");
        assert!(fallback.as_ref().is_some_and(|dir| dir.ends_with("Bowser")), "{fallback:?}");
        assert_eq!(resolve_dir("..\\..\\windows"), fallback);
        assert_eq!(resolve_dir("\\\\server\\share"), fallback);
    }

    #[test]
    fn slices_are_stitched_top_to_bottom_into_one_picture() {
        // Three slices of different shades make one 6 x 9 PNG whose rows keep their order.
        let slices = [10u8, 120, 240].map(|shade| picture(6, 3, CaptureFormat::Png, move |_| shade)).to_vec();
        let whole = stitch(&slices, CaptureFormat::Png).expect("stitched");
        assert_eq!(kind_of(&whole), Some(CaptureFormat::Png));
        assert_eq!(size_of(&whole), Some((6, 9)));
        let rows = decode(&factory().unwrap(), &whole, 100).unwrap();
        let shade = |row: u32| rows.pixels[(row * rows.stride) as usize];
        assert_eq!([shade(0), shade(2), shade(3), shade(5), shade(6), shade(8)], [10, 10, 120, 120, 240, 240]);
        // JPEG output from PNG slices, and a single slice already in the format is returned unchanged.
        let jpeg = stitch(&slices, CaptureFormat::Jpeg).expect("jpeg");
        assert_eq!(kind_of(&jpeg), Some(CaptureFormat::Jpeg));
        assert_eq!(size_of(&jpeg), Some((6, 9)));
        assert_eq!(stitch(&slices[..1], CaptureFormat::Png).unwrap(), slices[0]);
        // Slices of different widths, and no slices at all, are refused.
        let odd = vec![picture(6, 3, CaptureFormat::Png, |_| 1), picture(7, 3, CaptureFormat::Png, |_| 1)];
        assert!(stitch(&odd, CaptureFormat::Png).is_err());
        assert!(stitch(&[], CaptureFormat::Png).is_err());
        assert!(stitch(&[b"junk".to_vec(), b"junk".to_vec()], CaptureFormat::Png).is_err());
    }

    #[test]
    fn a_30000_pixel_page_is_stitched_from_its_five_slices() {
        let plan = crate::page_tools::plan_slices(crate::page_tools::MAX_PAGE_HEIGHT);
        assert_eq!(plan.len(), 5);
        let slices: Vec<Vec<u8>> = plan.iter().map(|(top, height)| picture(4, *height, CaptureFormat::Png, move |row| ((top + row) / 120) as u8)).collect();
        let whole = stitch(&slices, CaptureFormat::Png).expect("stitched");
        assert_eq!(size_of(&whole), Some((4, 30_000)));
    }

    #[test]
    fn the_clipboard_bitmap_is_a_bottom_up_24_bit_dib() {
        let png = picture(5, 2, CaptureFormat::Png, |row| if row == 0 { 200 } else { 20 });
        let dib = dib_of(&png).expect("dib");
        assert_eq!(u32::from_le_bytes(dib[0..4].try_into().unwrap()), 40);
        assert_eq!(i32::from_le_bytes(dib[4..8].try_into().unwrap()), 5);
        assert_eq!(i32::from_le_bytes(dib[8..12].try_into().unwrap()), 2);
        assert_eq!(u16::from_le_bytes(dib[14..16].try_into().unwrap()), 24);
        let stride = 16usize; // 5 * 3 bytes, padded to 4
        assert_eq!(dib.len(), 40 + stride * 2);
        assert_eq!(dib[40], 20, "the bottom row comes first");
        assert_eq!(dib[40 + stride], 200);
        assert!(dib_of(b"not an image").is_err());
    }
}
