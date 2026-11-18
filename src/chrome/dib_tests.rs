//! Faults only the next GDI allocation boundary; all surface state, pixels and drawing stay real.
use super::*;
use std::cell::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    Dc,
    Bitmap,
    NullBits,
    Selection,
    SelectionError,
}

thread_local! {
    static FAILURE: Cell<Option<Failure>> = const { Cell::new(None) };
    static CANDIDATE: Cell<(HDC, HBITMAP)> = const { Cell::new((HDC(ptr::null_mut()), HBITMAP(ptr::null_mut()))) };
}

struct FailNext;
impl FailNext {
    fn at(failure: Failure) -> Self {
        FAILURE.with(|slot| assert!(slot.replace(Some(failure)).is_none()));
        Self
    }
}
impl Drop for FailNext {
    fn drop(&mut self) {
        FAILURE.with(|slot| slot.set(None));
    }
}

fn take(failure: Failure) -> bool {
    FAILURE.with(|slot| {
        if slot.get() == Some(failure) {
            slot.set(None);
            true
        } else {
            false
        }
    })
}

pub(super) unsafe fn create_dc() -> HDC {
    let dc = if take(Failure::Dc) { HDC::default() } else { unsafe { CreateCompatibleDC(None) } };
    CANDIDATE.with(|slot| slot.set((dc, HBITMAP::default())));
    dc
}

pub(super) unsafe fn create_bitmap(
    dc: HDC,
    info: &BITMAPINFO,
    bits: *mut *mut std::ffi::c_void,
) -> windows::core::Result<HBITMAP> {
    if take(Failure::Bitmap) {
        return Err(windows::core::Error::from_hresult(windows::core::HRESULT(0x8007000E_u32 as i32)));
    }
    let bitmap = unsafe { CreateDIBSection(Some(dc), info, DIB_RGB_COLORS, bits, None, 0) };
    if let Ok(bitmap) = &bitmap {
        CANDIDATE.with(|slot| slot.set((dc, *bitmap)));
    }
    if take(Failure::NullBits) {
        unsafe { *bits = ptr::null_mut(); }
    }
    bitmap
}

pub(super) unsafe fn select_bitmap(dc: HDC, bitmap: HBITMAP) -> HGDIOBJ {
    if take(Failure::Selection) {
        HGDIOBJ::default()
    } else if take(Failure::SelectionError) {
        HGDIOBJ(-1_isize as *mut std::ffi::c_void)
    } else {
        unsafe { SelectObject(dc, bitmap.into()) }
    }
}

fn replacement_keeps_pixels(failure: Failure) {
    let mut surface = ChromeSurface::new(320, 100, 96);
    surface.dib.pixels_mut().fill(0x7b);
    let original = surface.pixels().to_vec();
    let _fault = FailNext::at(failure);
    surface.resize(800, 600, 144);
    assert_eq!((surface.width(), surface.height(), surface.dpi), (320, 100, 96));
    assert!(surface.pixels() == original, "the last usable surface survives {failure:?}");
    let expected = match failure {
        Failure::Dc => SurfaceError::CreateDc,
        Failure::Bitmap | Failure::NullBits => SurfaceError::CreateBitmap,
        Failure::Selection | Failure::SelectionError => SurfaceError::SelectBitmap,
    };
    assert_eq!(surface.allocation_error(), Some(expected));
    assert_eq!(surface.stride(), 1280);
    // A subsequent draw still uses the retained allocation's bounds and stride.
    surface.fill_rect(Rect::new(0, 0, 800, 600), Rgba { r: 1, g: 2, b: 3, a: 255 }, Rect::new(0, 0, 800, 600));
    assert_eq!(&surface.pixels()[..4], &[3, 2, 1, 255]);
    surface.resize(800, 600, 144);
    assert_eq!((surface.width(), surface.height(), surface.dpi), (800, 600, 144));
    assert!(surface.pixels().len() >= 800 * 600 * 4);
    assert_eq!(surface.allocation_error(), None);
}

#[test]
fn bitmap_failure_preserves_the_previous_surface() { replacement_keeps_pixels(Failure::Bitmap); }
#[test]
fn dc_failure_preserves_the_previous_surface() { replacement_keeps_pixels(Failure::Dc); }
#[test]
fn null_bits_preserve_the_previous_surface() { replacement_keeps_pixels(Failure::NullBits); }
#[test]
fn selection_failure_preserves_the_previous_surface() { replacement_keeps_pixels(Failure::Selection); }
#[test]
fn selection_error_preserves_the_previous_surface() { replacement_keeps_pixels(Failure::SelectionError); }

