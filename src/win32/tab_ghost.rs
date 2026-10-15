//! The native ghost of a held tab (docs/design/tab-drag.md).
//!
//! The HTML tab strip (and the vertical sidebar) is a WebView2 exactly as big as its row, so a tab
//! dragged off it cannot be drawn by the page. While a tab is held this module shows one
//! click-through, no-activate, topmost layered window per browser window with a per-pixel-alpha
//! picture of the tab: an opaque island with the tab's favicon, title and close glyph, scaled up a
//! little (the lift) and dropped on an elevated shadow. It follows the system cursor in screen
//! coordinates on a 16 ms timer, so it is right on every monitor and whoever holds capture.
//!
//! The page drives it (`tabdrag` start / move / end) and hides its own copy of the tab while the
//! ghost is up (`tabghost` shown); on release the ghost glides into the slot (or fades on a split
//! drop), says `land` so the page shows its copy, and goes a frame later. A watchdog takes a held
//! ghost down (`gone`) when the page falls silent, the mouse button is up with no release heard, or
//! the window is no longer the active, visible one. Private tabs' ghosts are excluded from screen
//! capture where Windows supports it; the ghost is a live window only and is never saved anywhere.

use std::{cell::RefCell, collections::HashMap, mem::size_of, ptr, rc::Rc, time::Instant};

use windows::{
    Win32::{
        Foundation::{COLORREF, HWND, POINT, RECT, SIZE},
        Graphics::Gdi::{
            AC_SRC_ALPHA, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CLEARTYPE_QUALITY, CreateCompatibleDC,
            CreateDIBSection, DIB_RGB_COLORS, DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DeleteDC,
            DeleteObject, DrawTextW, GdiFlush, HBITMAP, HDC, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON},
            WindowsAndMessaging::{
                CreateWindowExW, GA_ROOTOWNER, GetAncestor, GetCursorPos, GetForegroundWindow, GetSystemMetrics,
                HWND_TOPMOST, IsIconic, IsWindow, IsWindowVisible, KillTimer, SM_SWAPBUTTON, SW_HIDE,
                SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetTimer, SetWindowDisplayAffinity,
                SetWindowPos, ShowWindow, ULW_ALPHA, UpdateLayeredWindow, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
                WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
            },
        },
    },
    core::w,
};

use crate::chrome_host::tab_drag::{
    body_origin, body_size, dpi_scale, ease_out, ghost_json, layout, lerp_point, slot_origin, watchdog,
    window_rect, GhostEnd, GhostKind, GhostShadow, GhostSignal, TabDragCommand, Watch, FADE_MS, FLOAT_ALPHA, GLIDE_MS,
    LAND_HOLD_MS, LIFTED_SHADOW, LIFT_SCALE, REST_SHADOW,
};
use crate::chrome_host::protocol::SplitCommand;
use crate::favicon::FaviconBitmap;
use crate::theme::Rgba;

/// The ghost's frame timer (its own window, so no id clash with the browser window's timers).
const TIMER_ID: usize = 0x6A57;
const FRAME_MS: u32 = 16;

/// What the window knows about the held tab (`window::tab_ghost_source`).
pub(super) struct Source {
    pub title: String,
    pub favicon: Option<Rc<FaviconBitmap>>,
    pub private: bool,
    pub pinned: bool,
    /// The browser window's DPI (the ghost keeps it on every monitor, like the tab it came from).
    pub dpi: u32,
    /// The window colour (alpha ignored: the ghost is opaque) and the active tab's text colour.
    pub window: Rgba,
    pub text: Rgba,
    pub dark: bool,
}

/// Everything the ghost draws.
struct Look {
    source: Source,
    kind: GhostKind,
    w: u32,
    h: u32,
    close: bool,
}

// ----- colours and pixels (pure) -----

/// Resolved colours of a ghost, non-premultiplied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Colours {
    pub fill: (u8, u8, u8),
    /// Hairline edge: colour and its strength (0 to 1) over the fill.
    pub edge: ((u8, u8, u8), f32),
    pub text: (u8, u8, u8),
    /// Close glyph and the globe placeholder, already mixed over the fill.
    pub glyph: (u8, u8, u8),
    /// The private violet bar, when the tab is private.
    pub private: Option<(u8, u8, u8)>,
}

fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round().clamp(0.0, 255.0) as u8;
    (l(a.0, b.0), l(a.1, b.1), l(a.2, b.2))
}

/// The lifted tab's colours, from the tab tokens (tokens.css): the active island (`--bw-island-active`:
/// text 24% over the window in dark, white 92% in light) made opaque over the window colour, its
/// `--bw-pill-edge` hairline (text 22%), or the private violet edge (75%) and bar.
pub(crate) fn colours(window: Rgba, text: Rgba, dark: bool, private: bool) -> Colours {
    let base = (window.r, window.g, window.b);
    let ink = (text.r, text.g, text.b);
    let fill = if dark { mix(base, ink, 0.24) } else { mix(base, (255, 255, 255), 0.92) };
    let violet = if dark { (0xB9, 0xA2, 0xFF) } else { (0x64, 0x40, 0xD0) };
    Colours {
        fill,
        edge: if private { (violet, 0.75) } else { (ink, 0.22) },
        text: ink,
        glyph: mix(fill, ink, 0.9),
        private: private.then_some(violet),
    }
}

/// Signed distance from (`x`, `y`) to a `w` x `h` box at the origin with corner radius `r`
/// (negative inside).
fn rounded_distance(x: f32, y: f32, w: f32, h: f32, r: f32) -> f32 {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let qx = (x - w / 2.0).abs() - (w / 2.0 - r);
    let qy = (y - h / 2.0).abs() - (h / 2.0 - r);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - r
}

