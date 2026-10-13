//! Anti-aliased bottom corners for the omnibox dropdown (V12).
//!
//! The dropdown is a WebView2 child in a popup, shaped with a window region. A region has no partial
//! pixels, so its rounded corners stair-step next to the page's smooth curve. Instead the region is
//! cut a hair inside the true curve (only pixels the curve fully covers) and this module lays a
//! click-through layered window over the corner pixels the curve covers only partly, painting the
//! edge colour at that coverage. The result is the same anti-aliased curve the toolbar pill has.
//!
//! The geometry is pure and unit-tested; the layered window is a thin shell around it.

use std::{cell::RefCell, collections::HashMap, mem::size_of, ptr};

use windows::{
    Win32::{
        Foundation::{COLORREF, HWND, POINT, SIZE},
        Graphics::Gdi::{
            AC_SRC_ALPHA, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC,
            CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, HBITMAP, HDC, SelectObject,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_HIDE,
            SetWindowPos, ShowWindow, ULW_ALPHA, UpdateLayeredWindow, WS_EX_LAYERED, WS_EX_NOACTIVATE,
            WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
        },
    },
    core::w,
};

/// Supersampling per axis when measuring how much of a pixel the curve covers.
const SAMPLES: i32 = 8;
const FULL: u8 = 255;

/// Coverage (0 to 255) of every pixel of the bottom-left corner square of side `radius`, row-major,
/// row 0 at the top of the square. The curve is the quarter circle centred on the square's top-right
/// corner, which is what CSS `border-radius` draws. Pure.
pub(crate) fn corner_coverage(radius: i32) -> Vec<u8> {
    let r = radius.max(0);
    let mut out = Vec::with_capacity((r * r) as usize);
    let r2 = f64::from(r) * f64::from(r);
    for y in 0..r {
        for x in 0..r {
            let mut inside = 0;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let px = f64::from(x) + (f64::from(sx) + 0.5) / f64::from(SAMPLES) - f64::from(r);
                    let py = f64::from(y) + (f64::from(sy) + 0.5) / f64::from(SAMPLES);
                    if px * px + py * py <= r2 {
                        inside += 1;
                    }
                }
            }
            out.push((inside * i32::from(FULL) / (SAMPLES * SAMPLES)) as u8);
        }
    }
    out
}

/// For each row of the corner square, how many leading pixels are not fully covered: the region
/// starts that far in. Pure.
pub(crate) fn row_insets(coverage: &[u8], radius: i32) -> Vec<i32> {
    let r = radius.max(0) as usize;
    (0..r)
        .map(|y| coverage[y * r..(y + 1) * r].iter().take_while(|&&c| c < FULL).count() as i32)
        .collect()
}

/// Premultiplied BGRA pixels (top-down, `width` by `radius`) for the strip along the dropdown's
/// bottom: the partly covered pixels of both corners in `colour`, everything else transparent. Pure.
pub(crate) fn fringe_pixels(width: i32, radius: i32, coverage: &[u8], colour: (u8, u8, u8)) -> Vec<u32> {
    let (w, r) = (width.max(0) as usize, radius.max(0) as usize);
    let mut pixels = vec![0u32; w * r];
    if w < 2 * r {
        return pixels;
    }
    let pre = |channel: u8, alpha: u8| (u32::from(channel) * u32::from(alpha) + 127) / 255;
    for y in 0..r {
        for x in 0..r {
            let c = coverage[y * r + x];
            if c == 0 || c == FULL {
                continue;
            }
            let value = (u32::from(c) << 24) | (pre(colour.0, c) << 16) | (pre(colour.1, c) << 8) | pre(colour.2, c);
            pixels[y * w + x] = value;
            pixels[y * w + (w - 1 - x)] = value;
        }
    }
    pixels
}

struct Fringe {
    window: HWND,
    dc: HDC,
    bitmap: HBITMAP,
    /// What the bitmap currently holds: (width, radius, colour).
    painted: Option<(i32, i32, (u8, u8, u8))>,
    shown: bool,
}

thread_local! {
    static FRINGES: RefCell<HashMap<isize, Fringe>> = RefCell::new(HashMap::new());
    static COVERAGE: RefCell<(i32, Vec<u8>)> = const { RefCell::new((0, Vec::new())) };
}

fn coverage_for(radius: i32) -> Vec<u8> {
    COVERAGE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.0 != radius || cache.1.is_empty() {
            *cache = (radius, corner_coverage(radius));
        }
        cache.1.clone()
    })
}

/// Region insets for a dropdown whose bottom corners have `radius`, for `suggest_region`.
pub(crate) fn insets_for(radius: i32) -> Vec<i32> {
    row_insets(&coverage_for(radius), radius)
}

