//! Antialiased shapes for the GDI popups. GDI's own `Ellipse`, `RoundRect`, `Polygon` and `LineTo`
//! are aliased, so circles, rounded corners and icon strokes look jagged at 96–144 DPI. These
//! helpers draw the same shapes through GDI+ with antialiasing. Text stays on GDI so it keeps the
//! hinting and ClearType of the message font.
//!
//! Coordinates and corner sizes follow the GDI calls they replace: a `RECT` is the GDI
//! `left, top, right, bottom` box, a corner `diameter` is the ellipse width/height that `RoundRect`
//! took, and a stroke of `width` pixels stays inside that box as the GDI pen did. If GDI+ cannot
//! start, each helper falls back to the plain GDI call with the same arguments.

use std::{ffi::c_void, sync::OnceLock};

use windows::Win32::{
    Foundation::{COLORREF, POINT, RECT},
    Graphics::Gdi::{
        CreatePen, CreateSolidBrush, DeleteObject, Ellipse, GetStockObject, HDC, HGDIOBJ, LineTo,
        MoveToEx, NULL_BRUSH, NULL_PEN, PS_SOLID, Polygon, Rectangle, RoundRect, SelectObject,
    },
};

use crate::theme::Rgba;

type Status = i32;
const OK: Status = 0;
const SMOOTHING_ANTIALIAS: i32 = 4;
const UNIT_PIXEL: i32 = 2;
const FILL_ALTERNATE: i32 = 0;

#[repr(C)]
#[derive(Clone, Copy)]
struct PointF {
    x: f32,
    y: f32,
}

#[repr(C)]
struct StartupInput {
    version: u32,
    debug_callback: *const c_void,
    suppress_background_thread: i32,
    suppress_external_codecs: i32,
}