/// Complementary error function (Abramowitz and Stegun 7.1.26).
fn erfc(x: f32) -> f32 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * z);
    let poly = t * (0.254_829_6 + t * (-0.284_496_74 + t * (1.421_413_7 + t * (-1.453_152_1 + t * 1.061_405_4))));
    let tail = poly * (-z * z).exp();
    if x >= 0.0 { tail } else { 2.0 - tail }
}

fn blurred(d: f32, sigma: f32) -> f32 {
    0.5 * erfc(d / (sigma.max(0.01) * std::f32::consts::SQRT_2))
}

/// Distance from (`px`, `py`) to the segment (`a`, `b`).
fn segment_distance(px: f32, py: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx * dx + dy * dy;
    let t = if len > 0.0 { (((px - a.0) * dx + (py - a.1) * dy) / len).clamp(0.0, 1.0) } else { 0.0 };
    let (cx, cy) = (a.0 + dx * t, a.1 + dy * t);
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

fn pack(alpha: u32, rgb: (u32, u32, u32)) -> u32 {
    (alpha << 24) | (rgb.0 << 16) | (rgb.1 << 8) | rgb.2
}

fn unpack(px: u32) -> (u32, u32, u32, u32) {
    (px >> 24, (px >> 16) & 0xFF, (px >> 8) & 0xFF, px & 0xFF)
}

/// Blends an opaque `colour` at `coverage` over an opaque pixel.
fn over_opaque(px: &mut u32, colour: (u8, u8, u8), coverage: f32) {
    if coverage <= 0.0 {
        return;
    }
    let (a, r, g, b) = unpack(*px);
    let c = coverage.min(1.0);
    let l = |dst: u32, src: u8| (dst as f32 + (f32::from(src) - dst as f32) * c).round() as u32;
    *px = pack(a, (l(r, colour.0), l(g, colour.1), l(b, colour.2)));
}

/// What `paint_body` draws, in device pixels of the bitmap.
pub(crate) struct BodySpec<'a> {
    /// The body's box inside the bitmap: left, top, width, height.
    pub body: (i32, i32, i32, i32),
    pub kind: GhostKind,
    pub radius: f32,
    pub border: f32,
    pub colours: Colours,
    pub shadow: GhostShadow,
    /// Favicon box (left, top, side) relative to the body, and the decoded favicon (premultiplied BGRA).
    pub icon: (i32, i32, i32),
    pub favicon: Option<&'a FaviconBitmap>,
    /// Close glyph: centre (relative to the body) and half span.
    pub close: Option<(f32, f32, f32)>,
    /// Device pixels per CSS pixel (for stroke widths).
    pub scale: f32,
}

