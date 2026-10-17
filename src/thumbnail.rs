//! Page thumbnails for the tab hover card (Settings > Appearance > "Show a thumbnail in tab previews").
//!
//! The engine captures a tab's page as a JPEG the size of the page; [`downscale_jpeg`] shrinks it
//! with WIC to a card-sized JPEG of a few kilobytes, and [`data_uri`] is what the hover card
//! carries. Nothing here touches the network or the disk: a thumbnail lives in memory with its tab.

/// The thumbnail's bounding box in pixels (twice the card's 120 x 75 CSS pixels, so it is sharp at
/// 200% scaling).
pub const THUMB_W: u32 = 240;
pub const THUMB_H: u32 = 150;
/// Largest JPEG a thumbnail may be, and so the largest the hover message carries (about 48 KB as text).
pub const MAX_THUMB_BYTES: usize = 36_000;
/// Largest captured page image the decoder is given.
pub const MAX_CAPTURE_BYTES: usize = 6 * 1024 * 1024;

/// The size `width` x `height` shrinks to inside `max_w` x `max_h`, keeping its shape. Never larger
/// than the source and never zero.
pub fn fit(width: u32, height: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    if width == 0 || height == 0 || max_w == 0 || max_h == 0 {
        return (1, 1);
    }
    let scale = (f64::from(max_w) / f64::from(width)).min(f64::from(max_h) / f64::from(height)).min(1.0);
    (((f64::from(width) * scale).round() as u32).clamp(1, max_w), ((f64::from(height) * scale).round() as u32).clamp(1, max_h))
}

/// Whether `bytes` is a JPEG small enough to send: it starts with the JPEG marker and is within
/// [`MAX_THUMB_BYTES`].
pub fn sendable(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes.len() <= MAX_THUMB_BYTES && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[bytes.len() - 2] == 0xFF && bytes[bytes.len() - 1] == 0xD9
}

/// `data:image/jpeg;base64,...` for a sendable thumbnail.
pub fn data_uri(bytes: &[u8]) -> Option<String> {
    sendable(bytes).then(|| format!("data:image/jpeg;base64,{}", crate::chrome_host::protocol::base64_encode(bytes)))
}

/// Shrinks a captured page image (JPEG, PNG or BMP) to fit `max_w` x `max_h` and encodes it as a
/// JPEG. None for an image that cannot be decoded, is too large, or whose thumbnail would be over
/// [`MAX_THUMB_BYTES`]. COM must be initialised on the calling thread, as it is on the UI thread.
#[cfg(windows)]
pub fn downscale_jpeg(bytes: &[u8], max_w: u32, max_h: u32) -> Option<Vec<u8>> {
    use windows::Win32::{
        Graphics::Imaging::{
            CLSID_WICImagingFactory, GUID_ContainerFormatJpeg, GUID_WICPixelFormat24bppBGR, IWICBitmapFrameEncode, IWICImagingFactory,
            WICBitmapDitherTypeNone, WICBitmapEncoderNoCache, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom,
            WICDecodeMetadataCacheOnLoad,
        },
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IStream, STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET, StructuredStorage::IPropertyBag2},
        UI::Shell::SHCreateMemStream,
    };
    if bytes.is_empty() || bytes.len() > MAX_CAPTURE_BYTES || max_w == 0 || max_h == 0 || max_w > 2048 || max_h > 2048 {
        return None;
    }
    let factory = unsafe { CoCreateInstance::<_, IWICImagingFactory>(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()? };
    let source = unsafe { factory.CreateStream().ok()? };
    unsafe { source.InitializeFromMemory(bytes).ok()? };
    let decoder = unsafe { factory.CreateDecoderFromStream(&source, std::ptr::null(), WICDecodeMetadataCacheOnLoad).ok()? };
    let frame = unsafe { decoder.GetFrame(0).ok()? };
    let (mut width, mut height) = (0u32, 0u32);
    unsafe { frame.GetSize(&mut width, &mut height).ok()? };
    if width == 0 || height == 0 || width > 16_384 || height > 16_384 {
        return None;
    }
    let (out_w, out_h) = fit(width, height, max_w, max_h);
    let scaler = unsafe { factory.CreateBitmapScaler().ok()? };
    unsafe { scaler.Initialize(&frame, out_w, out_h, WICBitmapInterpolationModeFant).ok()? };
    // JPEG has no alpha: convert to 24-bit BGR after scaling.
    let converter = unsafe { factory.CreateFormatConverter().ok()? };
    unsafe { converter.Initialize(&scaler, &GUID_WICPixelFormat24bppBGR, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom).ok()? };

    let sink: IStream = unsafe { SHCreateMemStream(None)? };
    let encoder = unsafe { factory.CreateEncoder(&GUID_ContainerFormatJpeg, std::ptr::null()).ok()? };
    unsafe { encoder.Initialize(&sink, WICBitmapEncoderNoCache).ok()? };
    let mut target: Option<IWICBitmapFrameEncode> = None;
    let mut options: Option<IPropertyBag2> = None;
    unsafe { encoder.CreateNewFrame(&mut target, &mut options).ok()? };
    let target = target?;
    unsafe {
        target.Initialize(options.as_ref()).ok()?;
        target.SetSize(out_w, out_h).ok()?;
        let mut format = GUID_WICPixelFormat24bppBGR;
        target.SetPixelFormat(&mut format).ok()?;
        target.WriteSource(&converter, std::ptr::null()).ok()?;
        target.Commit().ok()?;
        encoder.Commit().ok()?;
    }
    let mut stat = STATSTG::default();
    unsafe { sink.Stat(&mut stat, STATFLAG_NONAME).ok()? };
    let len = usize::try_from(stat.cbSize).ok()?;
    if len == 0 || len > MAX_THUMB_BYTES {
        return None;
    }
    unsafe { sink.Seek(0, STREAM_SEEK_SET, None).ok()? };
    let mut out = vec![0u8; len];
    let mut read = 0u32;
    unsafe { sink.Read(out.as_mut_ptr().cast(), len as u32, Some(&mut read)).ok().ok()? };
    out.truncate(read as usize);
    sendable(&out).then_some(out)
}

