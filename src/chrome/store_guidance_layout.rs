//! Test-only reservation contract for a proposed native Store guidance strip.
//! No production layout/control uses this model yet. Measurements are physical pixels and must
//! come from the actual selected font/wrapping; this model does not authenticate a page or prove
//! text readability. The zero-guidance result preserves the existing bookmark reservation.

use super::layout::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rows {
    /// Page before bookmark reservation, used by the existing auto-hide page contract.
    auto_hide_page: Rect,
    /// A visible reserved bookmark row, or its separately identified floating row.
    bookmarks: Option<Rect>,
    bookmarks_floating: bool,
    guidance: Option<Rect>,
    page: Rect,
}

#[derive(Clone, Copy, Debug)]
struct Request {
    area: Rect,
    bookmark_height: i32,
    bookmark_bottom: bool,
    bookmark_reserved: bool,
    /// None means no authenticated current listing, unavailable Add surface, failed measurement,
    /// or failed control creation. No blank guidance space may be retained in those states.
    measured_guidance: Option<i32>,
    minimum_page_height: i32,
}

fn partition(request: Request) -> Option<Rows> {
    let Request { area, bookmark_height, bookmark_bottom, bookmark_reserved,
        measured_guidance, minimum_page_height } = request;
    let width = area.right.checked_sub(area.left)?;
    let height = area.bottom.checked_sub(area.top)?;
    if width < 0 || height < 0 { return None; }
    let bookmark_height = bookmark_height.max(0);
    // Match bookmark_bar::reserved_viewport: an existing reserved row leaves at least one pixel.
    let reserved = if bookmark_reserved { bookmark_height.min((height - 1).max(0)) } else { 0 };
    let minimum_page_height = minimum_page_height.max(1);
    let guidance_height = measured_guidance.filter(|&height| height > 0 && width > 0)
        .filter(|&wanted| wanted <= height.saturating_sub(reserved).saturating_sub(minimum_page_height))
        .unwrap_or(0);
    let guidance_bottom = area.top.checked_add(guidance_height)?;
    let auto_hide_page = Rect::new(area.left, guidance_bottom, area.right, area.bottom);
    let (page_top, page_bottom) = if bookmark_bottom {
        (guidance_bottom, area.bottom.checked_sub(reserved)?)
    } else {
        (guidance_bottom.checked_add(reserved)?, area.bottom)
    };
    let page = Rect::new(area.left, page_top, area.right, page_bottom);
    let row_height = if bookmark_reserved { reserved } else { bookmark_height.min(page.height().max(0)) };
    let bookmarks = (row_height > 0).then(|| {
        if bookmark_bottom {
            Rect::new(area.left, area.bottom - row_height, area.right, area.bottom)
        } else {
            Rect::new(area.left, guidance_bottom, area.right, guidance_bottom + row_height)
        }
    });
    Some(Rows { auto_hide_page, bookmarks, bookmarks_floating: !bookmark_reserved && bookmarks.is_some(),
        guidance: (guidance_height > 0).then(|| Rect::new(area.left, area.top, area.right, guidance_bottom)), page })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> Request { Request { area: Rect::new(10, 50, 1010, 750), bookmark_height: 28,
        bookmark_bottom: false, bookmark_reserved: true, measured_guidance: Some(40), minimum_page_height: 32 } }
    #[test]
    fn zero_guidance_preserves_existing_bookmark_reservation() {
        for bottom in [false, true] { for height in [0, 1, 10, 700] {
            let mut r = request(); r.area.bottom = r.area.top + height; r.bookmark_bottom = bottom; r.measured_guidance = None;
            let rows = partition(r).unwrap();
            let old_height = r.bookmark_height.min((height - 1).max(0));
            let expected = if bottom { Rect::new(r.area.left,r.area.top,r.area.right,r.area.bottom-old_height) }
                else { Rect::new(r.area.left,r.area.top+old_height,r.area.right,r.area.bottom) };
            assert_eq!(rows.page, expected);
            assert_eq!(rows.auto_hide_page, r.area); assert_eq!(rows.guidance, None);
        }}
    }
    #[test]
    fn top_rows_partition_once_without_overlap() {
        let rows = partition(request()).unwrap();
        assert_eq!(rows.guidance, Some(Rect::new(10,50,1010,90)));
        assert_eq!(rows.bookmarks, Some(Rect::new(10,90,1010,118)));
        assert_eq!(rows.page, Rect::new(10,118,1010,750));
        assert_eq!(rows.auto_hide_page, Rect::new(10,90,1010,750));
        assert!(!rows.bookmarks_floating);
    }
    #[test]
    fn bottom_bookmark_is_distinct_from_top_guidance() {
        let mut r=request();r.bookmark_bottom=true;let rows=partition(r).unwrap();
        assert_eq!(rows.bookmarks,Some(Rect::new(10,722,1010,750)));
        assert_eq!(rows.page,Rect::new(10,90,1010,722));
    }
    #[test]
    fn floating_bookmark_never_covers_the_reserved_guidance() {
        let mut r=request();r.bookmark_reserved=false;let rows=partition(r).unwrap();
        assert!(rows.bookmarks_floating);
        assert_eq!(rows.bookmarks.unwrap().top,rows.guidance.unwrap().bottom);
        assert_eq!(rows.page.top,rows.bookmarks.unwrap().top);
        assert_eq!(rows.page.bottom,750);
    }
    #[test]
    fn hidden_or_failed_guidance_keeps_no_blank_reserve() {
        for measurement in [None,Some(0),Some(-1),Some(i32::MAX)] {
            let mut r=request();r.measured_guidance=measurement;let rows=partition(r).unwrap();
            assert_eq!(rows.guidance,None);assert_eq!(rows.page.top,78);
        }
    }
    #[test]
    fn tiny_window_refuses_guidance_instead_of_truncating_or_consuming_page() {
        let mut r=request();r.area.bottom=120;let rows=partition(r).unwrap();
        assert_eq!(rows.guidance,None);assert_eq!(rows.page.height(),42);
    }
    #[test]
    fn physical_measurements_partition_at_representative_scales() {
        for factor in [1,2,3] { for bottom in [false,true] {
            let mut r=request();r.area=Rect::new(0,50*factor,1000*factor,750*factor);
            r.bookmark_height=28*factor;r.measured_guidance=Some(40*factor);r.bookmark_bottom=bottom;
            let rows=partition(r).unwrap();
            assert_eq!(rows.page.height()+rows.guidance.unwrap().height()+rows.bookmarks.unwrap().height(),r.area.height());
            assert_eq!(rows.page.width(),r.area.width());
        }}
    }
    #[test]
    fn empty_width_and_invalid_extents_cannot_reserve_guidance() {
        let mut r=request();r.area.right=r.area.left;assert_eq!(partition(r).unwrap().guidance,None);
        r.area.right=r.area.left-1;assert!(partition(r).is_none());
        r.area=Rect::new(i32::MIN,0,i32::MAX,10);assert!(partition(r).is_none());
    }
    #[test]
    fn repeated_resize_recomputes_instead_of_retaining_old_reserve() {
        let mut r=request();assert!(partition(r).unwrap().guidance.is_some());
        r.area.bottom=120;assert!(partition(r).unwrap().guidance.is_none());
        r.area.bottom=750;assert!(partition(r).unwrap().guidance.is_some());
        r.measured_guidance=None;assert!(partition(r).unwrap().guidance.is_none());
    }

    /// Native font/wrapping prerequisite only. No control is created and no production viewport
    /// consumes this measurement; screenshots and actual child-control lifecycle remain separate.
    #[test]
    fn actual_message_font_measurements_fit_the_partition_at_supported_scales() {
        use windows::Win32::{Foundation::RECT, Graphics::Gdi::{
            ANTIALIASED_QUALITY, CreateCompatibleDC, DeleteDC, DeleteObject, DT_CALCRECT,
            DT_NOPREFIX, DT_WORDBREAK, GetTextMetricsW, HDC, HFONT, HGDIOBJ, SelectObject,
            TEXTMETRICW,
        }};
        #[link(name = "user32")]
        unsafe extern "system" {
            #[link_name = "DrawTextW"]
            fn measure_text(dc: HDC, text: *const u16, count: i32, rect: *mut RECT, format: u32) -> i32;
        }
        struct SelectedFont { dc: HDC, font: HFONT, previous: HGDIOBJ }
        impl SelectedFont {
            fn release(&mut self) -> bool {
                unsafe {
                    let restored = if self.dc.0.is_null() || self.previous.0.is_null() { true }
                        else { let selected = SelectObject(self.dc, self.previous); !selected.0.is_null() && selected.0 as isize != -1 };
                    let dc_deleted = self.dc.0.is_null() || DeleteDC(self.dc).as_bool();
                    if dc_deleted { self.dc = HDC::default(); }
                    // Never try deleting a font which may still be selected in a failed DC.
                    let font_deleted = (restored || dc_deleted) && (self.font.0.is_null() || DeleteObject(self.font.into()).as_bool());
                    if font_deleted { self.font = HFONT::default(); }
                    restored && dc_deleted && font_deleted
                }
            }
        }
        impl Drop for SelectedFont { fn drop(&mut self) { let _ = self.release(); } }
        let text: Vec<u16> = "Google installs into Chrome. Bowser uses a separate verified package and permission review. Extension compatibility varies.".encode_utf16().collect();
        assert!(!text.is_empty());
        let mut previous_dpi_height = 0;
        for dpi in [96_u32, 144, 192] {
            let mut previous_scale_height = 0;
            for scale in [100_u32, 150, 200] {
            let mut resources = SelectedFont { dc: unsafe { CreateCompatibleDC(None) },
                font: super::super::paint::create_message_font_with(dpi, ANTIALIASED_QUALITY, scale, None),
                previous: HGDIOBJ::default() };
            assert!(!resources.dc.0.is_null(), "native measurement DC unavailable");
            assert!(!resources.font.0.is_null(), "actual Bowser message font unavailable");
            resources.previous = unsafe { SelectObject(resources.dc, resources.font.into()) };
            assert!(!resources.previous.0.is_null() && resources.previous.0 as isize != -1, "actual message font selection failed");
            let mut metrics = TEXTMETRICW::default();
            assert!(unsafe { GetTextMetricsW(resources.dc, &mut metrics) }.as_bool());
            assert!(metrics.tmHeight > previous_scale_height, "realized font height did not grow with requested scale");
            previous_scale_height = metrics.tmHeight;
            if scale == 100 {
                assert!(metrics.tmHeight > previous_dpi_height, "realized baseline font height did not grow with explicit DPI");
                previous_dpi_height = metrics.tmHeight;
            }
            for width_dip in [260_i32, 640, 960] {
                let width = width_dip * dpi as i32 / 96;
                let mut measured = RECT { left: 0, top: 0, right: width, bottom: 0 };
                let returned = unsafe { measure_text(resources.dc, text.as_ptr(), text.len() as i32,
                    &mut measured, (DT_CALCRECT | DT_NOPREFIX | DT_WORDBREAK).0) };
                let measured_height = measured.bottom - measured.top;
                assert!(returned > 0 && measured_height >= metrics.tmHeight);
                assert_eq!(returned, measured_height, "native calculated height and rectangle disagree");
                assert!(measured.right > measured.left && measured.right - measured.left <= width);
                let padded_height = measured_height.checked_add(16 * dpi as i32 / 96).unwrap();
                let request = Request { area: Rect::new(0, 0, width, 720 * dpi as i32 / 96),
                    bookmark_height: 28 * dpi as i32 / 96, bookmark_bottom: false,
                    bookmark_reserved: true, measured_guidance: Some(padded_height),
                    minimum_page_height: 96 * dpi as i32 / 96 };
                let rows = partition(request).unwrap();
                let guidance = rows.guidance.expect("complete measured copy must fit representative viewport");
                assert_eq!(guidance.height(), padded_height);
                assert_eq!(rows.page.top, guidance.bottom + request.bookmark_height);
                assert!(rows.page.height() >= request.minimum_page_height);
                println!("STORE_GUIDANCE_FONT_MEASUREMENT dpi={dpi} font_scale={scale} width={width} text_units={} font_line_height={} measured_text_height={measured_height} reserved_height={padded_height} native_control_created=false", text.len(), metrics.tmHeight);
            }
            assert!(resources.release(), "native measurement resources did not release cleanly");
        }}
    }

}