/// Paints the ghost into `pixels` (top-down premultiplied BGRA, `width` x `height`): shadow, the
/// rounded island with its hairline (and private bar), favicon or globe, close glyph. The title is
/// drawn afterwards with GDI. Pure.
pub(crate) fn paint_body(pixels: &mut [u32], width: i32, height: i32, spec: &BodySpec<'_>) {
    let (bx, by, bw, bh) = spec.body;
    let (fw, fh) = (bw as f32, bh as f32);
    let fill = spec.colours.fill;
    let (edge, edge_alpha) = spec.colours.edge;
    let bar = (2.0 * spec.scale).max(1.0);
    for y in 0..height {
        for x in 0..width {
            let (lx, ly) = ((x - bx) as f32 + 0.5, (y - by) as f32 + 0.5);
            let d = rounded_distance(lx, ly, fw, fh, spec.radius);
            let cover = (0.5 - d).clamp(0.0, 1.0);
            let s = spec.shadow;
            let ambient = s.ambient_alpha * blurred(d, s.ambient_sigma);
            let key = s.key_alpha * blurred(rounded_distance(lx, ly - s.key_drop, fw, fh, spec.radius), s.key_sigma);
            let shade = 1.0 - (1.0 - ambient.clamp(0.0, 1.0)) * (1.0 - key.clamp(0.0, 1.0));
            let alpha = cover + shade * (1.0 - cover);
            let index = (y * width + x) as usize;
            if cover <= 0.0 {
                pixels[index] = pack((alpha * 255.0).round().clamp(0.0, 255.0) as u32, (0, 0, 0));
                continue;
            }
            // The hairline: full strength in the outer `border` pixels of the shape.
            let inner = -d;
            let line = (spec.border + 0.5 - inner).clamp(0.0, 1.0) * edge_alpha;
            let mut colour = mix(fill, edge, line);
            if let Some(violet) = spec.colours.private {
                let on_bar = match spec.kind {
                    GhostKind::Row => lx < bar,
                    GhostKind::Tab | GhostKind::Tile => ly > fh - bar,
                };
                if on_bar {
                    colour = violet;
                }
            }
            let c = cover;
            let pre = |v: u8| (f32::from(v) * c).round() as u32;
            pixels[index] = pack((alpha * 255.0).round().clamp(0.0, 255.0) as u32, (pre(colour.0), pre(colour.1), pre(colour.2)));
        }
    }
    // Favicon (premultiplied source over the opaque body), centred in its box; or the globe glyph.
    let (ix, iy, side) = spec.icon;
    let (ix, iy) = (bx + ix, by + iy);
    match spec.favicon {
        Some(image) if image.width > 0 && image.height > 0 => {
            let (iw, ih) = (image.width as i32, image.height as i32);
            let (ox, oy) = (ix + (side - iw) / 2, iy + (side - ih) / 2);
            for sy in 0..ih {
                for sx in 0..iw {
                    let (x, y) = (ox + sx, oy + sy);
                    if x < 0 || y < 0 || x >= width || y >= height {
                        continue;
                    }
                    let at = ((sy * iw + sx) * 4) as usize;
                    let Some(src) = image.bgra_premultiplied.get(at..at + 4) else { continue };
                    let sa = u32::from(src[3]);
                    if sa == 0 {
                        continue;
                    }
                    let index = (y * width + x) as usize;
                    let (da, dr, dg, db) = unpack(pixels[index]);
                    let keep = 255 - sa;
                    let blend = |s: u8, d: u32| (u32::from(s) + (d * keep + 127) / 255).min(255);
                    pixels[index] = pack(da.max(sa), (blend(src[2], dr), blend(src[1], dg), blend(src[0], db)));
                }
            }
        }
        _ => {
            // The family's globe, quiet: a ring, an equator and a meridian ellipse.
            let colour = spec.colours.private.unwrap_or(spec.colours.glyph);
            let (cx, cy) = (ix as f32 + side as f32 / 2.0, iy as f32 + side as f32 / 2.0);
            let r = side as f32 * 0.42;
            let stroke = (1.25 * spec.scale).max(1.0);
            for y in iy.max(0)..(iy + side).min(height) {
                for x in ix.max(0)..(ix + side).min(width) {
                    let (px, py) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                    let ring = ((px * px + py * py).sqrt() - r).abs();
                    let equator = if px.abs() <= r { py.abs() } else { f32::MAX };
                    let meridian = (((px / (r * 0.45)).powi(2) + (py / r).powi(2)).sqrt() - 1.0).abs() * r * 0.45;
                    let d = ring.min(equator).min(meridian);
                    over_opaque(&mut pixels[(y * width + x) as usize], colour, (stroke / 2.0 + 0.5 - d).clamp(0.0, 1.0));
                }
            }
        }
    }
    // Close glyph: an X, 1.25 CSS px strokes.
    if let Some((cx, cy, half)) = spec.close {
        let (cx, cy) = (bx as f32 + cx, by as f32 + cy);
        let stroke = (1.25 * spec.scale).max(1.0);
        let reach = half.ceil() as i32 + 2;
        for y in (cy as i32 - reach).max(0)..(cy as i32 + reach).min(height) {
            for x in (cx as i32 - reach).max(0)..(cx as i32 + reach).min(width) {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let d = segment_distance(px, py, (cx - half, cy - half), (cx + half, cy + half))
                    .min(segment_distance(px, py, (cx - half, cy + half), (cx + half, cy - half)));
                over_opaque(&mut pixels[(y * width + x) as usize], spec.colours.glyph, (stroke / 2.0 + 0.5 - d).clamp(0.0, 1.0));
            }
        }
    }
}

// ----- the bitmap -----

struct Canvas {
    dc: HDC,
    bitmap: HBITMAP,
    bits: *mut u32,
    capacity: (i32, i32),
}

impl Canvas {
    const fn empty() -> Self {
        Self { dc: HDC(ptr::null_mut()), bitmap: HBITMAP(ptr::null_mut()), bits: ptr::null_mut(), capacity: (0, 0) }
    }

    /// Makes sure the bitmap is at least `width` x `height`.
    fn ensure(&mut self, width: i32, height: i32) -> bool {
        if !self.bits.is_null() && self.capacity.0 >= width && self.capacity.1 >= height {
            return true;
        }
        self.release();
        let (width, height) = (width.max(self.capacity.0).max(1), height.max(self.capacity.1).max(1));
        unsafe {
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
                return false;
            };
            if bits.is_null() {
                let _ = DeleteObject(bitmap.into());
                let _ = DeleteDC(dc);
                return false;
            }
            SelectObject(dc, bitmap.into());
            // Field by field: assigning `*self` would drop (and release) through `Drop` again.
            self.dc = dc;
            self.bitmap = bitmap;
            self.bits = bits.cast();
            self.capacity = (width, height);
        }
        true
    }

    fn release(&mut self) {
        unsafe {
            if !self.dc.0.is_null() {
                let _ = DeleteDC(self.dc);
            }
            if !self.bitmap.0.is_null() {
                let _ = DeleteObject(self.bitmap.into());
            }
        }
        self.dc = HDC(ptr::null_mut());
        self.bitmap = HBITMAP(ptr::null_mut());
        self.bits = ptr::null_mut();
        self.capacity = (0, 0);
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        self.release();
    }
}