#[link(name = "gdiplus")]
unsafe extern "system" {
    fn GdiplusStartup(token: *mut usize, input: *const StartupInput, output: *mut c_void)
    -> Status;
    fn GdipCreateFromHDC(hdc: HDC, graphics: *mut *mut c_void) -> Status;
    fn GdipDeleteGraphics(graphics: *mut c_void) -> Status;
    fn GdipSetSmoothingMode(graphics: *mut c_void, mode: i32) -> Status;
    fn GdipCreateSolidFill(colour: u32, brush: *mut *mut c_void) -> Status;
    fn GdipDeleteBrush(brush: *mut c_void) -> Status;
    fn GdipCreatePen1(colour: u32, width: f32, unit: i32, pen: *mut *mut c_void) -> Status;
    fn GdipDeletePen(pen: *mut c_void) -> Status;
    fn GdipCreatePath(fill_mode: i32, path: *mut *mut c_void) -> Status;
    fn GdipDeletePath(path: *mut c_void) -> Status;
    fn GdipAddPathArc(
        path: *mut c_void,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        start: f32,
        sweep: f32,
    ) -> Status;
    fn GdipAddPathRectangle(path: *mut c_void, x: f32, y: f32, w: f32, h: f32) -> Status;
    fn GdipClosePathFigure(path: *mut c_void) -> Status;
    fn GdipFillPath(graphics: *mut c_void, brush: *mut c_void, path: *mut c_void) -> Status;
    fn GdipDrawPath(graphics: *mut c_void, pen: *mut c_void, path: *mut c_void) -> Status;
    fn GdipFillEllipse(
        graphics: *mut c_void,
        brush: *mut c_void,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> Status;
    fn GdipDrawEllipse(
        graphics: *mut c_void,
        pen: *mut c_void,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> Status;
    fn GdipDrawPolygon(
        graphics: *mut c_void,
        pen: *mut c_void,
        points: *const PointF,
        count: i32,
    ) -> Status;
    fn GdipDrawLine(
        graphics: *mut c_void,
        pen: *mut c_void,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
    ) -> Status;
}

/// GDI+ is started once per process and never shut down; the popups are the only users.
/// `false` means GDI+ could not start, and the helpers use their GDI fallbacks.
fn started() -> bool {
    static TOKEN: OnceLock<Option<usize>> = OnceLock::new();
    TOKEN
        .get_or_init(|| {
            let input = StartupInput {
                version: 1,
                debug_callback: std::ptr::null(),
                suppress_background_thread: 0,
                suppress_external_codecs: 0,
            };
            let mut token = 0usize;
            let status = unsafe { GdiplusStartup(&mut token, &input, std::ptr::null_mut()) };
            (status == OK).then_some(token)
        })
        .is_some()
}

fn argb(colour: Rgba) -> u32 {
    0xFF00_0000 | (u32::from(colour.r) << 16) | (u32::from(colour.g) << 8) | u32::from(colour.b)
}

fn colorref(colour: Rgba) -> COLORREF {
    COLORREF(u32::from(colour.r) | (u32::from(colour.g) << 8) | (u32::from(colour.b) << 16))
}

/// A GDI+ graphics object bound to `dc` for the duration of one drawing call.
struct Canvas(*mut c_void);

impl Canvas {
    unsafe fn new(dc: HDC) -> Option<Self> {
        if !started() {
            return None;
        }
        let mut graphics = std::ptr::null_mut();
        if unsafe { GdipCreateFromHDC(dc, &mut graphics) } != OK {
            return None;
        }
        unsafe { GdipSetSmoothingMode(graphics, SMOOTHING_ANTIALIAS) };
        Some(Self(graphics))
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe { GdipDeleteGraphics(self.0) };
    }
}

/// Owned GDI+ brush, deleted on drop.
struct Brush(*mut c_void);

impl Drop for Brush {
    fn drop(&mut self) {
        unsafe { GdipDeleteBrush(self.0) };
    }
}

/// Owned GDI+ pen, deleted on drop.
struct Pen(*mut c_void);

impl Drop for Pen {
    fn drop(&mut self) {
        unsafe { GdipDeletePen(self.0) };
    }
}

/// Owned GDI+ path, deleted on drop.
struct Path(*mut c_void);

impl Drop for Path {
    fn drop(&mut self) {
        unsafe { GdipDeletePath(self.0) };
    }
}

unsafe fn solid_brush(colour: Rgba) -> Option<Brush> {
    let mut brush = std::ptr::null_mut();
    (unsafe { GdipCreateSolidFill(argb(colour), &mut brush) } == OK).then_some(Brush(brush))
}

unsafe fn pen(colour: Rgba, width: f32) -> Option<Pen> {
    let mut pen = std::ptr::null_mut();
    (unsafe { GdipCreatePen1(argb(colour), width, UNIT_PIXEL, &mut pen) } == OK).then_some(Pen(pen))
}

/// A closed rounded-rectangle outline, or a plain rectangle when the radius is zero.
unsafe fn rounded_path(x: f32, y: f32, w: f32, h: f32, radius: f32) -> Option<Path> {
    let mut path = std::ptr::null_mut();
    if unsafe { GdipCreatePath(FILL_ALTERNATE, &mut path) } != OK {
        return None;
    }
    let path = Path(path);
    let r = radius.clamp(0.0, (w.min(h)) / 2.0);
    unsafe {
        if r <= 0.0 {
            GdipAddPathRectangle(path.0, x, y, w, h);
        } else {
            let d = 2.0 * r;
            GdipAddPathArc(path.0, x, y, d, d, 180.0, 90.0);
            GdipAddPathArc(path.0, x + w - d, y, d, d, 270.0, 90.0);
            GdipAddPathArc(path.0, x + w - d, y + h - d, d, d, 0.0, 90.0);
            GdipAddPathArc(path.0, x, y + h - d, d, d, 90.0, 90.0);
            GdipClosePathFigure(path.0);
        }
    }
    Some(path)
}

/// The GDI box inset by half the stroke, so an antialiased stroke covers the same pixels the GDI
/// pen covered.
fn stroke_box(rect: RECT, width: f32) -> (f32, f32, f32, f32) {
    let half = width / 2.0;
    (
        rect.left as f32 + half,
        rect.top as f32 + half,
        (rect.right - rect.left) as f32 - width,
        (rect.bottom - rect.top) as f32 - width,
    )
}

fn fill_box(rect: RECT) -> (f32, f32, f32, f32) {
    (
        rect.left as f32,
        rect.top as f32,
        (rect.right - rect.left) as f32,
        (rect.bottom - rect.top) as f32,
    )
}

/// Selects a solid brush and pen into `dc` for `draw`, then restores the previous objects and frees
/// the ones created here. `None` means a hollow fill or no stroke, and stock objects are never freed.
unsafe fn with_gdi_objects(
    dc: HDC,
    fill: Option<Rgba>,
    stroke: Option<(Rgba, i32)>,
    draw: impl FnOnce(),
) {
    unsafe {
        let brush: HGDIOBJ = match fill {
            Some(colour) => CreateSolidBrush(colorref(colour)).into(),
            None => GetStockObject(NULL_BRUSH),
        };
        let pen: HGDIOBJ = match stroke {
            Some((colour, width)) => CreatePen(PS_SOLID, width, colorref(colour)).into(),
            None => GetStockObject(NULL_PEN),
        };
        let old_brush = SelectObject(dc, brush);
        let old_pen = SelectObject(dc, pen);
        draw();
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
        if fill.is_some() {
            let _ = DeleteObject(brush);
        }
        if stroke.is_some() {
            let _ = DeleteObject(pen);
        }
    }
}

/// Filled rounded rectangle over the GDI box `rect`. `diameter` is the corner ellipse size, as
/// `RoundRect` took it.
pub unsafe fn fill_round_rect(dc: HDC, rect: RECT, diameter: i32, colour: Rgba) {
    unsafe {
        let Some(canvas) = Canvas::new(dc) else {
            with_gdi_objects(dc, Some(colour), None, || {
                let _ = RoundRect(
                    dc,
                    rect.left,
                    rect.top,
                    rect.right,
                    rect.bottom,
                    diameter,
                    diameter,
                );
            });
            return;
        };
        let Some(brush) = solid_brush(colour) else {
            return;
        };
        let (x, y, w, h) = fill_box(rect);
        let Some(path) = rounded_path(x, y, w, h, diameter as f32 / 2.0) else {
            return;
        };
        GdipFillPath(canvas.0, brush.0, path.0);
    }
}

/// Outlined rounded rectangle over the GDI box `rect`, with a pen of `width` pixels. `diameter` is
/// the corner ellipse size, as `RoundRect` took it. A zero diameter gives a square-cornered box.
pub unsafe fn stroke_round_rect(dc: HDC, rect: RECT, diameter: i32, colour: Rgba, width: i32) {
    unsafe {
        let Some(canvas) = Canvas::new(dc) else {
            with_gdi_objects(dc, None, Some((colour, width)), || {
                if diameter > 0 {
                    let _ = RoundRect(
                        dc,
                        rect.left,
                        rect.top,
                        rect.right,
                        rect.bottom,
                        diameter,
                        diameter,
                    );
                } else {
                    let _ = Rectangle(dc, rect.left, rect.top, rect.right, rect.bottom);
                }
            });
            return;
        };
        let Some(pen) = pen(colour, width as f32) else {
            return;
        };
        let (x, y, w, h) = stroke_box(rect, width as f32);
        // The centre line of the stroke sits half a pen width inside the outer corner.
        let radius = diameter as f32 / 2.0 - width as f32 / 2.0;
        let Some(path) = rounded_path(x, y, w, h, radius) else {
            return;
        };
        GdipDrawPath(canvas.0, pen.0, path.0);
    }
}

/// Filled ellipse inside the GDI box `rect`.
pub unsafe fn fill_ellipse(dc: HDC, rect: RECT, colour: Rgba) {
    unsafe {
        let Some(canvas) = Canvas::new(dc) else {
            with_gdi_objects(dc, Some(colour), None, || {
                let _ = Ellipse(dc, rect.left, rect.top, rect.right, rect.bottom);
            });
            return;
        };
        let Some(brush) = solid_brush(colour) else {
            return;
        };
        let (x, y, w, h) = fill_box(rect);
        GdipFillEllipse(canvas.0, brush.0, x, y, w, h);
    }
}

/// Outlined ellipse inside the GDI box `rect`, with a pen of `width` pixels.
pub unsafe fn stroke_ellipse(dc: HDC, rect: RECT, colour: Rgba, width: i32) {
    unsafe {
        let Some(canvas) = Canvas::new(dc) else {
            with_gdi_objects(dc, None, Some((colour, width)), || {
                let _ = Ellipse(dc, rect.left, rect.top, rect.right, rect.bottom);
            });
            return;
        };
        let Some(pen) = pen(colour, width as f32) else {
            return;
        };
        let (x, y, w, h) = stroke_box(rect, width as f32);
        GdipDrawEllipse(canvas.0, pen.0, x, y, w, h);
    }
}

/// Outlined rectangle inside the GDI box `rect`, with a pen of `width` pixels.
pub unsafe fn stroke_rect(dc: HDC, rect: RECT, colour: Rgba, width: i32) {
    unsafe { stroke_round_rect(dc, rect, 0, colour, width) }
}

/// Outlined polygon through `points`, in device pixels, with a pen of `width` pixels.
pub unsafe fn stroke_polygon(dc: HDC, points: &[POINT], colour: Rgba, width: i32) {
    unsafe {
        let Some(canvas) = Canvas::new(dc) else {
            with_gdi_objects(dc, None, Some((colour, width)), || {
                let _ = Polygon(dc, points);
            });
            return;
        };
        let Some(pen) = pen(colour, width as f32) else {
            return;
        };
        let points = to_pointf(points);
        GdipDrawPolygon(canvas.0, pen.0, points.as_ptr(), points.len() as i32);
    }
}

/// Straight stroke from one pixel centre to another, `width` pixels thick.
pub unsafe fn stroke_line(dc: HDC, from: POINT, to: POINT, colour: Rgba, width: i32) {
    unsafe {
        let Some(canvas) = Canvas::new(dc) else {
            with_gdi_objects(dc, None, Some((colour, width)), || {
                let _ = MoveToEx(dc, from.x, from.y, None);
                let _ = LineTo(dc, to.x, to.y);
            });
            return;
        };
        let Some(pen) = pen(colour, width as f32) else {
            return;
        };
        // A pixel's centre is at +0.5, so an antialiased line lands on the pixels GDI would use.
        GdipDrawLine(
            canvas.0,
            pen.0,
            from.x as f32 + 0.5,
            from.y as f32 + 0.5,
            to.x as f32 + 0.5,
            to.y as f32 + 0.5,
        );
    }
}

fn to_pointf(points: &[POINT]) -> Vec<PointF> {
    points
        .iter()
        .map(|p| PointF {
            x: p.x as f32,
            y: p.y as f32,
        })
        .collect()
}