#[test]
fn initial_failure_recovers_at_the_same_requested_size() {
    let _fault = FailNext::at(Failure::Bitmap);
    let mut surface = ChromeSurface::new(320, 100, 96);
    assert!(surface.pixels().is_empty());
    assert_eq!((surface.width(), surface.height(), surface.stride()), (0, 0, 0));
    assert_eq!(surface.allocation_error(), Some(SurfaceError::CreateBitmap));
    assert_eq!(surface.try_resize(320, 100, 96), Ok(true));
    assert_eq!((surface.width(), surface.height()), (320, 100));
    assert!(surface.pixels().len() >= 320 * 100 * 4, "same-size resize must retry");
}

#[test]
fn sidebar_is_safe_without_an_initial_allocation() {
    let _fault = FailNext::at(Failure::Bitmap);
    let mut surface = ChromeSurface::new(640, 400, 96);
    let sidebar = crate::vertical_tabs::SidebarLayout::compute_in_frame_at(
        640, 400, 96, 260, false, crate::settings::SidePanelPosition::Left, 100, 400, 28,
    );
    surface.paint_vertical_sidebar(SidebarPaintView {
        layout: &sidebar, rows: &[], workspace: "Work", active: crate::tabs::TabId(1),
        scroll: 0, hover_row: None, html_sidebar: false,
    }, Rect::new(0, 0, 640, 400), crate::theme::palette(crate::theme::Appearance::Dark, false));
    assert!(surface.pixels().is_empty());
    surface.resize(640, 400, 96);
    assert!(!surface.pixels().is_empty());
}

#[test]
fn split_drop_is_safe_without_an_initial_allocation() {
    let _fault = FailNext::at(Failure::Bitmap);
    let mut surface = ChromeSurface::new(640, 400, 96);
    surface.paint_split_drop(Rect::new(100, 100, 300, 300), crate::theme::palette(crate::theme::Appearance::Dark, false), Rect::new(0, 0, 640, 400));
    assert!(surface.pixels().is_empty());
}

#[test]
fn tile_titles_are_safe_without_an_initial_allocation() {
    let _fault = FailNext::at(Failure::Bitmap);
    let mut surface = ChromeSurface::new(640, 400, 96);
    surface.paint_tile_titles(&[(Rect::new(100, 100, 300, 130), "Title".into(), true)], crate::theme::palette(crate::theme::Appearance::Dark, false), Rect::new(0, 0, 640, 400));
    assert!(surface.pixels().is_empty());
}

#[test]
fn unrepresentable_size_preserves_the_previous_surface() {
    let mut surface = ChromeSurface::new(320, 100, 96);
    surface.dib.pixels_mut().fill(0x62);
    let original = surface.pixels().to_vec();
    assert_eq!(surface.try_resize(i32::MAX, 1, 96), Err(SurfaceError::InvalidSize));
    assert_eq!((surface.width(), surface.height()), (320, 100));
    assert!(surface.pixels() == original);
}

#[test]
fn failed_text_mask_growth_retains_its_pixels() {
    let mut text = TextCache::new(320, 100, 96);
    text.draw(&[b'A' as u16], Rect::new(0, 0, 100, 30), Rect::new(0, 0, 100, 30), true);
    let original = text.dib.pixels().to_vec();
    assert!(original.iter().any(|&b| b != 0));
    let _fault = FailNext::at(Failure::Bitmap);
    text.resize(800, 600, 96);
    assert_eq!((text.dib.width, text.dib.height), (320, 100));
    assert!(text.dib.pixels() == original);
    text.draw(&[b'B' as u16], Rect::new(600, 500, 700, 530), Rect::new(600, 500, 700, 530), true);
    assert_eq!((text.dib.width, text.dib.height), (800, 600));
}

#[test]
fn rejected_replacements_release_their_gdi_handles() {
    let mut dib = Dib::new(64, 64);
    for failure in [Failure::Bitmap, Failure::NullBits, Failure::Selection, Failure::SelectionError] {
        let _fault = FailNext::at(failure);
        assert!(dib.resize(256, 256).is_err());
        let (dc, bitmap) = CANDIDATE.with(Cell::get);
        assert_released(dc, bitmap);
    }
}