/// Draws `look` at `lift` (1 at rest) with `shadow` into the canvas: pixels from `paint_body`, the
/// title with GDI. Returns the bitmap's size and the shadow margins, or `None` if no bitmap.
fn draw(canvas: &mut Canvas, look: &Look, lift: f32, shadow: GhostShadow) -> Option<((i32, i32), (i32, i32))> {
    let scale = dpi_scale(look.source.dpi) * lift;
    let body = body_size(look.w, look.h, scale);
    let shadow = shadow.scaled(dpi_scale(look.source.dpi));
    let margins = shadow.margins();
    let (_, _, width, height) = window_rect((0, 0), body, margins);
    // Room for the largest frame (the lifted one), so the glide never reallocates.
    let lifted = window_rect((0, 0), body_size(look.w, look.h, dpi_scale(look.source.dpi) * LIFT_SCALE), LIFTED_SHADOW.scaled(dpi_scale(look.source.dpi)).margins());
    if !canvas.ensure(width.max(lifted.2), height.max(lifted.3)) {
        return None;
    }
    let stride = canvas.capacity.0;
    let place = layout(look.kind, body, scale, look.source.pinned, look.close);
    let colours = colours(look.source.window, look.source.text, look.source.dark, look.source.private);
    let mut pixels = vec![0u32; (width * height) as usize];
    paint_body(
        &mut pixels,
        width,
        height,
        &BodySpec {
            body: (margins.0, margins.0, body.0, body.1),
            kind: look.kind,
            radius: place.radius,
            border: (1.0 * scale).max(1.0),
            colours,
            shadow,
            icon: place.icon,
            favicon: look.source.favicon.as_deref(),
            close: place.close,
            scale,
        },
    );
    unsafe {
        let bits = std::slice::from_raw_parts_mut(canvas.bits, (stride * canvas.capacity.1) as usize);
        for y in 0..height {
            let row = (y * stride) as usize;
            bits[row..row + width as usize].copy_from_slice(&pixels[(y * width) as usize..((y + 1) * width) as usize]);
        }
        if let Some((left, right)) = place.title
            && !look.source.title.is_empty()
        {
            let dpi = (look.source.dpi as f32 * lift).round().max(1.0) as u32;
            let font = crate::chrome::paint::create_message_font_with(dpi, CLEARTYPE_QUALITY, 100, None);
            let previous = SelectObject(canvas.dc, font.into());
            let (r, g, b) = colours.text;
            let _ = SetTextColor(canvas.dc, COLORREF(u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16)));
            let _ = SetBkMode(canvas.dc, TRANSPARENT);
            let mut rect = RECT { left: margins.0 + left, top: margins.0, right: margins.0 + right, bottom: margins.0 + body.1 };
            let mut text: Vec<u16> = look.source.title.encode_utf16().take(512).collect();
            DrawTextW(canvas.dc, &mut text, &mut rect, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX);
            let _ = GdiFlush();
            SelectObject(canvas.dc, previous);
            let _ = DeleteObject(font.into());
            // GDI leaves alpha 0 where it drew; the title sits inside the opaque body.
            let (top, bottom) = (margins.0.max(0), (margins.0 + body.1).min(height));
            for y in top..bottom {
                for x in (margins.0 + left).max(0)..(margins.0 + right).min(width) {
                    let px = &mut bits[(y * stride + x) as usize];
                    *px |= 0xFF00_0000;
                }
            }
        }
    }
    Some(((width, height), margins))
}

// ----- the window -----

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    /// Following the cursor.
    Held,
    /// Gliding from `from` (body origin) to `to` since `start`.
    Glide { from: (i32, i32), to: (i32, i32), start: Instant },
    /// Fading out where it is since `start`.
    Fade { start: Instant },
    /// `land` was sent; the window goes at `until`.
    Landed { until: Instant },
}

struct Ghost {
    window: HWND,
    canvas: Canvas,
    look: Look,
    id: u32,
    grab: (u32, u32),
    phase: Phase,
    floating: bool,
    /// Body origin (screen px) of the frame on screen, the lift and opacity it was drawn with.
    origin: (i32, i32),
    lift: f32,
    alpha: u8,
    last_message: Instant,
    button_up_since: Option<Instant>,
}

thread_local! {
    static GHOSTS: RefCell<HashMap<isize, Ghost>> = RefCell::new(HashMap::new());
}

fn key(owner: HWND) -> isize {
    owner.0 as isize
}

/// One `tabdrag` command from `owner`'s tab view. Runs on the UI thread.
pub(super) fn command(owner: HWND, command: TabDragCommand) {
    let mut signals = Vec::new();
    match command {
        TabDragCommand::Start { id, kind, w, h, gx, gy, close } => {
            let Some(source) = super::window::tab_ghost_source(owner, id) else {
                send(owner, GhostSignal::Gone, id);
                return;
            };
            // A new lift starts clean: a preview left by a drag whose `end` never arrived goes.
            super::split_ui::queue_from_tabs(owner, SplitCommand::DragEnd { id });
            start(owner, id, Look { source, kind, w, h, close }, (gx, gy), &mut signals);
        }
        TabDragCommand::Move { id, floating } => GHOSTS.with(|all| {
            let mut all = all.borrow_mut();
            let Some(ghost) = all.get_mut(&key(owner)) else { return };
            if ghost.id != id || ghost.phase != Phase::Held {
                return;
            }
            ghost.last_message = Instant::now();
            ghost.floating = floating;
            follow(ghost);
        }),
        TabDragCommand::End { id, end } => GHOSTS.with(|all| {
            let mut all = all.borrow_mut();
            let Some(ghost) = all.get_mut(&key(owner)) else { return };
            if ghost.id != id {
                return;
            }
            release(owner, ghost, end, &mut signals);
        }),
    }
    for (signal, id) in signals {
        send(owner, signal, id);
    }
}

fn send(owner: HWND, signal: GhostSignal, id: u32) {
    super::window::tab_ghost_post(owner, &ghost_json(signal, id));
}