#[cfg(not(windows))]
pub fn downscale_jpeg(_bytes: &[u8], _max_w: u32, _max_h: u32) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_image_shrinks_inside_the_box_and_never_grows() {
        assert_eq!(fit(1280, 720, 240, 150), (240, 135));
        assert_eq!(fit(800, 800, 240, 150), (150, 150));
        assert_eq!(fit(100, 60, 240, 150), (100, 60), "a small image is not enlarged");
        assert_eq!(fit(4000, 10, 240, 150), (240, 1), "never zero");
        assert_eq!(fit(0, 10, 240, 150), (1, 1));
    }

    #[test]
    fn only_a_small_whole_jpeg_is_sendable() {
        let jpeg = [0xFF, 0xD8, 1, 2, 3, 0xFF, 0xD9];
        assert!(sendable(&jpeg));
        assert!(!sendable(&[0xFF, 0xD8, 1, 2, 3]), "no end marker");
        assert!(!sendable(&[0x89, b'P', b'N', b'G', 0, 0]), "a PNG is not what the card shows");
        assert!(!sendable(&[]));
        let mut big = vec![0xFF, 0xD8];
        big.resize(MAX_THUMB_BYTES + 1, 0);
        big[MAX_THUMB_BYTES - 1] = 0xFF;
        assert!(!sendable(&big));
        assert!(data_uri(&jpeg).is_some_and(|uri| uri.starts_with("data:image/jpeg;base64,/9g")));
        assert!(data_uri(&[1, 2, 3]).is_none());
    }

    /// A 24-bit BMP of a horizontal gradient, which WIC decodes like any captured page image.
    #[cfg(windows)]
    fn gradient_bmp(width: u32, height: u32) -> Vec<u8> {
        let row = (width * 3).div_ceil(4) * 4;
        let size = 54 + row * height;
        let mut out = Vec::with_capacity(size as usize);
        out.extend_from_slice(b"BM");
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&54u32.to_le_bytes());
        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&24u16.to_le_bytes());
        out.extend_from_slice(&[0; 24]);
        for _ in 0..height {
            for x in 0..width {
                let v = (x * 255 / width.max(1)) as u8;
                out.extend_from_slice(&[v, 128, 255 - v]);
            }
            out.resize(out.len() + (row - width * 3) as usize, 0);
        }
        out
    }

    #[cfg(windows)]
    #[test]
    fn a_captured_page_becomes_a_card_sized_jpeg() {
        use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let bmp = gradient_bmp(1280, 720);
        let jpeg = downscale_jpeg(&bmp, THUMB_W, THUMB_H).expect("a thumbnail");
        assert!(sendable(&jpeg), "{} bytes", jpeg.len());
        assert!(jpeg.len() < MAX_THUMB_BYTES);
        // The JPEG itself decodes back to the size `fit` promised.
        let again = downscale_jpeg(&jpeg, 2048, 2048).expect("decodes again");
        assert!(sendable(&again));
        assert!(downscale_jpeg(b"not an image", THUMB_W, THUMB_H).is_none());
        assert!(downscale_jpeg(&[], THUMB_W, THUMB_H).is_none());
        assert!(downscale_jpeg(&bmp, 0, 10).is_none());
    }
}