fn assert_released(dc: HDC, bitmap: HBITMAP) {
    use windows::Win32::Graphics::Gdi::{BITMAP, GetCurrentObject, GetObjectW, OBJ_BITMAP};
    // GetObjectType alone returned OBJ_DC for deleted DC handles on this Windows build.
    // Read actual object state rather than using that value as a liveness probe.
    unsafe {
        assert!(GetCurrentObject(dc, OBJ_BITMAP).0.is_null(), "released DC has no selected bitmap");
        if !bitmap.0.is_null() {
            let mut info = BITMAP::default();
            assert_eq!(GetObjectW(bitmap.into(), size_of::<BITMAP>() as i32, Some((&mut info as *mut BITMAP).cast())), 0,
                "released bitmap has no readable object state");
        }
    }
}

#[test]
fn failed_mask_draw_never_composites_stale_pixels() {
    let mut surface = ChromeSurface::new(800, 600, 96);
    surface.text.wanted = (320, 100);
    let label = Rect::new(4, 4, 100, 28);
    surface.draw_text("Old text", label, Rgba { r: 255, g: 255, b: 255, a: 255 }, true, label, label);
    assert!(surface.pixels().iter().any(|&byte| byte != 0));
    let old_mask = surface.text.dib.pixels().to_vec();
    surface.dib.pixels_mut().fill(0);
    surface.text.wanted = (800, 600);
    let _fault = FailNext::at(Failure::Bitmap);
    surface.draw_text("", label, Rgba { r: 255, g: 255, b: 255, a: 255 }, true, label, label);
    assert!(surface.pixels().iter().all(|&byte| byte == 0), "a failed draw must not reuse old glyphs");
    assert!(surface.text.dib.pixels() == old_mask, "keep the old mask until replacement succeeds");
}

#[test]
fn failed_mask_release_retains_a_usable_dc() {
    let mut text = TextCache::new(320, 100, 96);
    text.draw(&[b'A' as u16], Rect::new(0, 0, 100, 30), Rect::new(0, 0, 100, 30), true);
    let old_mask = text.dib.pixels().to_vec();
    let old_width = text.measure(&[b'A' as u16], 1);
    assert!(old_width > 0);
    let _fault = FailNext::at(Failure::Bitmap);
    text.release_mask();
    assert_eq!(text.measure(&[b'A' as u16], 1), old_width);
    assert!(text.dib.pixels() == old_mask);
    text.release_mask();
    assert_eq!((text.dib.width, text.dib.height), (1, 1));
}

#[test]
fn failed_initial_text_dc_recovers_on_resize() {
    let _fault = FailNext::at(Failure::Dc);
    let mut text = TextCache::new(320, 100, 96);
    text.resize(320, 100, 96);
    assert!(text.measure(&[b'A' as u16], 1) > 0, "same-size resize must recover font measurement");
}

#[test]
fn allocation_arithmetic_rejects_unrepresentable_images_before_gdi() {
    for size in [(i32::MAX, 1), (1, i32::MAX), (65536, 65536), (536870912, 1)] {
        assert_eq!(dib_allocation((0, 0), size), Err(SurfaceError::InvalidSize), "{size:?}");
    }
    assert_eq!(dib_allocation((0, 0), (1, 1)), Ok(Some((64, 64))));
    assert_eq!(dib_allocation((0, 0), (16384, 16384)), Ok(Some((16384, 16384))));
}

#[test]
fn selected_bitmap_is_replaced_and_all_owned_handles_are_released() {
    use windows::Win32::Graphics::Gdi::{GetCurrentObject, OBJ_BITMAP};
    let mut dib = Dib::new(64, 64);
    let old_dc = dib.dc;
    let old_bitmap = dib.bitmap;
    assert_eq!(dib.resize(256, 256), Ok(true));
    let dc = dib.dc;
    let bitmap = dib.bitmap;
    assert_released(old_dc, old_bitmap);
    unsafe {
        assert_eq!(GetCurrentObject(dc, OBJ_BITMAP), bitmap.into());
    }
    drop(dib);
    assert_released(dc, bitmap);
}

#[test]
fn failed_presentation_reports_failure_and_forces_a_full_retry() {
    let mut surface = ChromeSurface::new(320, 100, 96);
    surface.last_presented = Some(PresentedSurface { width: 320, height: 100, dpi: 96, origin: (0, 0) });
    assert!(!surface.present(HWND::default(), POINT::default(), Rect::new(0, 0, 10, 10)));
    assert_eq!(surface.last_presented, None);
    let _fault = FailNext::at(Failure::Bitmap);
    let mut absent = ChromeSurface::new(320, 100, 96);
    assert!(!absent.present(HWND::default(), POINT::default(), Rect::new(0, 0, 320, 100)));
}

