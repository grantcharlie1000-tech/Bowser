//! The omnibox dropdown's backdrop: anti-aliased bottom corners (V12) and its soft shadow.
//!
//! The dropdown is a WebView2 child in a popup, shaped with a window region. A region has no partial
//! pixels, so its rounded corners would stair-step next to the page's smooth curve, and a popup
//! clipped to its region cannot draw a shadow outside itself. So the region is cut a hair inside the
//! true curve (only pixels the curve fully covers), and this module lays one click-through layered
//! window under the dropdown, a margin larger than it on the sides and below. It paints:
//!
//! - the partly covered corner pixels in the list's surface colour at that coverage, over the shadow,
//!   so the visible curve is anti-aliased like the toolbar pill's;
//! - the shadow around the list's sides and bottom: two layers (a tight ambient edge and a soft key
//!   shadow dropped below), the same two the focused pill carries in CSS (`--bw-sh-omni`), so the
//!   open surface is lifted off the page by elevation rather than an outline;
//! - the surface colour under the list itself, which the dropdown window covers; it only shows if a
//!   frame of the region and of this window ever land apart, and then it reads as the list.
//!
//! There is no shadow above the list's top edge: that edge is the seam with the pill.
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

/// The dropdown's shadow, in physical pixels: an ambient layer that hugs the edge and a key layer
/// dropped `key_drop` below it. `sigma` is the Gaussian's standard deviation (half a CSS blur
/// radius); `alpha` the layer's black at full cover, 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Shadow {
    pub ambient_sigma: f32,
    pub ambient_alpha: f32,
    pub key_drop: f32,
    pub key_sigma: f32,
    pub key_alpha: f32,
}

impl Shadow {
    /// No shadow at all: the backdrop is then the corner fringe alone.
    #[cfg(test)]
    pub(crate) const NONE: Shadow = Shadow { ambient_sigma: 1.0, ambient_alpha: 0.0, key_drop: 0.0, key_sigma: 1.0, key_alpha: 0.0 };

    /// Room the shadow needs beside the list (each side) and below it, in whole pixels: three
    /// standard deviations of the wider layer, plus the key layer's drop below. Pure.
    pub(crate) fn margins(self) -> (i32, i32) {
        if self.ambient_alpha <= 0.0 && self.key_alpha <= 0.0 {
            return (0, 0);
        }
        let reach = (3.0 * self.ambient_sigma.max(self.key_sigma)).ceil();
        (reach as i32, (reach + self.key_drop.max(0.0)).ceil() as i32)
    }
}

/// Complementary error function, Abramowitz and Stegun 7.1.26 (error below 1.5e-7). Pure.
fn erfc(x: f32) -> f32 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * z);
    let poly = t * (0.254_829_6 + t * (-0.284_496_74 + t * (1.421_413_7 + t * (-1.453_152_1 + t * 1.061_405_4))));
    let tail = poly * (-z * z).exp();
    if x >= 0.0 { tail } else { 2.0 - tail }
}

/// How much of a Gaussian blur of standard deviation `sigma` covers a point `d` pixels outside an
/// edge (negative inside): the blurred half-plane, which is exact along straight edges and close at
/// the corners. Pure.
fn blurred(d: f32, sigma: f32) -> f32 {
    0.5 * erfc(d / (sigma.max(0.01) * std::f32::consts::SQRT_2))
}

/// Signed distance from (`x`, `y`) to the list's shape: the box [0, `w`] x [0, `h`] with its bottom
/// corners rounded by `r` and its top corners square. Negative inside. Pure.
fn shape_distance(x: f32, y: f32, w: f32, h: f32, r: f32) -> f32 {
    if r > 0.0 && y > h - r && (x < r || x > w - r) {
        let cx = if x < r { r } else { w - r };
        let (dx, dy) = (x - cx, y - (h - r));
        return (dx * dx + dy * dy).sqrt() - r;
    }
    let dx = (-x).max(x - w);
    let dy = (-y).max(y - h);
    let (ox, oy) = (dx.max(0.0), dy.max(0.0));
    if ox > 0.0 || oy > 0.0 { (ox * ox + oy * oy).sqrt() } else { dx.max(dy) }
}