fn start(owner: HWND, id: u32, look: Look, grab: (u32, u32), signals: &mut Vec<(GhostSignal, u32)>) {
    GHOSTS.with(|all| {
        let mut all = all.borrow_mut();
        let entry = key(owner);
        let reuse = all.get(&entry).is_some_and(|ghost| unsafe { IsWindow(Some(ghost.window)).as_bool() });
        if !reuse {
            all.remove(&entry);
            let Some(window) = create(owner) else {
                signals.push((GhostSignal::Gone, id));
                return;
            };
            all.insert(
                entry,
                Ghost {
                    window,
                    canvas: Canvas::empty(),
                    look,
                    id,
                    grab,
                    phase: Phase::Held,
                    floating: false,
                    origin: (0, 0),
                    lift: LIFT_SCALE,
                    alpha: 255,
                    last_message: Instant::now(),
                    button_up_since: None,
                },
            );
        } else if let Some(ghost) = all.get_mut(&entry) {
            // A new lift replaces whatever the last ghost was doing.
            if ghost.id != id && !matches!(ghost.phase, Phase::Landed { .. }) && ghost.phase != Phase::Held {
                signals.push((GhostSignal::Land, ghost.id));
            }
            ghost.look = look;
            ghost.id = id;
            ghost.grab = grab;
            ghost.phase = Phase::Held;
            ghost.floating = false;
            ghost.lift = LIFT_SCALE;
            ghost.alpha = 255;
            ghost.last_message = Instant::now();
            ghost.button_up_since = None;
        }
        let Some(ghost) = all.get_mut(&entry) else { return };
        unsafe {
            // A private tab's ghost stays out of screenshots and recordings where Windows allows it.
            let affinity = if ghost.look.source.private { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE };
            let _ = SetWindowDisplayAffinity(ghost.window, affinity);
        }
        if !repaint(ghost, cursor_origin(ghost)) {
            hide(ghost);
            signals.push((GhostSignal::Gone, id));
            return;
        }
        unsafe {
            let _ = SetWindowPos(ghost.window, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
            SetTimer(Some(ghost.window), TIMER_ID, FRAME_MS, Some(tick));
        }
        signals.push((GhostSignal::Shown, id));
    });
}

/// The body origin that keeps the grabbed point under the cursor at the ghost's lift.
fn cursor_origin(ghost: &Ghost) -> (i32, i32) {
    let mut cursor = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut cursor);
    }
    body_origin((cursor.x, cursor.y), ghost.grab, dpi_scale(ghost.look.source.dpi) * ghost.lift)
}

/// Repaints the ghost at its lift and shows it with its body at `origin`.
fn repaint(ghost: &mut Ghost, origin: (i32, i32)) -> bool {
    let t = ((LIFT_SCALE - ghost.lift) / (LIFT_SCALE - 1.0)).clamp(0.0, 1.0);
    let Some((size, margins)) = draw(&mut ghost.canvas, &ghost.look, ghost.lift, LIFTED_SHADOW.mix(REST_SHADOW, t)) else {
        return false;
    };
    ghost.origin = origin;
    present(ghost, Some(size), margins)
}

/// Moves the ghost to the cursor (and applies the floating opacity).
fn follow(ghost: &mut Ghost) {
    let origin = cursor_origin(ghost);
    ghost.alpha = if ghost.floating { FLOAT_ALPHA } else { 255 };
    ghost.origin = origin;
    let margins = LIFTED_SHADOW.scaled(dpi_scale(ghost.look.source.dpi)).margins();
    present(ghost, None, margins);
}

/// `UpdateLayeredWindow`: new pixels when `size` is given, else just the position and opacity.
fn present(ghost: &Ghost, size: Option<(i32, i32)>, margins: (i32, i32)) -> bool {
    let (x, y, _, _) = window_rect(ghost.origin, (0, 0), margins);
    let origin = POINT { x, y };
    let blend = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: ghost.alpha, AlphaFormat: AC_SRC_ALPHA as u8 };
    unsafe {
        match size {
            Some((cx, cy)) => {
                let size = SIZE { cx, cy };
                let source = POINT { x: 0, y: 0 };
                UpdateLayeredWindow(ghost.window, None, Some(&origin), Some(&size), Some(ghost.canvas.dc), Some(&source), COLORREF(0), Some(&blend), ULW_ALPHA).is_ok()
            }
            None => UpdateLayeredWindow(ghost.window, None, Some(&origin), None, None, None, COLORREF(0), Some(&blend), ULW_ALPHA).is_ok(),
        }
    }
}

fn hide(ghost: &mut Ghost) {
    unsafe {
        let _ = KillTimer(Some(ghost.window), TIMER_ID);
        let _ = ShowWindow(ghost.window, SW_HIDE);
    }
    ghost.phase = Phase::Landed { until: Instant::now() };
}

/// The page released the tab: glide into the slot, fade on the page, or land at once.
fn release(owner: HWND, ghost: &mut Ghost, end: GhostEnd, signals: &mut Vec<(GhostSignal, u32)>) {
    let now = Instant::now();
    let animate = super::menu::animations_enabled();
    match (end, ghost.phase) {
        // Released over the page with no drop zone showing: nothing will open, so glide home.
        (GhostEnd::Drop { x, y }, Phase::Held) if animate && !super::split_ui::drop_zone_showing(owner) => {
            release(owner, ghost, GhostEnd::Settle { x, y }, signals);
        }
        (GhostEnd::Settle { x, y }, Phase::Held | Phase::Glide { .. }) if animate => {
            let Some((view, dpi)) = super::window::tabs_host_screen(owner) else {
                land(ghost, signals);
                return;
            };
            let to = slot_origin((view.left, view.top), dpi_scale(dpi), x, y);
            // A second settle (the host re-laid the slot) re-aims the running glide.
            let start = match ghost.phase {
                Phase::Glide { start, .. } => start,
                _ => now,
            };
            ghost.phase = Phase::Glide { from: ghost.origin, to, start };
            ghost.alpha = 255;
        }
        (GhostEnd::Drop { .. }, Phase::Held) if animate => ghost.phase = Phase::Fade { start: now },
        (GhostEnd::Settle { .. }, Phase::Landed { .. } | Phase::Fade { .. }) => {}
        _ => land(ghost, signals),
    }
}