/// Shows the fringe for the dropdown whose visible bottom edge is at screen y `bottom`, starting at
/// screen x `left` and `width` wide, just above `after` in z-order.
pub(crate) fn show(owner: HWND, after: HWND, left: i32, bottom: i32, width: i32, radius: i32, colour: (u8, u8, u8)) {
    if radius < 2 || width < 2 * radius {
        hide(owner);
        return;
    }
    FRINGES.with(|all| {
        let mut all = all.borrow_mut();
        let key = owner.0 as isize;
        if !all.contains_key(&key) {
            let Some(fringe) = create(owner) else { return };
            all.insert(key, fringe);
        }
        let Some(fringe) = all.get_mut(&key) else { return };
        unsafe {
            if fringe.painted != Some((width, radius, colour)) {
                repaint(fringe, width, radius, colour);
            }
            let origin = POINT { x: left, y: bottom - radius };
            let size = SIZE { cx: width, cy: radius };
            let source = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
            let _ = UpdateLayeredWindow(fringe.window, None, Some(&origin), Some(&size), Some(fringe.dc), Some(&source), COLORREF(0), Some(&blend), ULW_ALPHA);
            let _ = SetWindowPos(fringe.window, Some(after), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
            fringe.shown = true;
        }
    });
}

pub(crate) fn hide(owner: HWND) {
    FRINGES.with(|all| {
        if let Some(fringe) = all.borrow_mut().get_mut(&(owner.0 as isize))
            && fringe.shown
        {
            unsafe {
                let _ = ShowWindow(fringe.window, SW_HIDE);
            }
            fringe.shown = false;
        }
    });
}

pub(crate) fn destroy(owner: HWND) {
    if let Some(fringe) = FRINGES.with(|all| all.borrow_mut().remove(&(owner.0 as isize))) {
        unsafe {
            let _ = DestroyWindow(fringe.window);
            release(&fringe);
        }
    }
}

fn create(owner: HWND) -> Option<Fringe> {
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let window = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            w!("STATIC"),
            w!(""),
            WS_POPUP,
            0,
            0,
            1,
            1,
            Some(owner),
            None,
            Some(instance.into()),
            None,
        )
        .ok()?;
        Some(Fringe { window, dc: HDC::default(), bitmap: HBITMAP::default(), painted: None, shown: false })
    }
}

unsafe fn release(fringe: &Fringe) {
    unsafe {
        if !fringe.dc.0.is_null() {
            let _ = DeleteDC(fringe.dc);
        }
        if !fringe.bitmap.0.is_null() {
            let _ = DeleteObject(fringe.bitmap.into());
        }
    }
}

unsafe fn repaint(fringe: &mut Fringe, width: i32, radius: i32, colour: (u8, u8, u8)) {
    unsafe {
        release(fringe);
        fringe.dc = HDC::default();
        fringe.bitmap = HBITMAP::default();
        fringe.painted = None;
        let dc = CreateCompatibleDC(None);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -radius,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            bmiColors: [Default::default()],
        };
        let mut bits = ptr::null_mut();
        let Ok(bitmap) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
            let _ = DeleteDC(dc);
            return;
        };
        if bits.is_null() {
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(dc);
            return;
        }
        let pixels = fringe_pixels(width, radius, &coverage_for(radius), colour);
        ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u32>(), pixels.len());
        let _ = SelectObject(dc, bitmap.into());
        fringe.dc = dc;
        fringe.bitmap = bitmap;
        fringe.painted = Some((width, radius, colour));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_is_empty_at_the_outer_corner_and_full_inside() {
        let r = 18;
        let c = corner_coverage(r);
        assert_eq!(c.len(), (r * r) as usize);
        // Top row: the side runs straight, so every pixel is inside.
        assert!(c[..r as usize].iter().all(|&v| v == FULL));
        // Bottom-left pixel is far outside the curve.
        assert_eq!(c[((r - 1) * r) as usize], 0);
        // Bottom-right pixel sits at the foot of the curve, on the bottom edge: mostly covered.
        assert!(c[(r * r - 1) as usize] > 128);
        // Partial pixels exist along the arc.
        assert!(c.iter().any(|&v| v > 0 && v < FULL));
    }

    #[test]
    fn insets_grow_toward_the_bottom_and_are_zero_on_the_straight_side() {
        let r = 18;
        let insets = row_insets(&corner_coverage(r), r);
        assert_eq!(insets.len(), r as usize);
        assert_eq!(insets[0], 0);
        assert!(insets.windows(2).all(|pair| pair[1] >= pair[0]));
        assert!(insets[(r - 1) as usize] >= r / 2);
    }

    #[test]
    fn fringe_mirrors_both_corners_and_leaves_the_middle_clear() {
        let (w, r) = (60, 12);
        let c = corner_coverage(r);
        let pixels = fringe_pixels(w, r, &c, (200, 100, 50));
        assert_eq!(pixels.len(), (w * r) as usize);
        for y in 0..r as usize {
            for x in 0..r as usize {
                assert_eq!(pixels[y * w as usize + x], pixels[y * w as usize + (w as usize - 1 - x)]);
            }
            // Middle of every row is untouched.
            assert_eq!(pixels[y * w as usize + w as usize / 2], 0);
        }
        // Premultiplied: no channel exceeds its alpha.
        for &p in pixels.iter().filter(|&&p| p != 0) {
            let a = p >> 24;
            assert!(((p >> 16) & 0xFF) <= a && ((p >> 8) & 0xFF) <= a && (p & 0xFF) <= a);
        }
        // Only partly covered pixels are drawn: fully covered ones belong to the window region.
        assert!(pixels.iter().all(|&p| p == 0 || (p >> 24) < u32::from(FULL)));
    }

    #[test]
    fn a_strip_narrower_than_two_radii_draws_nothing() {
        let r = 12;
        assert!(fringe_pixels(2 * r - 1, r, &corner_coverage(r), (1, 2, 3)).iter().all(|&p| p == 0));
    }
}