/// Shadow opacity (0 to 1) at pixel centre (`x`, `y`), in the list's own coordinates. Pure.
fn shadow_alpha(x: f32, y: f32, w: f32, h: f32, r: f32, shadow: Shadow) -> f32 {
    let ambient = shadow.ambient_alpha * blurred(shape_distance(x, y, w, h, r), shadow.ambient_sigma);
    let key = shadow.key_alpha * blurred(shape_distance(x, y - shadow.key_drop, w, h, r), shadow.key_sigma);
    1.0 - (1.0 - ambient.clamp(0.0, 1.0)) * (1.0 - key.clamp(0.0, 1.0))
}

/// The backdrop bitmap's size for a list `width` wide with `visible` rows of pixels showing. Pure.
pub(crate) fn backdrop_size(width: i32, visible: i32, shadow: Shadow) -> (i32, i32) {
    let (side, below) = shadow.margins();
    (width.max(0) + 2 * side, visible.max(0) + below)
}

/// Fills `pixels` (premultiplied BGRA, top-down, `backdrop_size` wide and at least that tall) for a
/// list `width` by `visible` whose bottom corners have `radius` (`coverage` is `corner_coverage` for
/// it) in the opaque `surface` colour. The list sits `side` pixels in from the left, at the top. Pure.
pub(crate) fn backdrop_pixels(
    pixels: &mut [u32],
    width: i32,
    visible: i32,
    radius: i32,
    coverage: &[u8],
    surface: (u8, u8, u8),
    shadow: Shadow,
) {
    let (bw, bh) = backdrop_size(width, visible, shadow);
    let (side, _) = shadow.margins();
    let (w, h) = (width.max(0), visible.max(0));
    let r = radius.clamp(0, (w / 2).min(h));
    // A radius cut down to fit a tiny list needs its own table.
    let own;
    let coverage = if coverage.len() == (r * r) as usize {
        coverage
    } else {
        own = corner_coverage(r);
        &own
    };
    let stride = bw.max(0) as usize;
    let pre = |channel: u8, alpha: u32| (u32::from(channel) * alpha + 127) / 255;
    let solid = 0xFF00_0000 | (u32::from(surface.0) << 16) | (u32::from(surface.1) << 8) | u32::from(surface.2);
    let (fw, fh, fr) = (w as f32, h as f32, r as f32);
    for by in 0..bh {
        let row = &mut pixels[by as usize * stride..(by as usize + 1) * stride];
        let y = by as f32 + 0.5;
        for bx in 0..bw {
            let lx = bx - side;
            // Coverage of the list itself: full inside, the corner table in the corner squares.
            let cover = if lx < 0 || lx >= w || by >= h {
                0
            } else if by >= h - r && (lx < r || lx >= w - r) {
                let cx = if lx < r { lx } else { w - 1 - lx };
                coverage[((by - (h - r)) * r + cx) as usize]
            } else {
                FULL
            };
            if cover == FULL {
                row[bx as usize] = solid;
                continue;
            }
            let shade = shadow_alpha(lx as f32 + 0.5, y, fw, fh, fr, shadow);
            let c = f32::from(cover) / 255.0;
            let alpha = ((c + shade * (1.0 - c)) * 255.0).round().clamp(0.0, 255.0) as u32;
            // The shadow is black, so only the surface's share carries colour.
            let share = u32::from(cover);
            row[bx as usize] = (alpha << 24) | (pre(surface.0, share) << 16) | (pre(surface.1, share) << 8) | pre(surface.2, share);
        }
    }
}

/// Where and what the backdrop shows: the list's screen rectangle (`left`, `top`, `width`, `visible`
/// rows of pixels), its current corner `radius`, the full list height `full` (the bitmap is sized
/// for it, so the list unrolling never reallocates), its surface colour and shadow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Backdrop {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub visible: i32,
    pub full: i32,
    pub radius: i32,
    pub surface: (u8, u8, u8),
    pub shadow: Shadow,
}