/// Tells the page to show its copy; the ghost covers it for one more frame, then goes.
fn land(ghost: &mut Ghost, signals: &mut Vec<(GhostSignal, u32)>) {
    if matches!(ghost.phase, Phase::Landed { .. }) {
        return;
    }
    signals.push((GhostSignal::Land, ghost.id));
    ghost.phase = Phase::Landed { until: Instant::now() + std::time::Duration::from_millis(u64::from(LAND_HOLD_MS)) };
}

fn ms_since(at: Instant, now: Instant) -> u32 {
    now.saturating_duration_since(at).as_millis().min(u128::from(u32::MAX)) as u32
}

fn primary_button_down() -> bool {
    unsafe {
        let vk = if GetSystemMetrics(SM_SWAPBUTTON) != 0 { VK_RBUTTON } else { VK_LBUTTON };
        (GetAsyncKeyState(i32::from(vk.0)) as u16 & 0x8000) != 0
    }
}

/// Whether `owner` is still the visible, active window (Alt+Tab, minimise and closing end a drag).
fn window_ok(owner: HWND) -> bool {
    unsafe {
        if !IsWindow(Some(owner)).as_bool() || !IsWindowVisible(owner).as_bool() || IsIconic(owner).as_bool() {
            return false;
        }
        let foreground = GetForegroundWindow();
        foreground.0.is_null() || foreground == owner || GetAncestor(foreground, GA_ROOTOWNER) == owner
    }
}

/// The frame timer: follow the cursor and watch the drag while held; step the glide or fade; take
/// the window down after landing.
unsafe extern "system" fn tick(window: HWND, _message: u32, _id: usize, _time: u32) {
    let now = Instant::now();
    let mut signals = Vec::new();
    let mut owner = None;
    GHOSTS.with(|all| {
        let Ok(mut all) = all.try_borrow_mut() else { return };
        let Some((&entry, ghost)) = all.iter_mut().find(|(_, ghost)| ghost.window == window) else {
            unsafe {
                let _ = KillTimer(Some(window), TIMER_ID);
            }
            return;
        };
        let parent = HWND(entry as *mut _);
        owner = Some(parent);
        match ghost.phase {
            Phase::Held => {
                ghost.button_up_since = if primary_button_down() { None } else { ghost.button_up_since.or(Some(now)) };
                let up = ghost.button_up_since.map_or(0, |since| ms_since(since, now));
                if watchdog(ms_since(ghost.last_message, now), up, window_ok(parent)) == Watch::Abort {
                    hide(ghost);
                    signals.push((GhostSignal::Gone, ghost.id));
                    // The page may never send its split `end` (it is the silent party): the drop
                    // zone preview goes with the ghost instead of staying up after the drag.
                    super::split_ui::queue_from_tabs(parent, SplitCommand::DragEnd { id: ghost.id });
                    return;
                }
                follow(ghost);
            }
            Phase::Glide { from, to, start } => {
                let t = ms_since(start, now) as f32 / GLIDE_MS as f32;
                let eased = ease_out(t);
                ghost.lift = LIFT_SCALE + (1.0 - LIFT_SCALE) * eased;
                if !repaint(ghost, lerp_point(from, to, eased)) || t >= 1.0 {
                    land(ghost, &mut signals);
                }
            }
            Phase::Fade { start } => {
                let t = ms_since(start, now) as f32 / FADE_MS as f32;
                ghost.alpha = ((1.0 - t.clamp(0.0, 1.0)) * f32::from(FLOAT_ALPHA)).round() as u8;
                let margins = LIFTED_SHADOW.scaled(dpi_scale(ghost.look.source.dpi)).margins();
                present(ghost, None, margins);
                if t >= 1.0 {
                    land(ghost, &mut signals);
                }
            }
            Phase::Landed { until } => {
                if now >= until {
                    hide(ghost);
                }
            }
        }
    });
    if let Some(owner) = owner {
        for (signal, id) in signals {
            send(owner, signal, id);
        }
    }
}