#[test]
fn text_recovery_requests_a_full_repaint_at_unchanged_dimensions() {
    let mut surface = ChromeSurface::new(320, 100, 96);
    let _fault = FailNext::at(Failure::Dc);
    surface.text = TextCache::new(320, 100, 96);
    assert_eq!(surface.allocation_error(), Some(SurfaceError::CreateDc));
    assert_eq!(surface.try_resize(320, 100, 96), Ok(true), "recovered text needs a full repaint");
    assert!(surface.caret_x("A", 1) > 0);
    assert_eq!(surface.allocation_error(), None);
}

#[test]
fn text_recovery_html_takeover_does_not_remain_failed_for_an_unneeded_mask() {
    let mut surface = ChromeSurface::new(320, 100, 96);
    let label = Rect::new(4, 4, 100, 28);
    {
        let _fault = FailNext::at(Failure::Bitmap);
        surface.draw_text("Before takeover", label, Rgba { r: 255, g: 255, b: 255, a: 255 }, true, label, label);
    }
    assert_eq!(surface.allocation_error(), Some(SurfaceError::CreateBitmap));
    assert!(!surface.text.mask_allocated());
    surface.set_html_toolbar(true);
    surface.set_html_tabs(true);
    surface.set_html_rail(true);
    assert_eq!(surface.try_resize(320, 100, 96), Ok(true));
    let layout = layout::compute(320, 100, 96, 0, false);
    let view = ChromeView {
        layout: &layout,
        palette: crate::theme::palette(crate::theme::Appearance::Dark, false),
        tabs: &[], active: 0,
        hover: Hit::None, pressed: Hit::None, caption_maximised: false,
        can_back: false, can_forward: false,
        omnibox: OmniboxView {
            text: "", selection: (0, 0), caret_visible: false, focused: false,
            completion_suffix: "", progress: None, site: SiteSecurity::NoSite,
        },
        now: Instant::now(), departures: &[], rail: None, icon_motion: None,
        content_rounded: true, show_site_info: true, show_split: true,
    };
    surface.paint(&view, Rect::new(0, 0, 320, 100));
    assert_eq!(surface.allocation_error(), None, "a frame with HTML text needs no native mask allocation");
    assert!(!surface.text.mask_allocated(), "recovery must keep the small measurement bitmap");
    let mut fresh = ChromeSurface::new(320, 100, 96);
    fresh.set_html_toolbar(true);
    fresh.set_html_tabs(true);
    fresh.set_html_rail(true);
    fresh.paint(&view, Rect::new(0, 0, 320, 100));
    assert!(surface.pixels() == fresh.pixels());
}

#[test]
fn text_recovery_keeps_a_skipped_label_observable_for_the_frame() {
    let mut surface = ChromeSurface::new(320, 100, 96);
    let label = Rect::new(4, 4, 100, 28);
    let colour = Rgba { r: 255, g: 255, b: 255, a: 255 };
    let _fault = FailNext::at(Failure::Bitmap);
    surface.draw_text("First", label, colour, true, label, label);
    assert_eq!(surface.allocation_error(), Some(SurfaceError::CreateBitmap));
    surface.draw_text("Second", label, colour, true, label, label);
    assert!(surface.pixels().iter().any(|&byte| byte != 0), "later allocation succeeded");
    assert_eq!(surface.allocation_error(), Some(SurfaceError::CreateBitmap), "the skipped earlier label still needs recovery");
    assert_eq!(surface.try_resize(320, 100, 96), Ok(true));
    let layout = layout::compute(320, 100, 96, 0, false);
    let view = ChromeView {
        layout: &layout,
        palette: crate::theme::palette(crate::theme::Appearance::Dark, false),
        tabs: &[], active: 0,
        hover: Hit::None, pressed: Hit::None, caption_maximised: false,
        can_back: false, can_forward: false,
        omnibox: OmniboxView {
            text: "", selection: (0, 0), caret_visible: false, focused: false,
            completion_suffix: "", progress: None, site: SiteSecurity::NoSite,
        },
        now: Instant::now(), departures: &[], rail: None, icon_motion: None,
        content_rounded: true, show_site_info: true, show_split: true,
    };
    surface.paint(&view, Rect::new(0, 0, 1, 1));
    assert_eq!(surface.allocation_error(), Some(SurfaceError::CreateBitmap), "a partial paint cannot restore a skipped label elsewhere");
    surface.paint(&view, Rect::new(0, 0, 320, 100));
    assert_eq!(surface.allocation_error(), None);
    let mut fresh = ChromeSurface::new(320, 100, 96);
    fresh.paint(&view, Rect::new(0, 0, 320, 100));
    assert!(surface.pixels() == fresh.pixels(), "recovery paints the same pixels as a fresh surface");
}