struct Fringe {
    window: HWND,
    dc: HDC,
    bitmap: HBITMAP,
    bits: *mut u32,
    /// The bitmap's size in pixels.
    capacity: (i32, i32),
    /// What the bitmap currently holds: (width, visible, radius, surface, shadow).
    painted: Option<(i32, i32, i32, (u8, u8, u8), Shadow)>,
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

/// Shows the backdrop for `spec`, just below `after` (the dropdown window) in z-order. Repaints only
/// when what it shows changed; a move alone is one `UpdateLayeredWindow`.
pub(crate) fn show(owner: HWND, after: HWND, spec: Backdrop) {
    let (bw, bh) = backdrop_size(spec.width, spec.visible, spec.shadow);
    if spec.width < 2 || spec.visible <= 0 || bw <= 0 || bh <= 0 {
        hide(owner);
        return;
    }
    let (side, _) = spec.shadow.margins();
    FRINGES.with(|all| {
        let mut all = all.borrow_mut();
        let key = owner.0 as isize;
        if !all.contains_key(&key) {
            let Some(fringe) = create(owner) else { return };
            all.insert(key, fringe);
        }
        let Some(fringe) = all.get_mut(&key) else { return };
        unsafe {
            let wanted = (spec.width, spec.visible, spec.radius, spec.surface, spec.shadow);
            if fringe.painted != Some(wanted) {
                let tall = backdrop_size(spec.width, spec.full.max(spec.visible), spec.shadow).1;
                if fringe.capacity.0 != bw || fringe.capacity.1 < bh {
                    allocate(fringe, bw, tall.max(bh));
                }
                if fringe.bits.is_null() {
                    return;
                }
                let pixels = std::slice::from_raw_parts_mut(fringe.bits, (bw * bh) as usize);
                let r = spec.radius.clamp(0, (spec.width / 2).min(spec.visible));
                backdrop_pixels(pixels, spec.width, spec.visible, r, &coverage_for(r), spec.surface, spec.shadow);
                fringe.painted = Some(wanted);
            }
            let origin = POINT { x: spec.left - side, y: spec.top };
            let size = SIZE { cx: bw, cy: bh };
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
        Some(Fringe {
            window,
            dc: HDC::default(),
            bitmap: HBITMAP::default(),
            bits: ptr::null_mut(),
            capacity: (0, 0),
            painted: None,
            shown: false,
        })
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

/// A fresh `width` x `height` top-down 32-bit DIB selected into the fringe's own DC.
unsafe fn allocate(fringe: &mut Fringe, width: i32, height: i32) {
    unsafe {
        release(fringe);
        fringe.dc = HDC::default();
        fringe.bitmap = HBITMAP::default();
        fringe.bits = ptr::null_mut();
        fringe.capacity = (0, 0);
        fringe.painted = None;
        let dc = CreateCompatibleDC(None);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
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
        let _ = SelectObject(dc, bitmap.into());
        fringe.dc = dc;
        fringe.bitmap = bitmap;
        fringe.bits = bits.cast::<u32>();
        fringe.capacity = (width, height);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHADOW: Shadow = Shadow { ambient_sigma: 1.0, ambient_alpha: 0.24, key_drop: 8.0, key_sigma: 8.0, key_alpha: 0.32 };
    const SURFACE: (u8, u8, u8) = (56, 60, 67);

    fn paint(width: i32, visible: i32, radius: i32, shadow: Shadow) -> (Vec<u32>, i32, i32) {
        let (bw, bh) = backdrop_size(width, visible, shadow);
        let mut pixels = vec![0xDEAD_BEEF; (bw * bh) as usize];
        backdrop_pixels(&mut pixels, width, visible, radius, &corner_coverage(radius), SURFACE, shadow);
        (pixels, bw, bh)
    }

    fn alpha(pixels: &[u32], bw: i32, x: i32, y: i32) -> u32 {
        pixels[(y * bw + x) as usize] >> 24
    }

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
    fn margins_cover_three_sigmas_and_the_drop() {
        assert_eq!(SHADOW.margins(), (24, 32));
        assert_eq!(Shadow::NONE.margins(), (0, 0));
        assert_eq!(backdrop_size(600, 200, SHADOW), (648, 232));
        assert_eq!(backdrop_size(600, 200, Shadow::NONE), (600, 200));
    }

    #[test]
    fn every_pixel_is_written_mirrored_and_premultiplied() {
        let (pixels, bw, bh) = paint(120, 90, 13, SHADOW);
        assert!(pixels.iter().all(|&p| p != 0xDEAD_BEEF), "no stale pixel is left");
        for y in 0..bh {
            for x in 0..bw / 2 {
                assert_eq!(pixels[(y * bw + x) as usize], pixels[(y * bw + bw - 1 - x) as usize], "({x},{y})");
            }
        }
        for &p in &pixels {
            let a = p >> 24;
            assert!(((p >> 16) & 0xFF) <= a && ((p >> 8) & 0xFF) <= a && (p & 0xFF) <= a);
        }
    }

    #[test]
    fn under_the_list_is_the_opaque_surface_and_the_corners_blend_into_the_shadow() {
        let (pixels, bw, _) = paint(120, 90, 13, SHADOW);
        let side = SHADOW.margins().0;
        let solid = 0xFF00_0000 | (56 << 16) | (60 << 8) | 67;
        // The middle of the list and its straight sides are the surface colour, fully opaque.
        assert_eq!(pixels[(40 * bw + side + 60) as usize], solid);
        assert_eq!(pixels[(40 * bw + side) as usize], solid);
        assert_eq!(pixels[(40 * bw + side + 119) as usize], solid);
        // A partly covered corner pixel is the surface at its coverage over the shadow: more opaque
        // than the coverage alone, and darker than the surface.
        let coverage = corner_coverage(13);
        let at = coverage.iter().position(|&v| v > 60 && v < 200).unwrap() as i32;
        let (cx, cy) = (at % 13, at / 13);
        let c = u32::from(coverage[at as usize]);
        let p = pixels[((90 - 13 + cy) * bw + side + cx) as usize];
        assert!(p >> 24 > c);
        assert_eq!((p >> 16) & 0xFF, (56 * c + 127) / 255);
    }

    #[test]
    fn the_shadow_fades_away_from_the_edges_and_is_weak_at_the_seam() {
        let (pixels, bw, bh) = paint(120, 90, 13, SHADOW);
        let side = SHADOW.margins().0;
        // Beside the list, half way down: strongest at the edge, falling off outward, nearly gone at the margin.
        let row: Vec<u32> = (0..side).rev().map(|x| alpha(&pixels, bw, x, 45)).collect();
        assert!(row.windows(2).all(|pair| pair[1] <= pair[0]), "{row:?}");
        assert!(row[0] >= 40 && row[0] <= 120, "{}", row[0]);
        assert!(alpha(&pixels, bw, 0, 45) <= 2);
        // Below the list, at its centre: the key shadow peaks a few pixels down, then fades out.
        let below: Vec<u32> = (90..bh).map(|y| alpha(&pixels, bw, bw / 2, y)).collect();
        assert!(below[0] > 60);
        assert!(*below.last().unwrap() <= 4, "{below:?}");
        assert!(below.windows(2).skip(2).all(|pair| pair[1] <= pair[0]), "{below:?}");
        // At the top, where the list meets the pill, the side shadow is only the ambient edge.
        assert!(alpha(&pixels, bw, side - 1, 0) < alpha(&pixels, bw, side - 1, 45));
        assert!(alpha(&pixels, bw, side - 6, 0) <= 12);
    }

    #[test]
    fn without_a_shadow_only_the_corner_fringe_is_drawn_outside_the_list() {
        let (pixels, bw, bh) = paint(60, 30, 12, Shadow::NONE);
        assert_eq!((bw, bh), (60, 30));
        let coverage = corner_coverage(12);
        for y in 0..12 {
            for x in 0..12 {
                let c = u32::from(coverage[(y * 12 + x) as usize]);
                assert_eq!(alpha(&pixels, bw, x, 18 + y), c, "({x},{y})");
            }
        }
        assert!(pixels[..(18 * bw) as usize].iter().all(|&p| p >> 24 == 255));
    }

    #[test]
    fn a_list_narrower_than_its_corners_or_not_yet_visible_is_safe() {
        let (pixels, _, _) = paint(10, 4, 13, SHADOW);
        assert!(pixels.iter().all(|&p| p != 0xDEAD_BEEF));
        let (empty, bw, bh) = paint(80, 0, 13, SHADOW);
        assert_eq!(empty.len(), (bw * bh) as usize);
        assert!(empty.iter().all(|&p| p >> 24 < 255), "nothing opaque before the list shows");
    }

    #[test]
    fn erfc_matches_known_values() {
        for (x, want) in [(0.0f32, 1.0f32), (0.5, 0.479_500), (1.0, 0.157_299), (2.0, 0.004_678), (-1.0, 1.842_701)] {
            assert!((erfc(x) - want).abs() < 1e-5, "erfc({x}) = {}", erfc(x));
        }
    }
}