fn create(owner: HWND) -> Option<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
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
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(kind: GhostKind, title: &str, private: bool, dark: bool, w: u32, h: u32, dpi: u32, favicon: Option<FaviconBitmap>) -> Look {
        let palette = crate::theme::palette(if dark { crate::theme::Appearance::Dark } else { crate::theme::Appearance::Light }, false);
        Look {
            source: Source {
                title: title.to_owned(),
                favicon: favicon.map(Rc::new),
                private,
                pinned: false,
                dpi,
                window: palette.window,
                text: palette.tab_text_active,
                dark,
            },
            kind,
            w,
            h,
            close: kind != GhostKind::Tile,
        }
    }

    fn render(look: &Look, lift: f32, shadow: GhostShadow) -> (Vec<u32>, i32, i32, (i32, i32)) {
        let mut canvas = Canvas::empty();
        let ((w, h), margins) = draw(&mut canvas, look, lift, shadow).expect("bitmap");
        let stride = canvas.capacity.0;
        let bits = unsafe { std::slice::from_raw_parts(canvas.bits, (stride * canvas.capacity.1) as usize) };
        let mut out = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            out.extend_from_slice(&bits[(y * stride) as usize..(y * stride + w) as usize]);
        }
        (out, w, h, margins)
    }

    fn favicon(side: u32) -> FaviconBitmap {
        let mut bgra = Vec::new();
        for y in 0..side {
            for x in 0..side {
                let edge = x == 0 || y == 0 || x == side - 1 || y == side - 1;
                let (b, g, r) = if edge { (0, 0, 0) } else { (0xDD, 0x63, 0x3E) };
                bgra.extend_from_slice(&[b, g, r, 255]);
            }
        }
        FaviconBitmap { width: side, height: side, bgra_premultiplied: bgra }
    }

    #[test]
    fn colours_follow_the_tokens() {
        let dark = colours(Rgba::hex(0x20242C, 0xCC), Rgba::hex(0xFFFFFF, 0xFF), true, false);
        assert_eq!(dark.fill, (86, 89, 95));
        assert_eq!(dark.private, None);
        let light = colours(Rgba::hex(0xF4F6F8, 0xCC), Rgba::hex(0x14181D, 0xFF), false, true);
        assert_eq!(light.fill, (254, 254, 254));
        assert_eq!(light.private, Some((0x64, 0x40, 0xD0)));
        assert_eq!(light.edge.1, 0.75);
    }

    #[test]
    fn body_is_opaque_shadow_is_translucent_black_and_pixels_stay_premultiplied() {
        let (px, w, h, m) = render(&look(GhostKind::Tab, "Inbox (12) - Mail", false, true, 180, 26, 96, None), LIFT_SCALE, LIFTED_SHADOW);
        let at = |x: i32, y: i32| unpack(px[(y * w + x) as usize]);
        // Centre of the body: opaque.
        let body = body_size(180, 26, LIFT_SCALE);
        assert_eq!(at(m.0 + body.0 / 2, m.0 + body.1 / 2).0, 255);
        // Below the body: shadow only (black, partly transparent, more below than above).
        let below = at(m.0 + body.0 / 2, m.0 + body.1 + 6);
        assert!(below.0 > 0 && below.0 < 128, "{below:?}");
        assert_eq!((below.1, below.2, below.3), (0, 0, 0));
        let above = at(m.0 + body.0 / 2, m.0 - 6);
        assert!(above.0 < below.0);
        // Far corners: nothing.
        assert_eq!(at(0, 0).0, 0);
        assert_eq!(at(w - 1, h - 1).0, 0);
        // Premultiplied everywhere.
        for &p in &px {
            let (a, r, g, b) = unpack(p);
            assert!(r <= a && g <= a && b <= a, "{a} {r} {g} {b}");
        }
        // The title was drawn: the title band holds pixels that differ from the fill.
        let place = layout(GhostKind::Tab, body, LIFT_SCALE, false, true);
        let (left, right) = place.title.expect("title");
        let fill = colours(Rgba::hex(0x20242C, 0xCC), Rgba::hex(0xFFFFFF, 0xFF), true, false).fill;
        let inked = (left..right).any(|x| {
            let (_, r, g, b) = at(m.0 + x, m.0 + body.1 / 2);
            (r, g, b) != (u32::from(fill.0), u32::from(fill.1), u32::from(fill.2))
        });
        assert!(inked, "the title is painted");
    }

    #[test]
    fn rounded_corners_are_anti_aliased_and_scale_with_dpi() {
        for dpi in [96, 144, 192] {
            let (px, w, _, m) = render(&look(GhostKind::Tab, "", false, false, 120, 26, dpi, Some(favicon(16 * dpi / 96))), 1.0, REST_SHADOW);
            let body = body_size(120, 26, dpi_scale(dpi));
            // The very corner pixel of the body box is outside the 9px curve: (almost) only shadow.
            let corner = unpack(px[(m.0 * w + m.0) as usize]);
            assert!(corner.0 < 128, "dpi {dpi}: {corner:?}");
            // The middle of the top edge is covered.
            assert_eq!(unpack(px[(m.0 * w + m.0 + body.0 / 2) as usize]).0, 255, "dpi {dpi}");
        }
    }

    #[test]
    fn favicon_and_private_bar_land_where_the_css_puts_them() {
        let (px, w, _, m) = render(&look(GhostKind::Tab, "Private", true, true, 160, 26, 96, Some(favicon(16))), 1.0, REST_SHADOW);
        let at = |x: i32, y: i32| unpack(px[(y * w + x) as usize]);
        // Favicon box at 8,5 (16px): its middle is the favicon colour.
        assert_eq!(at(m.0 + 16, m.0 + 13), (255, 0x3E, 0x63, 0xDD));
        // The 2px violet bar along the bottom (inside the body).
        assert_eq!(at(m.0 + 80, m.0 + 25), (255, 0xB9, 0xA2, 0xFF));
    }

    /// Writes the ghost through the real painter as PNGs (for the design screenshots) when
    /// `BOWSER_GHOST_PNG_DIR` is set; otherwise does nothing.
    #[test]
    fn dump_ghost_pngs_when_asked() {
        let Some(dir) = std::env::var_os("BOWSER_GHOST_PNG_DIR") else { return };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("png dir");
        // Sizes default to a wide strip tab and a sidebar row; the screenshot harness passes the
        // sizes its own tabs have (BOWSER_GHOST_TAB_W, BOWSER_GHOST_ROW_W).
        let width = |name: &str, default: u32| std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default);
        let (tab, row) = (width("BOWSER_GHOST_TAB_W", 132), width("BOWSER_GHOST_ROW_W", 228));
        let shots: [(&str, GhostKind, &str, bool, bool, u32, u32, u32, f32, GhostShadow); 9] = [
            ("ghost-tab-dark", GhostKind::Tab, "Hacker News", false, true, tab, 26, 96, LIFT_SCALE, LIFTED_SHADOW),
            ("ghost-tab-light", GhostKind::Tab, "Hacker News", false, false, tab, 26, 96, LIFT_SCALE, LIFTED_SHADOW),
            ("ghost-tab-dark-rest", GhostKind::Tab, "Hacker News", false, true, tab, 26, 96, 1.0, REST_SHADOW),
            ("ghost-tab-private-dark", GhostKind::Tab, "Private tab", true, true, tab, 26, 96, LIFT_SCALE, LIFTED_SHADOW),
            ("ghost-tab-private-light", GhostKind::Tab, "Private tab", true, false, tab, 26, 96, LIFT_SCALE, LIFTED_SHADOW),
            ("ghost-row-dark", GhostKind::Row, "Hacker News", false, true, row, 26, 96, LIFT_SCALE, LIFTED_SHADOW),
            ("ghost-row-light", GhostKind::Row, "Hacker News", false, false, row, 26, 96, LIFT_SCALE, LIFTED_SHADOW),
            ("ghost-tab-dark-150", GhostKind::Tab, "Hacker News", false, true, 132, 26, 144, LIFT_SCALE, LIFTED_SHADOW),
            ("ghost-tab-dark-200", GhostKind::Tab, "Hacker News", false, true, 132, 26, 192, LIFT_SCALE, LIFTED_SHADOW),
        ];
        for (name, kind, title, private, dark, w, h, dpi, lift, shadow) in shots {
            let icon = (!private).then(|| letter_favicon(16 * dpi / 96));
            let mut ghost = look(kind, title, private, dark, w, h, dpi, icon);
            ghost.close = kind == GhostKind::Row || w >= 123;
            let (px, width, height, margins) = render(&ghost, lift, shadow);
            write_png(&dir.join(format!("{name}.png")), width as u32, height as u32, &px);
            std::fs::write(dir.join(format!("{name}.json")), format!("{{\"w\":{width},\"h\":{height},\"margin\":{},\"below\":{},\"dpi\":{dpi},\"lift\":{lift}}}", margins.0, margins.1)).expect("meta");
        }
    }

    /// An orange rounded tile with a white "H", like the harness's lettered favicons.
    fn letter_favicon(side: u32) -> FaviconBitmap {
        let s = side as f32;
        let mut bgra = Vec::new();
        for y in 0..side {
            for x in 0..side {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let cover = (0.5 - rounded_distance(fx, fy, s, s, s * 0.22)).clamp(0.0, 1.0);
                let u = fx / s;
                let v = fy / s;
                let stroke = ((0.30..0.40).contains(&u) || (0.60..0.70).contains(&u)) && (0.25..0.75).contains(&v)
                    || (0.30..0.70).contains(&u) && (0.46..0.56).contains(&v);
                let (r, g, b) = if stroke { (255.0, 255.0, 255.0) } else { (0xF5 as f32, 0xA5 as f32, 0x24 as f32) };
                let a = cover * 255.0;
                bgra.extend_from_slice(&[(b * cover) as u8, (g * cover) as u8, (r * cover) as u8, a as u8]);
            }
        }
        FaviconBitmap { width: side, height: side, bgra_premultiplied: bgra }
    }

    /// A minimal PNG (stored deflate blocks), un-premultiplying the BGRA pixels.
    fn write_png(path: &std::path::Path, width: u32, height: u32, px: &[u32]) {
        let mut raw = Vec::new();
        for y in 0..height {
            raw.push(0u8);
            for x in 0..width {
                let (a, r, g, b) = unpack(px[(y * width + x) as usize]);
                let un = |c: u32| if a == 0 { 0 } else { ((c * 255 + a / 2) / a).min(255) as u8 };
                raw.extend_from_slice(&[un(r), un(g), un(b), a as u8]);
            }
        }
        let mut z = vec![0x78, 0x01];
        for (i, chunk) in raw.chunks(65_535).enumerate() {
            let last = (i + 1) * 65_535 >= raw.len();
            z.push(u8::from(last));
            let len = chunk.len() as u16;
            z.extend_from_slice(&len.to_le_bytes());
            z.extend_from_slice(&(!len).to_le_bytes());
            z.extend_from_slice(chunk);
        }
        let (mut a, mut b) = (1u32, 0u32);
        for &byte in &raw {
            a = (a + u32::from(byte)) % 65_521;
            b = (b + a) % 65_521;
        }
        z.extend_from_slice(&((b << 16) | a).to_be_bytes());
        let crc = |data: &[u8]| {
            let mut c = 0xFFFF_FFFFu32;
            for &byte in data {
                c ^= u32::from(byte);
                for _ in 0..8 {
                    c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
                }
            }
            !c
        };
        let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let mut chunk = |kind: &[u8; 4], data: &[u8]| {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            let mut body = kind.to_vec();
            body.extend_from_slice(data);
            out.extend_from_slice(&body);
            out.extend_from_slice(&crc(&body).to_be_bytes());
        };
        let mut header = Vec::new();
        header.extend_from_slice(&width.to_be_bytes());
        header.extend_from_slice(&height.to_be_bytes());
        header.extend_from_slice(&[8, 6, 0, 0, 0]);
        chunk(b"IHDR", &header);
        chunk(b"IDAT", &z);
        chunk(b"IEND", &[]);
        std::fs::write(path, out).expect("write png");
    }
}
