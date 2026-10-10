//! CPU chrome painter. The chrome is drawn into a premultiplied BGRA DIB section and presented in
//! a layered overlay above the acrylic host window. This preserves per-pixel alpha while the host
//! supplies the blurred backdrop.
//!
//! In glass mode the chrome background is left transparent, so the strip, the gaps, the omnibox row
//! and the empty viewport all show the same blur and tint that DWM draws for the whole window.
//! Only the solid fallback paints its own opaque window colour.

use std::{mem::size_of, ptr, slice, time::Instant};

use windows::Win32::{
    Foundation::{COLORREF, HWND, POINT, RECT, SIZE},
    Graphics::Gdi::{
        AC_SRC_ALPHA, ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLACKNESS,
        BLENDFUNCTION, CreateCompatibleDC, CreateDIBSection, CreateFontIndirectW, DIB_RGB_COLORS,
        DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject,
        GetTextExtentPoint32W, GetTextMetricsW, HBITMAP, HDC, HFONT, HGDIOBJ, IntersectClipRect,
        LOGFONTW, PatBlt, SelectClipRgn, SelectObject, SetBkColor, SetBkMode, SetTextColor,
        TEXTMETRICW, TRANSPARENT,
    },
    UI::{
        HiDpi::SystemParametersInfoForDpi,
        WindowsAndMessaging::{
            NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS, ULW_ALPHA, UPDATELAYEREDWINDOWINFO,
            UpdateLayeredWindow, UpdateLayeredWindowIndirect,
        },
    },
};

use crate::{
    chrome::{
        layout::{self, Hit, Layout, Metrics, Rect},
        motion::{Departure, Emphasis},
    },
    favicon::FaviconBitmap,
    theme::{Palette, Rgba},
};

/// Longest omnibox text, in UTF-16 units, that is measured and drawn. A pasted megabyte must not
/// make every repaint and every click O(n log n) in GDI.
const MAX_TEXT_UNITS: usize = 4096;

/// Data needed to paint one tab island. `favicon` is already premultiplied BGRA.
#[derive(Clone, Copy)]
pub struct TabView<'a> {
    pub title: &'a str,
    pub loading: bool,
    pub favicon: Option<&'a FaviconBitmap>,
    pub emphasis: Emphasis,
    pub group_size: Option<usize>,
    pub group_control: bool,
}

/// State of the ghost native EDIT rendered by the chrome painter.
pub struct OmniboxView<'a> {
    pub text: &'a str,
    /// UTF-16 code-unit offsets, matching `EM_SETSEL`.
    pub selection: (usize, usize),
    pub caret_visible: bool,
    pub focused: bool,
    pub completion_suffix: &'a str,
    pub progress: Option<u8>,
}

/// A complete read-only snapshot of the chrome state for one paint.
pub struct ChromeView<'a> {
    pub layout: &'a Layout,
    pub palette: &'a Palette,
    pub tabs: &'a [TabView<'a>],
    pub active: usize,
    pub hover: Hit,
    pub pressed: Hit,
    pub caption_maximised: bool,
    pub can_back: bool,
    pub can_forward: bool,
    pub omnibox: OmniboxView<'a>,
    /// The instant every transition is sampled at. Painting is a pure function of it.
    pub now: Instant,
    /// Islands of tabs that were just closed, fading out where they stood.
    pub departures: &'a [Departure],
}

/// Reusable top-down premultiplied BGRA image. Its height stops at the omnibox row.
pub struct ChromeSurface {
    width: i32,
    height: i32,
    dpi: u32,
    dib: Dib,
    /// The omnibox text being painted or measured. Nothing else may write to it during a paint.
    text_units: Vec<u16>,
    /// Scratch for tab titles, the placeholder and the completion suffix.
    label_units: Vec<u16>,
    text: TextCache,
    /// Pixels the omnibox text is scrolled left so that the caret stays inside the field.
    omnibox_scroll: i32,
    /// Where the caret goes in the focused omnibox, whether or not it is currently drawn. Empty
    /// when the omnibox is not focused.
    caret_rect: Rect,
    last_presented: Option<PresentedSurface>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PresentedSurface {
    width: i32,
    height: i32,
    dpi: u32,
    origin: (i32, i32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Upload {
    Skip,
    Full,
    Dirty(Rect),
}

fn upload_decision(
    previous: Option<PresentedSurface>,
    current: PresentedSurface,
    dirty: Rect,
) -> Upload {
    if dirty.is_empty() {
        Upload::Skip
    } else if previous != Some(current) {
        Upload::Full
    } else {
        Upload::Dirty(dirty)
    }
}

fn last_presented_after_upload(
    previous: Option<PresentedSurface>,
    current: PresentedSurface,
    upload: Upload,
    success: bool,
) -> Option<PresentedSurface> {
    match upload {
        Upload::Skip => previous,
        Upload::Full | Upload::Dirty(_) if success => Some(current),
        Upload::Full | Upload::Dirty(_) => None,
    }
}

impl ChromeSurface {
    /// Creates a surface for a client size. `height` is the client height; only chrome rows are kept.
    pub fn new(width: i32, height: i32, dpi: u32) -> Self {
        let (width, height, dpi) = surface_size(width, height, dpi);
        Self {
            width,
            height,
            dpi,
            dib: Dib::new(width, height),
            text_units: Vec::new(),
            label_units: Vec::new(),
            text: TextCache::new(width, height, dpi),
            omnibox_scroll: 0,
            caret_rect: Rect::default(),
            last_presented: None,
        }
    }

    /// Rebuilds storage only when client dimensions or DPI change.
    pub fn resize(&mut self, width: i32, height: i32, dpi: u32) {
        let (width, height, dpi) = surface_size(width, height, dpi);
        if (self.width, self.height, self.dpi) == (width, height, dpi) {
            return;
        }
        self.width = width;
        self.height = height;
        self.dpi = dpi;
        self.dib.resize(width, height);
        self.text.resize(width, height, dpi);
        self.omnibox_scroll = 0;
    }

    /// Recreates the cached fonts, for when the system message font changes.
    pub fn rebuild_fonts(&mut self) {
        self.text.rebuild_fonts();
    }

    /// The caret rectangle from the last omnibox paint. A caret blink only needs this repainted.
    pub fn caret_rect(&self) -> Rect {
        self.caret_rect
    }

    pub fn pixels(&self) -> &[u8] {
        self.dib.pixels()
    }
    pub fn stride(&self) -> usize {
        self.dib.stride()
    }
    pub fn width(&self) -> i32 {
        self.width
    }
    pub fn height(&self) -> i32 {
        self.height
    }

    /// Renders only pixels intersecting `dirty`; the caller owns invalidation and scheduling.
    pub fn paint(&mut self, view: &ChromeView<'_>, dirty: Rect) {
        let bounds = Rect::new(0, 0, self.width, self.height);
        let dirty = dirty.intersect(bounds);
        if dirty.is_empty() || self.dib.bits.is_null() {
            return;
        }
        let stride = self.stride();
        let pixels = self.dib.pixels_mut();
        for y in dirty.top..dirty.bottom {
            let start = y as usize * stride + dirty.left as usize * 4;
            pixels[start..start + dirty.width() as usize * 4].fill(0);
        }
        if !view.palette.translucent {
            self.fill_rect(bounds, view.palette.window, dirty);
        }
        self.paint_departures(view, dirty);
        self.paint_tabs(view, dirty);
        self.paint_tab_menu(view, dirty);
        self.paint_new_tab(view, dirty);
        self.paint_nav(view, dirty);
        self.paint_caption(view, dirty);
        self.paint_omnibox(view, dirty);
    }

    /// Updates the layered chrome surface using its premultiplied per-pixel alpha.
    pub fn present(&mut self, overlay: HWND, origin: POINT, dirty: Rect) {
        if self.width <= 0 || self.height <= 0 || self.dib.bits.is_null() {
            return;
        }
        let dirty = dirty.intersect(Rect::new(0, 0, self.width, self.height));
        let presented = PresentedSurface {
            width: self.width,
            height: self.height,
            dpi: self.dpi,
            origin: (origin.x, origin.y),
        };
        let upload = upload_decision(self.last_presented, presented, dirty);
        if upload == Upload::Skip {
            return;
        }
        unsafe {
            let size = SIZE {
                cx: self.width,
                cy: self.height,
            };
            let source = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION {
                BlendOp: 0,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let dirty_rect = match upload {
                Upload::Dirty(r) => RECT {
                    left: r.left,
                    top: r.top,
                    right: r.right,
                    bottom: r.bottom,
                },
                Upload::Full | Upload::Skip => RECT::default(),
            };
            let info = UPDATELAYEREDWINDOWINFO {
                cbSize: size_of::<UPDATELAYEREDWINDOWINFO>() as u32,
                hdcDst: HDC::default(),
                pptDst: &origin,
                psize: &size,
                hdcSrc: self.dib.dc,
                pptSrc: &source,
                crKey: COLORREF(0),
                pblend: &blend,
                dwFlags: ULW_ALPHA,
                prcDirty: if matches!(upload, Upload::Dirty(_)) {
                    &dirty_rect
                } else {
                    ptr::null()
                },
            };
            let success = UpdateLayeredWindowIndirect(overlay, &info).as_bool()
                || UpdateLayeredWindow(
                    overlay,
                    None,
                    Some(&origin),
                    Some(&size),
                    Some(self.dib.dc),
                    Some(&source),
                    COLORREF(0),
                    Some(&blend),
                    ULW_ALPHA,
                )
                .is_ok();
            self.last_presented =
                last_presented_after_upload(self.last_presented, presented, upload, success);
        }
    }

    /// Returns the pixel width from a text origin to UTF-16 code-unit index `index`.
    pub fn caret_x(&mut self, text: &str, index: usize) -> i32 {
        let length = encode_into(&mut self.text_units, text);
        self.text.measure(&self.text_units[..length], index)
    }

    /// Maps an x offset from the left edge of the omnibox text field to the nearest UTF-16
    /// code-unit boundary, accounting for how far the text is currently scrolled.
    pub fn index_at_x(&mut self, text: &str, x: i32) -> usize {
        let length = encode_into(&mut self.text_units, text);
        let units = &self.text_units[..length];
        let target = x.saturating_add(self.omnibox_scroll);
        nearest_boundary(units, target, |index| self.text.measure(units, index))
    }

    fn fill_rect(&mut self, rect: Rect, colour: Rgba, dirty: Rect) {
        let r = rect
            .intersect(dirty)
            .intersect(Rect::new(0, 0, self.width, self.height));
        let stride = self.stride();
        let pixels = self.dib.pixels_mut();
        for y in r.top..r.bottom {
            for x in r.left..r.right {
                let i = y as usize * stride + x as usize * 4;
                blend_pixel(&mut pixels[i..i + 4], colour, 255);
            }
        }
    }

    fn round(&mut self, rect: Rect, radius: i32, colour: Rgba, dirty: Rect) {
        let (width, height, stride) = (self.width, self.height, self.stride());
        raster_rounded_rect(
            self.dib.pixels_mut(),
            width,
            height,
            stride,
            rect,
            radius as f32,
            colour,
            dirty,
        );
    }

    fn border(&mut self, rect: Rect, radius: i32, thickness: f32, colour: Rgba, dirty: Rect) {
        let (width, height, stride) = (self.width, self.height, self.stride());
        raster_round_border(
            self.dib.pixels_mut(),
            width,
            height,
            stride,
            rect,
            radius as f32,
            thickness,
            colour,
            dirty,
        );
    }

    /// Closed tabs fade out and shrink slightly on the island they left. They take no input.
    fn paint_departures(&mut self, view: &ChromeView<'_>, dirty: Rect) {
        let m = view.layout.metrics;
        let radius = m.island_radius();
        for departure in view.departures {
            let amount = departure.amount(view.now);
            if amount <= 0.0 {
                continue;
            }
            let inset = ((1.0 - amount) * m.px(4) as f32).round() as i32;
            let shown = departure.rect.intersect(view.layout.tab_zone);
            let rect = Rect::new(
                shown.left + inset,
                shown.top + inset,
                shown.right - inset,
                shown.bottom - inset,
            );
            if rect.is_empty() {
                continue;
            }
            self.round(
                rect,
                radius,
                scale_alpha(view.palette.tab_active, amount),
                dirty,
            );
        }
    }

    fn paint_tabs(&mut self, view: &ChromeView<'_>, dirty: Rect) {
        let m = view.layout.metrics;
        for (slot, tab_rect) in view.layout.tabs().enumerate() {
            let index = view.layout.first_visible_tab() + slot;
            let rect = tab_rect.intersect(view.layout.tab_zone);
            if rect.is_empty() || rect.intersect(dirty).is_empty() {
                continue;
            }
            let active = index == view.active;
            let details = view.tabs.get(index);
            let (hover, selected) = details.map_or((0.0, 0.0), |t| t.emphasis.amounts(view.now));
            let fill = mix(
                mix(view.palette.tab_inactive, view.palette.tab_hover, hover),
                view.palette.tab_active,
                selected,
            );
            let radius = m.island_radius();
            self.round(rect, radius, fill, dirty);
            self.border(
                rect,
                radius,
                m.dpi() as f32 / 96.0,
                view.palette.hairline,
                dirty,
            );
            // Group members share a thin accent underline, kept inside the visible island.
            if details.is_some_and(|tab| tab.group_size.is_some()) {
                let accent = scale_alpha(view.palette.tab_active_border, 0.8);
                let thickness = m.px(1).max(1);
                let inset = m.px(Metrics::ISLAND_RADIUS / 2);
                let line = Rect::new(
                    tab_rect.left + inset,
                    tab_rect.bottom - m.px(2) - thickness,
                    tab_rect.right - inset,
                    tab_rect.bottom - m.px(2),
                )
                .intersect(rect);
                self.fill_rect(line, accent, dirty);
            }
            if selected > 0.0 {
                // Translucent palettes keep only a faint outline on the selected island, so it
                // reads as a soft fill rather than a framed box.
                let stroke = if view.palette.translucent { 0.25 } else { 1.0 };
                let colour = scale_alpha(view.palette.tab_active_border, selected * stroke);
                self.border(rect, radius, m.dpi() as f32 / 96.0, colour, dirty);
            }
            // Islands are independent rounded shapes with a gap between them, so no divider is drawn.
            let parts = view.layout.tab_parts(tab_rect);
            // A part that would be cut off by the tab zone is skipped, so a half-visible close
            // glyph never lands on a neighbouring control.
            // The group control shows a small member-count chip, only for a real group.
            let group_chip =
                details.and_then(|tab| tab.group_control.then_some(tab.group_size).flatten());
            if let (Some(count), true) = (group_chip, rect.contains_rect(parts.favicon)) {
                self.round(
                    parts.favicon,
                    m.px(6),
                    scale_alpha(view.palette.tab_active_border, 0.75),
                    dirty,
                );
                let count = count.to_string();
                self.draw_text(
                    &count,
                    parts.favicon,
                    view.palette.tab_text_active,
                    true,
                    rect,
                    dirty,
                );
            } else if rect.contains_rect(parts.favicon) {
                if let Some(Some(favicon)) = details.map(|value| value.favicon) {
                    self.blit_favicon(parts.favicon, favicon, dirty);
                } else {
                    self.round(
                        parts.favicon,
                        m.px(4),
                        Rgba::hex(if active { 0x6B7785 } else { 0x84909D }, 210),
                        dirty,
                    );
                    let cx = (parts.favicon.left + parts.favicon.right) / 2;
                    let cy = (parts.favicon.top + parts.favicon.bottom) / 2;
                    self.line(cx - 2, cy, cx + 2, cy, 1.3, Rgba::hex(0xFFFFFF, 230), dirty);
                    self.line(cx, cy - 2, cx, cy + 2, 1.3, Rgba::hex(0xFFFFFF, 230), dirty);
                }
            }
            if !parts.title.is_empty() {
                let title = details.map_or("New tab", |value| value.title);
                let color = mix(
                    view.palette.tab_text_inactive,
                    view.palette.tab_text_active,
                    selected,
                );
                self.draw_text(title, parts.title, color, true, rect, dirty);
            }
            if !rect.contains_rect(parts.close) {
                continue;
            }
            // Layout hit testing exposes close hit targets across the full tab; draw each close glyph.
            let close_hover = matches!(view.hover, Hit::TabClose(i) if i == index);
            let close_pressed = matches!(view.pressed, Hit::TabClose(i) if i == index);
            if close_hover || close_pressed {
                self.round(
                    parts.close,
                    m.px(6),
                    if close_pressed {
                        view.palette.tab_active_border
                    } else {
                        view.palette.caption_hover
                    },
                    dirty,
                );
            }
            let c = if close_hover || active {
                view.palette.tab_text_active
            } else {
                view.palette.tab_text_inactive
            };
            let (cx, cy) = center(parts.close);
            let half = m.px(4);
            self.line(cx - half, cy - half, cx + half, cy + half, 1.35, c, dirty);
            self.line(cx - half, cy + half, cx + half, cy - half, 1.35, c, dirty);
        }
    }

    fn paint_new_tab(&mut self, view: &ChromeView<'_>, dirty: Rect) {
        let rect = view.layout.new_tab_button;
        if rect.intersect(dirty).is_empty() {
            return;
        }
        if matches!(view.hover, Hit::NewTab) || matches!(view.pressed, Hit::NewTab) {
            self.round(
                rect,
                view.layout.metrics.island_radius(),
                view.palette.caption_hover,
                dirty,
            );
        }
        let (x, y) = center(rect);
        let c = view.palette.caption_glyph;
        let arm = view.layout.metrics.px(4);
        self.line(x - arm, y, x + arm, y, 1.4, c, dirty);
        self.line(x, y - arm, x, y + arm, 1.4, c, dirty);
    }

    fn paint_tab_menu(&mut self, view: &ChromeView<'_>, dirty: Rect) {
        let rect = view.layout.tab_menu_button;
        if rect.intersect(dirty).is_empty() {
            return;
        }
        let radius = view.layout.metrics.island_radius();
        if view.hover == Hit::TabMenu || view.pressed == Hit::TabMenu {
            self.round(rect, radius, view.palette.caption_hover, dirty);
        }
        self.border(
            rect,
            radius,
            view.layout.metrics.dpi() as f32 / 96.0,
            view.palette.hairline,
            dirty,
        );
        let (x, y) = center(rect);
        let c = view.palette.caption_glyph;
        self.line(x - 4, y - 2, x, y + 2, 1.5, c, dirty);
        self.line(x, y + 2, x + 4, y - 2, 1.5, c, dirty);
    }

    /// Back, forward and reload. Back and forward are dimmed and inert when there is no history.
    fn paint_nav(&mut self, view: &ChromeView<'_>, dirty: Rect) {
        let m = view.layout.metrics;
        let buttons = [
            (Hit::Back, view.layout.back_button, view.can_back),
            (Hit::Forward, view.layout.forward_button, view.can_forward),
            (Hit::Reload, view.layout.reload_button, true),
        ];
        for (hit, rect, enabled) in buttons {
            if rect.intersect(dirty).is_empty() {
                continue;
            }
            let mut color = view.palette.caption_glyph;
            if enabled {
                if view.hover == hit || view.pressed == hit {
                    self.round(rect, m.island_radius(), view.palette.caption_hover, dirty);
                }
            } else {
                color.a = (u32::from(color.a) * 0x55 / 0xFF) as u8;
            }
            let (cx, cy) = center(rect);
            let (cx, cy) = (cx as f32, cy as f32);
            let half = m.px(7) as f32;
            let head = m.px(4) as f32;
            let w = 1.3;
            match hit {
                Hit::Back | Hit::Forward => {
                    // Forward is the mirror image of back.
                    let dir = if hit == Hit::Back { -1.0 } else { 1.0 };
                    let tip = cx + dir * half;
                    let tail = cx - dir * half;
                    self.line_f(tail, cy, tip, cy, w, color, dirty);
                    self.line_f(tip, cy, tip - dir * head, cy - head, w, color, dirty);
                    self.line_f(tip, cy, tip - dir * head, cy + head, w, color, dirty);
                }
                _ => {
                    // A clockwise arc of about 290 degrees, open at the upper right, with an
                    // arrowhead where it ends. Screen y points down, so angles grow clockwise.
                    const SEGMENTS: usize = 12;
                    let radius = half - 1.0;
                    let start = 10.0_f32.to_radians();
                    let sweep = 290.0_f32.to_radians();
                    let point = |step: usize| {
                        let angle = start + sweep * step as f32 / SEGMENTS as f32;
                        (cx + radius * angle.cos(), cy + radius * angle.sin())
                    };
                    let mut previous = point(0);
                    for step in 1..=SEGMENTS {
                        let next = point(step);
                        self.line_f(previous.0, previous.1, next.0, next.1, w, color, dirty);
                        previous = next;
                    }
                    let end = start + sweep;
                    let (tangent, normal) = ((-end.sin(), end.cos()), (end.cos(), end.sin()));
                    let barb = head * 0.9;
                    for side in [-1.0_f32, 1.0] {
                        self.line_f(
                            previous.0,
                            previous.1,
                            previous.0 - tangent.0 * head + normal.0 * barb * side,
                            previous.1 - tangent.1 * head + normal.1 * barb * side,
                            w,
                            color,
                            dirty,
                        );
                    }
                }
            }
        }
    }

    fn paint_caption(&mut self, view: &ChromeView<'_>, dirty: Rect) {
        let m = view.layout.metrics;
        for i in 0..3 {
            let button_hit = match i {
                0 => Hit::CaptionMinimize,
                1 => Hit::CaptionMaximize,
                _ => Hit::CaptionClose,
            };
            let rect = view.layout.caption_button(button_hit);
            if rect.intersect(dirty).is_empty() {
                continue;
            }
            let close = i == 2;
            let hovered = view.hover == button_hit;
            let pressed = view.pressed == button_hit;
            if hovered || pressed {
                self.fill_rect(
                    rect,
                    if close {
                        view.palette.close_hover
                    } else {
                        view.palette.caption_hover
                    },
                    dirty,
                );
            }
            let color = if close && (hovered || pressed) {
                view.palette.close_glyph_hover
            } else {
                view.palette.caption_glyph
            };
            let (cx, cy) = center(rect);
            let (p3, p4, p5) = (m.px(3), m.px(4), m.px(5));
            match i {
                0 => self.line(cx - p5, cy, cx + p5, cy, 1.35, color, dirty),
                1 if view.caption_maximised => {
                    self.outline_rect(
                        Rect::new(cx - p5, cy - p4, cx + p4, cy + p4),
                        1.1,
                        color,
                        dirty,
                    );
                    self.line(cx - p3, cy - p5, cx + p5, cy - p5, 1.0, color, dirty);
                    self.line(cx + p5, cy - p5, cx + p5, cy + p3, 1.0, color, dirty);
                }
                1 => self.outline_rect(
                    Rect::new(cx - p5, cy - p4, cx + p5, cy + p5),
                    1.1,
                    color,
                    dirty,
                ),
                _ => {
                    self.line(cx - p5, cy - p5, cx + p5, cy + p5, 1.45, color, dirty);
                    self.line(cx - p5, cy + p5, cx + p5, cy - p5, 1.45, color, dirty);
                }
            }
        }
    }

    fn paint_omnibox(&mut self, view: &ChromeView<'_>, dirty: Rect) {
        let rect = view.layout.omnibox;
        if rect.intersect(dirty).is_empty() {
            return;
        }
        let m = view.layout.metrics;
        let radius = m.omnibox_radius();
        self.round(rect, radius, view.palette.omnibox_fill, dirty);
        self.border(
            rect,
            radius,
            (if view.omnibox.focused { 1.5 } else { 1.0 }) * m.dpi() as f32 / 96.0,
            if view.omnibox.focused {
                view.palette.omnibox_focus_border
            } else {
                view.palette.omnibox_border
            },
            dirty,
        );
        let inset = m.px(Metrics::OMNIBOX_TEXT_INSET);
        let left_inset = m.px(Metrics::OMNIBOX_TEXT_LEFT);
        let line = self.text.line_height();
        let top = rect.top + (rect.height() - line) / 2;
        let origin = Rect::new(rect.left + left_inset, top, rect.right - inset, top + line);
        // Thin magnifier, left aligned and vertically centred in the field.
        let lens = m.px(10);
        let lens_rect = Rect::new(
            rect.left + m.px(10),
            (rect.top + rect.bottom - lens) / 2 - m.px(1),
            rect.left + m.px(10) + lens,
            (rect.top + rect.bottom - lens) / 2 - m.px(1) + lens,
        );
        let placeholder = view.palette.omnibox_placeholder;
        self.border(lens_rect, lens / 2, 1.2, placeholder, dirty);
        // The handle leaves the circle along its lower right diagonal.
        let reach = lens as f32 / 2.0 * std::f32::consts::FRAC_1_SQRT_2;
        let (lx, ly) = (
            (lens_rect.left + lens_rect.right) as f32 / 2.0 + reach,
            (lens_rect.top + lens_rect.bottom) as f32 / 2.0 + reach,
        );
        let handle = m.px(3) as f32;
        self.line_f(lx, ly, lx + handle, ly + handle, 1.2, placeholder, dirty);
        // Text, selection and caret never leave the field, however far the text is scrolled.
        let field = Rect::new(origin.left, rect.top, origin.right, rect.bottom);
        let caret_width = m.px(1).max(1);
        let value = view.omnibox.text;
        self.caret_rect = Rect::default();
        if value.is_empty() {
            self.omnibox_scroll = 0;
            if view.omnibox.focused {
                self.caret_rect = Rect::new(
                    origin.left,
                    origin.top + 1,
                    origin.left + caret_width,
                    origin.bottom - 1,
                );
            }
            self.draw_text(
                "Search or enter address",
                origin,
                view.palette.omnibox_placeholder,
                false,
                field,
                dirty,
            );
            if view.omnibox.caret_visible && view.omnibox.focused {
                self.fill_rect(self.caret_rect, view.palette.omnibox_caret, dirty);
            }
        } else {
            let length = encode_into(&mut self.text_units, value);
            let (sel_start, sel_end) = view.omnibox.selection;
            let start = sel_start.min(sel_end).min(length);
            let end = sel_start.max(sel_end).min(length);
            let units = &self.text_units[..length];
            let start_px = self.text.measure(units, start);
            let end_px = self.text.measure(units, end);
            let text_width = self.text.measure(units, length);
            self.omnibox_scroll = if view.omnibox.focused {
                // Selecting everything shows the start of the address, as a browser does.
                let anchor = if end > start && start == 0 {
                    start_px
                } else {
                    end_px
                };
                scroll_to_keep_visible(
                    self.omnibox_scroll,
                    anchor,
                    text_width,
                    origin.width(),
                    caret_width,
                )
            } else {
                0
            };
            let text_left = origin.left - self.omnibox_scroll;
            if end > start {
                let selected = Rect::new(
                    text_left + start_px,
                    rect.top + m.px(4),
                    text_left + end_px,
                    rect.bottom - m.px(4),
                )
                .intersect(field);
                self.round(selected, m.px(2), view.palette.omnibox_selection, dirty);
            }
            self.draw_text(
                value,
                Rect::new(text_left, origin.top, origin.right, origin.bottom),
                view.palette.omnibox_text,
                false,
                field,
                dirty,
            );
            let tail_x = text_left + text_width;
            if !view.omnibox.completion_suffix.is_empty() && tail_x < origin.right {
                self.draw_text(
                    view.omnibox.completion_suffix,
                    Rect::new(tail_x, origin.top, origin.right, origin.bottom),
                    view.palette.omnibox_completion,
                    false,
                    field,
                    dirty,
                );
            }
            if view.omnibox.focused {
                let x = text_left + end_px;
                self.caret_rect = Rect::new(x, origin.top + 1, x + caret_width, origin.bottom - 1)
                    .intersect(field);
                if view.omnibox.caret_visible {
                    self.fill_rect(self.caret_rect, view.palette.omnibox_caret, dirty);
                }
            }
        }
        if let Some(progress) = view.omnibox.progress {
            if progress > 0 {
                let unit = m.px(1).max(1);
                let stroke = if view.omnibox.focused { 2 * unit } else { unit };
                let gap = unit;
                let bar_h = 3 * unit;
                let bottom = rect.bottom - stroke - gap;
                let top = bottom - bar_h;
                let max_width = (rect.width() - stroke * 2).max(0);
                let width = (max_width as i64 * i64::from(progress.min(100)) / 100) as i32;
                let bar = Rect::new(rect.left + stroke, top, rect.left + stroke + width, bottom)
                    .intersect(rect);
                let (surface_w, surface_h, stride) = (self.width, self.height, self.stride());
                raster_rounded_rect_clipped(
                    self.dib.pixels_mut(),
                    surface_w,
                    surface_h,
                    stride,
                    bar,
                    (bar_h / 2) as f32,
                    rect,
                    radius as f32,
                    view.palette.progress_fill,
                    dirty,
                );
            }
        }
    }

    /// Draws `value` into `rect`, keeping only the part of it inside `clip`.
    fn draw_text(
        &mut self,
        value: &str,
        rect: Rect,
        color: Rgba,
        ellipsis: bool,
        clip: Rect,
        dirty: Rect,
    ) {
        let visible = rect.intersect(clip).intersect(dirty).intersect(Rect::new(
            0,
            0,
            self.width,
            self.height,
        ));
        if visible.is_empty() {
            return;
        }
        let length = encode_into(&mut self.label_units, value);
        self.text
            .draw(&self.label_units[..length], rect, visible, ellipsis);
        self.composite_mask(visible, color);
    }

    fn composite_mask(&mut self, rect: Rect, color: Rgba) {
        if self.text.dib.bits.is_null() || self.dib.bits.is_null() {
            return;
        }
        let mask = self.text.dib.pixels();
        let mask_stride = self.text.dib.stride();
        let stride = self.dib.stride();
        let pixels = self.dib.pixels_mut();
        for y in rect.top..rect.bottom {
            for x in rect.left..rect.right {
                let coverage = mask[y as usize * mask_stride + x as usize * 4 + 1];
                if coverage != 0 {
                    let i = y as usize * stride + x as usize * 4;
                    blend_pixel(&mut pixels[i..i + 4], color, coverage);
                }
            }
        }
    }

    fn blit_favicon(&mut self, rect: Rect, image: &FaviconBitmap, dirty: Rect) {
        if image.width == 0
            || image.height == 0
            || image.bgra_premultiplied.len()
                < (image.width as usize)
                    .saturating_mul(image.height as usize)
                    .saturating_mul(4)
        {
            return;
        }
        let clip = rect
            .intersect(dirty)
            .intersect(Rect::new(0, 0, self.width, self.height));
        if clip.is_empty() {
            return;
        }
        let stride = self.stride();
        let pixels = self.dib.pixels_mut();
        for y in clip.top..clip.bottom {
            for x in clip.left..clip.right {
                let sx = ((x - rect.left) as u64 * image.width as u64 / rect.width().max(1) as u64)
                    as usize;
                let sy = ((y - rect.top) as u64 * image.height as u64 / rect.height().max(1) as u64)
                    as usize;
                let si = (sy * image.width as usize + sx) * 4;
                let di = y as usize * stride + x as usize * 4;
                let src = &image.bgra_premultiplied[si..si + 4];
                over_premultiplied(&mut pixels[di..di + 4], src);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, width: f32, color: Rgba, dirty: Rect) {
        self.line_f(
            x0 as f32, y0 as f32, x1 as f32, y1 as f32, width, color, dirty,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn line_f(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: Rgba, dirty: Rect) {
        let (surface_w, surface_h, stride) = (self.width, self.height, self.stride());
        raster_line(
            self.dib.pixels_mut(),
            surface_w,
            surface_h,
            stride,
            x0,
            y0,
            x1,
            y1,
            width,
            color,
            dirty,
        );
    }
    fn outline_rect(&mut self, rect: Rect, width: f32, color: Rgba, dirty: Rect) {
        self.line(
            rect.left, rect.top, rect.right, rect.top, width, color, dirty,
        );
        self.line(
            rect.right,
            rect.top,
            rect.right,
            rect.bottom,
            width,
            color,
            dirty,
        );
        self.line(
            rect.right,
            rect.bottom,
            rect.left,
            rect.bottom,
            width,
            color,
            dirty,
        );
        self.line(
            rect.left,
            rect.bottom,
            rect.left,
            rect.top,
            width,
            color,
            dirty,
        );
    }
}

/// Clamps a client size to the chrome rows that are painted, and a DPI to at least 96.
fn surface_size(width: i32, height: i32, dpi: u32) -> (i32, i32, u32) {
    let dpi = dpi.max(96);
    let chrome = height.max(0).min(layout::chrome_height(dpi));
    (width.max(1), chrome.max(1), dpi)
}

/// Encodes `text` as UTF-16 into `out`, capped at `MAX_TEXT_UNITS` without splitting a surrogate
/// pair. Returns the number of units.
fn encode_into(out: &mut Vec<u16>, text: &str) -> usize {
    out.clear();
    out.extend(text.encode_utf16().take(MAX_TEXT_UNITS));
    if out.len() == MAX_TEXT_UNITS && out.last().is_some_and(|&unit| is_high_surrogate(unit)) {
        out.pop();
    }
    out.len()
}

fn is_high_surrogate(unit: u16) -> bool {
    (0xD800..0xDC00).contains(&unit)
}

fn is_low_surrogate(unit: u16) -> bool {
    (0xDC00..0xE000).contains(&unit)
}

/// Whether `index` is between characters, not inside a surrogate pair.
fn is_boundary(text: &[u16], index: usize) -> bool {
    index == 0
        || index >= text.len()
        || !(is_low_surrogate(text[index]) && is_high_surrogate(text[index - 1]))
}

/// The index of the character boundary nearest to pixel offset `x`, where `width_of(i)` is the
/// pixel width of the first `i` units. Never splits a surrogate pair.
fn nearest_boundary(text: &[u16], x: i32, width_of: impl Fn(usize) -> i32) -> usize {
    let len = text.len();
    if x <= 0 || len == 0 {
        return 0;
    }
    // Largest index whose prefix still fits in `x`.
    let (mut lo, mut hi) = (0, len);
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if width_of(mid) <= x {
            lo = mid
        } else {
            hi = mid - 1
        }
    }
    if !is_boundary(text, lo) {
        lo -= 1;
    }
    if lo >= len {
        return len;
    }
    let mut next = lo + 1;
    if !is_boundary(text, next) {
        next += 1;
    }
    if x - width_of(lo) > width_of(next) - x {
        next
    } else {
        lo
    }
}

/// The new horizontal scroll offset that keeps `anchor` (a pixel offset into the text) visible in
/// a field `field` pixels wide, starting from `scroll`. Never scrolls past the end of the text.
fn scroll_to_keep_visible(
    scroll: i32,
    anchor: i32,
    text_width: i32,
    field: i32,
    caret_width: i32,
) -> i32 {
    if field <= caret_width {
        return 0;
    }
    let max_scroll = (text_width + caret_width - field).max(0);
    let mut scroll = scroll.clamp(0, max_scroll);
    if anchor < scroll {
        scroll = anchor;
    } else if anchor + caret_width > scroll + field {
        scroll = anchor + caret_width - field;
    }
    scroll.clamp(0, max_scroll)
}

fn center(rect: Rect) -> (i32, i32) {
    ((rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2)
}

/// Source-over in premultiplied BGRA. `coverage` modulates the source alpha.
fn blend_pixel(dst: &mut [u8], color: Rgba, coverage: u8) {
    if color.a == 255 && coverage == 255 {
        dst.copy_from_slice(&[color.b, color.g, color.r, 255]);
        return;
    }
    let alpha = div255(u32::from(color.a) * u32::from(coverage));
    let inverse = 255 - alpha;
    let src = [
        div255(u32::from(color.b) * alpha),
        div255(u32::from(color.g) * alpha),
        div255(u32::from(color.r) * alpha),
        alpha,
    ];
    for i in 0..4 {
        dst[i] = (src[i] + div255(u32::from(dst[i]) * inverse)).min(255) as u8;
    }
}

/// Blends two colours by weighting each with its alpha, so a transparent end adds no tint.
fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let (wa, wb) = ((1.0 - t) * f32::from(a.a), t * f32::from(b.a));
    let alpha = wa + wb;
    if alpha <= 0.0 {
        return a;
    }
    let channel = |x: u8, y: u8| ((f32::from(x) * wa + f32::from(y) * wb) / alpha).round() as u8;
    Rgba {
        r: channel(a.r, b.r),
        g: channel(a.g, b.g),
        b: channel(a.b, b.b),
        a: alpha.round() as u8,
    }
}

/// `colour` with its alpha multiplied by `factor`, clamped to `0.0..=1.0`.
fn scale_alpha(colour: Rgba, factor: f32) -> Rgba {
    let a = (f32::from(colour.a) * factor.clamp(0.0, 1.0)).round() as u8;
    Rgba { a, ..colour }
}

fn over_premultiplied(dst: &mut [u8], src: &[u8]) {
    let inv = 255 - u32::from(src[3]);
    for i in 0..4 {
        dst[i] = (u32::from(src[i]) + div255(u32::from(dst[i]) * inv)).min(255) as u8;
    }
}

/// `round(v / 255)` for `v <= 255 * 255`, without a division.
fn div255(v: u32) -> u32 {
    let t = v + 128;
    (t + (t >> 8)) >> 8
}

fn rounded_distance(x: f32, y: f32, rect: Rect, radius: f32) -> f32 {
    let hw = (rect.width() as f32 / 2.0).max(0.0);
    let hh = (rect.height() as f32 / 2.0).max(0.0);
    let r = radius.max(0.0).min(hw.min(hh));
    let qx = (x - (rect.left as f32 + hw)).abs() - (hw - r);
    let qy = (y - (rect.top as f32 + hh)).abs() - (hh - r);
    let (ox, oy) = (qx.max(0.0), qy.max(0.0));
    // Away from the corners one of the offsets is zero and the hypotenuse is the other.
    let outside = if ox == 0.0 {
        oy
    } else if oy == 0.0 {
        ox
    } else {
        ox.hypot(oy)
    };
    outside + qx.max(qy).min(0.0) - r
}

#[allow(clippy::too_many_arguments)]
fn raster_rounded_rect(
    pixels: &mut [u8],
    width: i32,
    height: i32,
    stride: usize,
    rect: Rect,
    radius: f32,
    color: Rgba,
    dirty: Rect,
) {
    if rect.is_empty() || width <= 0 || height <= 0 {
        return;
    }
    let bounds = rect
        .intersect(dirty)
        .intersect(Rect::new(0, 0, width, height));
    for y in bounds.top..bounds.bottom {
        for x in bounds.left..bounds.right {
            let d = rounded_distance(x as f32 + 0.5, y as f32 + 0.5, rect, radius);
            let cov = ((0.5 - d).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            if cov > 0 {
                let i = y as usize * stride + x as usize * 4;
                blend_pixel(&mut pixels[i..i + 4], color, cov);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn raster_round_border(
    pixels: &mut [u8],
    width: i32,
    height: i32,
    stride: usize,
    rect: Rect,
    radius: f32,
    thickness: f32,
    color: Rgba,
    dirty: Rect,
) {
    if rect.is_empty() || thickness <= 0.0 {
        return;
    }
    let bounds = rect
        .intersect(dirty)
        .intersect(Rect::new(0, 0, width, height));
    for y in bounds.top..bounds.bottom {
        for x in bounds.left..bounds.right {
            let d = rounded_distance(x as f32 + 0.5, y as f32 + 0.5, rect, radius);
            let outer = (0.5 - d).clamp(0.0, 1.0);
            let inner = (0.5 - (d + thickness)).clamp(0.0, 1.0);
            let cov = ((outer - inner).max(0.0) * 255.0 + 0.5) as u8;
            if cov > 0 {
                let i = y as usize * stride + x as usize * 4;
                blend_pixel(&mut pixels[i..i + 4], color, cov);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn raster_rounded_rect_clipped(
    pixels: &mut [u8],
    width: i32,
    height: i32,
    stride: usize,
    rect: Rect,
    radius: f32,
    clip: Rect,
    clip_radius: f32,
    color: Rgba,
    dirty: Rect,
) {
    if rect.is_empty() || clip.is_empty() {
        return;
    }
    let bounds = rect
        .intersect(clip)
        .intersect(dirty)
        .intersect(Rect::new(0, 0, width, height));
    for y in bounds.top..bounds.bottom {
        for x in bounds.left..bounds.right {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let bar_cov = (0.5 - rounded_distance(px, py, rect, radius)).clamp(0.0, 1.0);
            let clip_cov = (0.5 - rounded_distance(px, py, clip, clip_radius)).clamp(0.0, 1.0);
            let coverage = (bar_cov * clip_cov * 255.0 + 0.5) as u8;
            if coverage > 0 {
                let i = y as usize * stride + x as usize * 4;
                blend_pixel(&mut pixels[i..i + 4], color, coverage);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn raster_line(
    pixels: &mut [u8],
    width: i32,
    height: i32,
    stride: usize,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    thickness: f32,
    color: Rgba,
    dirty: Rect,
) {
    if width <= 0 || height <= 0 {
        return;
    }
    let pad = (thickness / 2.0 + 1.0).ceil() as i32;
    let bounds = Rect::new(
        x0.min(x1).floor() as i32 - pad,
        y0.min(y1).floor() as i32 - pad,
        x0.max(x1).ceil() as i32 + pad,
        y0.max(y1).ceil() as i32 + pad,
    )
    .intersect(dirty)
    .intersect(Rect::new(0, 0, width, height));
    let vx = x1 - x0;
    let vy = y1 - y0;
    let len2 = vx * vx + vy * vy;
    for y in bounds.top..bounds.bottom {
        for x in bounds.left..bounds.right {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let t = if len2 == 0.0 {
                0.0
            } else {
                (((px - x0) * vx + (py - y0) * vy) / len2).clamp(0.0, 1.0)
            };
            let d = (px - (x0 + t * vx)).hypot(py - (y0 + t * vy)) - thickness / 2.0;
            let cov = ((0.5 - d).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            if cov > 0 {
                let i = y as usize * stride + x as usize * 4;
                blend_pixel(&mut pixels[i..i + 4], color, cov);
            }
        }
    }
}

/// A 32-bit top-down DIB section selected into its own memory DC.
struct Dib {
    dc: HDC,
    bitmap: HBITMAP,
    bits: *mut u8,
    width: i32,
    height: i32,
}
impl Dib {
    fn new(width: i32, height: i32) -> Self {
        let mut value = Self {
            dc: HDC::default(),
            bitmap: HBITMAP::default(),
            bits: ptr::null_mut(),
            width: 0,
            height: 0,
        };
        value.resize(width, height);
        value
    }
    /// Reallocates for a new size and reports whether it did. A freshly created DIB is zeroed.
    fn resize(&mut self, width: i32, height: i32) -> bool {
        let (width, height) = (width.max(1), height.max(1));
        if self.width == width && self.height == height {
            return false;
        }
        self.release();
        self.width = width;
        self.height = height;
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
                    biSizeImage: 0,
                    ..Default::default()
                },
                bmiColors: [Default::default()],
            };
            let mut bits = ptr::null_mut();
            let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
                .unwrap_or_default();
            let _ = SelectObject(dc, bitmap.into());
            self.dc = dc;
            self.bitmap = bitmap;
            self.bits = bits.cast();
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
        self.dc = HDC::default();
        self.bitmap = HBITMAP::default();
        self.bits = ptr::null_mut();
    }
    fn stride(&self) -> usize {
        self.width as usize * 4
    }
    fn len(&self) -> usize {
        self.stride() * self.height as usize
    }
    fn pixels(&self) -> &[u8] {
        if self.bits.is_null() {
            return &[];
        }
        // SAFETY: `bits` points to the `len()` bytes of the live DIB section owned by `self`.
        unsafe { slice::from_raw_parts(self.bits, self.len()) }
    }
    fn pixels_mut(&mut self) -> &mut [u8] {
        if self.bits.is_null() {
            return &mut [];
        }
        // SAFETY: as in `pixels`, and `&mut self` makes this the only live view.
        unsafe { slice::from_raw_parts_mut(self.bits, self.len()) }
    }
}
impl Drop for Dib {
    fn drop(&mut self) {
        self.release();
    }
}

/// A white-on-black GDI text mask sharing coordinates with the chrome surface. Its DC permanently
/// holds the current font, so measuring and drawing need no state changes.
struct TextCache {
    dib: Dib,
    font: HFONT,
    /// The font the DC started with, selected back before the cached fonts are deleted.
    original_font: HGDIOBJ,
    dpi: u32,
    fonts: Vec<(u32, HFONT)>,
    line_height: i32,
}
impl TextCache {
    fn new(width: i32, height: i32, dpi: u32) -> Self {
        let mut this = Self {
            dib: Dib::new(width, height),
            font: HFONT::default(),
            original_font: HGDIOBJ::default(),
            dpi: 0,
            fonts: Vec::new(),
            line_height: 16,
        };
        this.prepare_dc();
        this.set_dpi(dpi);
        this
    }
    fn resize(&mut self, width: i32, height: i32, dpi: u32) {
        if self.dib.resize(width, height) {
            self.prepare_dc();
            self.select_font();
        }
        self.set_dpi(dpi);
    }
    /// One-off DC state for a freshly created DIB.
    fn prepare_dc(&mut self) {
        unsafe {
            let _ = SetTextColor(self.dib.dc, COLORREF(0x00FF_FFFF));
            let _ = SetBkColor(self.dib.dc, COLORREF(0));
            let _ = SetBkMode(self.dib.dc, TRANSPARENT);
        }
        self.original_font = HGDIOBJ::default();
    }
    fn set_dpi(&mut self, dpi: u32) {
        if self.dpi == dpi {
            return;
        }
        self.dpi = dpi;
        self.font = if let Some((_, font)) = self.fonts.iter().find(|(d, _)| *d == dpi) {
            *font
        } else {
            let font = create_message_font(dpi, ANTIALIASED_QUALITY);
            self.fonts.push((dpi, font));
            font
        };
        self.select_font();
    }
    fn select_font(&mut self) {
        unsafe {
            let previous = SelectObject(self.dib.dc, self.font.into());
            if self.original_font.0.is_null() {
                self.original_font = previous;
            }
            let mut metrics = TEXTMETRICW::default();
            let _ = GetTextMetricsW(self.dib.dc, &mut metrics);
            self.line_height = metrics.tmHeight.max(1);
        }
    }
    fn rebuild_fonts(&mut self) {
        unsafe {
            if !self.original_font.0.is_null() {
                let _ = SelectObject(self.dib.dc, self.original_font);
            }
            for (_, font) in self.fonts.drain(..) {
                if !font.0.is_null() {
                    let _ = DeleteObject(font.into());
                }
            }
        }
        self.font = HFONT::default();
        let dpi = std::mem::replace(&mut self.dpi, 0);
        self.set_dpi(dpi);
    }
    fn line_height(&self) -> i32 {
        self.line_height
    }
    /// Draws `text` inside `rect`, touching only `visible`, which is the part that will be read
    /// back. `visible` must lie inside the mask.
    fn draw(&self, text: &[u16], rect: Rect, visible: Rect, ellipsis: bool) {
        if self.dib.bits.is_null() {
            return;
        }
        unsafe {
            let _ = PatBlt(
                self.dib.dc,
                visible.left,
                visible.top,
                visible.width(),
                visible.height(),
                BLACKNESS,
            );
        }
        // `Vec::as_ptr()` for an empty UTF-16 buffer is a dangling sentinel (0x2 on Windows).
        // DrawTextW may still inspect that pointer when the count is zero, so never call GDI for
        // an empty label. Clearing first also prevents a previous mask from being composited.
        if text.is_empty() {
            return;
        }
        unsafe {
            let _ = IntersectClipRect(
                self.dib.dc,
                visible.left,
                visible.top,
                visible.right,
                visible.bottom,
            );
            let mut r = RECT {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            };
            let mut flags = DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX;
            if ellipsis {
                flags |= DT_END_ELLIPSIS;
            }
            let _ = draw_text_raw(
                self.dib.dc,
                text.as_ptr(),
                text.len() as i32,
                &mut r,
                flags.0,
            );
            let _ = SelectClipRgn(self.dib.dc, None);
        }
    }
    /// Pixel width of the first `index` units of `text`.
    fn measure(&self, text: &[u16], index: usize) -> i32 {
        let count = index.min(text.len());
        if count == 0 {
            return 0;
        }
        let mut size = SIZE::default();
        unsafe {
            let _ = GetTextExtentPoint32W(self.dib.dc, &text[..count], &mut size);
        }
        size.cx
    }
}
impl Drop for TextCache {
    fn drop(&mut self) {
        unsafe {
            if !self.original_font.0.is_null() {
                let _ = SelectObject(self.dib.dc, self.original_font);
            }
            for (_, font) in self.fonts.drain(..) {
                if !font.0.is_null() {
                    let _ = DeleteObject(font.into());
                }
            }
        }
    }
}

/// The system message font at `dpi`. `SystemParametersInfoForDpi` already scales the height.
pub fn create_message_font(
    dpi: u32,
    quality: windows::Win32::Graphics::Gdi::FONT_QUALITY,
) -> HFONT {
    unsafe {
        let mut ncm = NONCLIENTMETRICSW {
            cbSize: size_of::<NONCLIENTMETRICSW>() as u32,
            ..Default::default()
        };
        if SystemParametersInfoForDpi(
            SPI_GETNONCLIENTMETRICS.0,
            ncm.cbSize,
            Some((&mut ncm as *mut NONCLIENTMETRICSW).cast()),
            0,
            dpi.max(96),
        )
        .is_err()
        {
            return HFONT::default();
        }
        let mut logfont: LOGFONTW = ncm.lfMessageFont;
        logfont.lfQuality = quality;
        CreateFontIndirectW(&logfont)
    }
}

/// Realizes the chrome's antialiased message font once, off the UI thread. GDI's first draw
/// with a font loads and rasterizes it (5–15 ms measured); the realization is shared
/// process-wide, so doing it while the main thread creates the window shortens the first paint.
pub fn prewarm_text(dpi: u32) {
    let dib = Dib::new(64, 16);
    if dib.bits.is_null() {
        return;
    }
    let font = create_message_font(dpi, ANTIALIASED_QUALITY);
    if font.0.is_null() {
        return;
    }
    let sample: Vec<u16> = "New tab Search or enter address".encode_utf16().collect();
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 64,
        bottom: 16,
    };
    unsafe {
        let previous = SelectObject(dib.dc, font.into());
        let _ = SetTextColor(dib.dc, COLORREF(0x00FF_FFFF));
        let _ = SetBkMode(dib.dc, TRANSPARENT);
        draw_text_raw(
            dib.dc,
            sample.as_ptr(),
            sample.len() as i32,
            &mut rect,
            (DT_SINGLELINE | DT_NOPREFIX).0,
        );
        let _ = SelectObject(dib.dc, previous);
        let _ = DeleteObject(font.into());
    }
}

#[link(name = "user32")]
unsafe extern "system" {
    #[link_name = "DrawTextW"]
    fn draw_text_raw(hdc: HDC, text: *const u16, count: i32, rect: *mut RECT, format: u32) -> i32;
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use crate::{
        chrome::motion::{self, Motion},
        tabs::TabId,
        theme::{Appearance, palette},
    };
    use std::{fs, path::PathBuf, time::Instant};

    fn surface(origin: (i32, i32), width: i32, height: i32, dpi: u32) -> PresentedSurface {
        PresentedSurface {
            width,
            height,
            dpi,
            origin,
        }
    }

    #[test]
    fn upload_decision_requires_full_surface_on_first_present() {
        assert_eq!(
            upload_decision(None, surface((10, 20), 400, 100, 96), Rect::new(1, 2, 3, 4)),
            Upload::Full
        );
    }

    #[test]
    fn upload_decision_uses_dirty_region_for_unchanged_surface() {
        let current = surface((10, 20), 400, 100, 96);
        let dirty = Rect::new(1, 2, 3, 4);
        assert_eq!(
            upload_decision(Some(current), current, dirty),
            Upload::Dirty(dirty)
        );
    }

    #[test]
    fn upload_decision_requires_full_surface_after_size_or_dpi_change() {
        let previous = surface((10, 20), 400, 100, 96);
        let dirty = Rect::new(1, 2, 3, 4);
        assert_eq!(
            upload_decision(Some(previous), surface((10, 20), 401, 100, 96), dirty),
            Upload::Full
        );
        assert_eq!(
            upload_decision(Some(previous), surface((10, 20), 400, 100, 120), dirty),
            Upload::Full
        );
    }

    #[test]
    fn upload_decision_requires_full_surface_after_origin_change() {
        let previous = surface((10, 20), 400, 100, 96);
        assert_eq!(
            upload_decision(
                Some(previous),
                surface((11, 20), 400, 100, 96),
                Rect::new(1, 2, 3, 4)
            ),
            Upload::Full
        );
    }

    #[test]
    fn upload_decision_skips_empty_dirty_region() {
        let current = surface((10, 20), 400, 100, 96);
        assert_eq!(
            upload_decision(None, current, Rect::default()),
            Upload::Skip
        );
    }

    #[test]
    fn successful_upload_records_the_presented_surface() {
        let current = surface((10, 20), 400, 100, 96);
        assert_eq!(
            last_presented_after_upload(None, current, Upload::Full, true),
            Some(current)
        );
    }

    #[test]
    fn failed_upload_clears_presented_surface_so_the_next_upload_is_full() {
        let current = surface((10, 20), 400, 100, 96);
        let dirty = Rect::new(1, 2, 3, 4);
        let previous =
            last_presented_after_upload(Some(current), current, Upload::Dirty(dirty), false);
        assert_eq!(previous, None);
        assert_eq!(upload_decision(previous, current, dirty), Upload::Full);
    }

    #[test]
    fn rounded_rect_coverage_is_symmetric_and_bounded() {
        let rect = Rect::new(1, 1, 9, 9);
        let mut p = vec![0_u8; 400];
        raster_rounded_rect(
            &mut p,
            10,
            10,
            40,
            rect,
            3.0,
            Rgba::hex(0xFFFFFF, 255),
            Rect::new(0, 0, 10, 10),
        );
        assert_eq!(p[59], p[339]);
        assert!(p[47] < p[59]);
    }

    #[test]
    fn compositing_preserves_premultiplied_invariant() {
        let mut dst = [0_u8; 4];
        blend_pixel(&mut dst, Rgba::hex(0xFF8040, 128), 128);
        assert!(dst[0] <= dst[3] && dst[1] <= dst[3] && dst[2] <= dst[3]);
        assert_ne!(dst, [0; 4]);
    }
    #[test]
    fn zero_sized_and_tiny_rounded_rects_are_safe() {
        let mut p = [0_u8; 16];
        raster_rounded_rect(
            &mut p,
            2,
            2,
            8,
            Rect::new(1, 1, 1, 2),
            10.0,
            Rgba::hex(0xFFFFFF, 255),
            Rect::new(0, 0, 2, 2),
        );
        raster_rounded_rect(
            &mut p,
            2,
            2,
            8,
            Rect::new(0, 0, 2, 2),
            10.0,
            Rgba::hex(0xFFFFFF, 255),
            Rect::new(0, 0, 2, 2),
        );
    }
    #[test]
    fn rasterizer_clips_to_dirty_rectangle() {
        let mut p = vec![0; 8 * 8 * 4];
        raster_rounded_rect(
            &mut p,
            8,
            8,
            32,
            Rect::new(0, 0, 8, 8),
            0.0,
            Rgba::hex(0xFFFFFF, 255),
            Rect::new(2, 2, 4, 4),
        );
        assert_eq!(p[39], 0);
        assert!(p[(2 * 8 + 2) * 4 + 3] > 0);
        assert_eq!(p[(4 * 8 + 4) * 4 + 3], 0);
    }

    #[test]
    fn repeated_dirty_paint_does_not_accumulate_translucent_alpha() {
        let layout = crate::chrome::layout::compute(500, 120, 96, 1, false);
        let tabs = [TabView {
            title: "Tab",
            loading: false,
            favicon: None,
            emphasis: rest(),
            group_size: None,
            group_control: false,
        }];
        let view = ChromeView {
            layout: &layout,
            palette: palette(Appearance::Light, false),
            tabs: &tabs,
            active: 0,
            hover: Hit::None,
            pressed: Hit::None,
            caption_maximised: false,
            can_back: false,
            can_forward: false,
            omnibox: OmniboxView {
                text: "",
                selection: (0, 0),
                caret_visible: false,
                focused: false,
                completion_suffix: "",
                progress: None,
            },
            now: Instant::now(),
            departures: &[],
        };
        let dirty = Rect::new(0, 0, 240, 84);
        let mut surface = ChromeSurface::new(500, 120, 96);
        surface.paint(&view, dirty);
        let once = surface.pixels().to_vec();
        surface.paint(&view, dirty);
        assert_eq!(surface.pixels(), once);
    }
    #[test]
    fn empty_and_huge_text_measurement_is_bounded() {
        let mut s = ChromeSurface::new(320, 100, 96);
        assert_eq!(s.caret_x("", 0), 0);
        let huge = "x".repeat(100_000);
        let capped = s.caret_x(&huge, usize::MAX);
        assert_eq!(capped, s.caret_x(&"x".repeat(MAX_TEXT_UNITS), usize::MAX));
        let text_len = encode_into(&mut s.label_units, &huge);
        assert_eq!(text_len, MAX_TEXT_UNITS);
        s.text.draw(
            &s.label_units[..text_len],
            Rect::new(0, 0, 40, s.text.line_height()),
            Rect::new(0, 0, 40, s.text.line_height()),
            true,
        );
    }

    #[test]
    fn capped_text_never_ends_inside_a_surrogate_pair() {
        let mut units = Vec::new();
        let text = format!("{}\u{1F600}", "a".repeat(MAX_TEXT_UNITS - 1));
        assert_eq!(encode_into(&mut units, &text), MAX_TEXT_UNITS - 1);
        assert!(units.iter().all(|&unit| unit == u16::from(b'a')));
    }

    fn rest() -> Emphasis {
        Emphasis::resting(false, false, Instant::now())
    }

    fn test_view<'a>(
        layout: &'a Layout,
        tabs: &'a [TabView<'a>],
        palette: &'a Palette,
        omnibox: OmniboxView<'a>,
    ) -> ChromeView<'a> {
        ChromeView {
            layout,
            palette,
            tabs,
            active: 0,
            hover: Hit::None,
            pressed: Hit::None,
            caption_maximised: false,
            can_back: true,
            can_forward: false,
            omnibox,
            now: Instant::now(),
            departures: &[],
        }
    }

    fn omnibox<'a>(text: &'a str, selection: (usize, usize), suffix: &'a str) -> OmniboxView<'a> {
        OmniboxView {
            text,
            selection,
            caret_visible: true,
            focused: true,
            completion_suffix: suffix,
            progress: None,
        }
    }

    fn alpha_at(surface: &ChromeSurface, x: i32, y: i32) -> u8 {
        surface.pixels()[y as usize * surface.stride() + x as usize * 4 + 3]
    }

    /// Paints two glass tabs, the first selected, sampled at `now` with the given departures.
    fn paint_tabs_at(
        layout: &Layout,
        emphasis: [Emphasis; 2],
        now: Instant,
        departures: &[Departure],
    ) -> ChromeSurface {
        let tabs = emphasis.map(|emphasis| TabView {
            title: "One",
            loading: false,
            favicon: None,
            emphasis,
            group_size: None,
            group_control: false,
        });
        let view = ChromeView {
            layout,
            palette: palette(Appearance::Dark, false),
            tabs: &tabs,
            active: 0,
            hover: Hit::None,
            pressed: Hit::None,
            caption_maximised: false,
            can_back: true,
            can_forward: false,
            omnibox: omnibox("", (0, 0), ""),
            now,
            departures,
        };
        let mut surface = ChromeSurface::new(1000, 120, 96);
        surface.paint(&view, Rect::new(0, 0, 1000, surface.height()));
        surface
    }

    #[test]
    fn a_settled_hover_paints_exactly_like_a_resting_hover() {
        let layout = layout::compute(1000, 120, 96, 2, false);
        let start = Instant::now();
        let settled = start + std::time::Duration::from_millis(400);
        let mut fading = Emphasis::resting(false, false, start);
        fading.retarget(true, false, start, false);
        let selected = Emphasis::resting(false, true, start);
        let animated = paint_tabs_at(&layout, [fading, selected], settled, &[]);
        let resting = paint_tabs_at(
            &layout,
            [Emphasis::resting(true, false, start), selected],
            settled,
            &[],
        );
        assert!(
            animated.pixels() == resting.pixels(),
            "a finished transition must not leave a residue"
        );
    }

    #[test]
    fn hover_fades_in_monotonically_from_the_resting_island() {
        let layout = layout::compute(1000, 120, 96, 2, false);
        let island = layout.tab(0).unwrap();
        let (x, y) = center(island);
        let start = Instant::now();
        let mut fading = Emphasis::resting(false, false, start);
        fading.retarget(true, false, start, false);
        let selected = Emphasis::resting(false, true, start);
        let at = |millis: u64| {
            let now = start + std::time::Duration::from_millis(millis);
            alpha_at(&paint_tabs_at(&layout, [fading, selected], now, &[]), x, y)
        };
        let (rest, quarter, half, done) = (at(0), at(40), at(75), at(150));
        assert!(
            rest > 0 && rest < 0x30,
            "the resting translucent island has only a subtle fill, got {rest}"
        );
        assert!(
            rest < quarter && quarter < half && half < done,
            "fill alpha must rise steadily: {rest} {quarter} {half} {done}"
        );
    }

    #[test]
    fn a_closed_tab_fades_out_over_its_island_and_then_leaves_nothing() {
        let layout = layout::compute(1000, 120, 96, 2, false);
        let island = layout.tab(1).unwrap();
        let (x, y) = center(island);
        let start = Instant::now();
        let (a, b) = (TabId(1), TabId(2));
        let mut motion = Motion::new(false);
        motion.sync(&[a, b], None, Some(a), |_| None, start);
        motion.sync(&[a], None, Some(a), |id| (id == b).then_some(island), start);
        let rest = [Emphasis::resting(false, false, start); 2];
        let baseline = paint_tabs_at(&layout, rest, start, &[]);
        let ghost_at = |millis: u64| {
            let now = start + std::time::Duration::from_millis(millis);
            paint_tabs_at(&layout, rest, now, motion.departures())
        };
        let closing = alpha_at(&ghost_at(0), x, y);
        let fading = alpha_at(&ghost_at(75), x, y);
        assert!(closing > 0, "the island is visible as the tab closes");
        assert!(
            0 < fading && fading < closing,
            "{fading} should be between 0 and {closing}"
        );
        let finished = ghost_at(motion::TRANSITION.as_millis() as u64);
        assert!(
            finished.pixels() == baseline.pixels(),
            "a finished departure must leave no pixels behind"
        );
    }

    #[test]
    fn typed_text_with_a_completion_suffix_paints_without_panicking() {
        // The caret used to be measured from a scratch buffer that drawing the suffix had already
        // overwritten, which sliced past the end of the shorter suffix.
        let layout = layout::compute(1000, 120, 96, 1, false);
        let tabs = [TabView {
            title: "Wikipedia",
            loading: false,
            favicon: None,
            emphasis: rest(),
            group_size: None,
            group_control: false,
        }];
        let text = "https://en.wikipedia.org/wi";
        let mut surface = ChromeSurface::new(1000, 120, 96);
        let pal = palette(Appearance::Dark, false);
        let end = text.encode_utf16().count();
        let dirty = Rect::new(0, 0, 1000, surface.height());
        let view = test_view(&layout, &tabs, pal, omnibox(text, (end, end), "ki/Rust"));
        surface.paint(&view, dirty);
        let view = test_view(&layout, &tabs, pal, omnibox(text, (3, 9), "ki/Rust"));
        surface.paint(&view, dirty);
    }

    #[test]
    fn glass_chrome_leaves_gaps_transparent_and_solid_chrome_is_opaque() {
        let layout = layout::compute(1000, 700, 96, 2, false);
        let tabs = [TabView {
            title: "One",
            loading: false,
            favicon: None,
            emphasis: rest(),
            group_size: None,
            group_control: false,
        }; 2];
        let mut surface = ChromeSurface::new(1000, 700, 96);
        let dirty = Rect::new(0, 0, 1000, surface.height());
        let glass = palette(Appearance::Dark, false);
        surface.paint(
            &test_view(&layout, &tabs, glass, omnibox("", (0, 0), "")),
            dirty,
        );
        // Left margin, right of the last tab, the gap above the omnibox and below it.
        for (x, y) in [(0, 20), (600, 20), (500, 31), (500, 61)] {
            assert_eq!(
                alpha_at(&surface, x, y),
                0,
                "glass pixel ({x},{y}) must be transparent"
            );
        }
        let omni = layout.omnibox;
        let fill = alpha_at(&surface, (omni.left + omni.right) / 2, omni.top + 4);
        assert!(
            fill > 0 && fill < 255,
            "the omnibox fill is translucent, got alpha {fill}"
        );

        let solid = palette(Appearance::Dark, true);
        surface.paint(
            &test_view(&layout, &tabs, solid, omnibox("", (0, 0), "")),
            dirty,
        );
        for y in 0..surface.height() {
            for x in 0..surface.width() {
                assert_eq!(alpha_at(&surface, x, y), 255, "solid pixel ({x},{y})");
            }
        }
    }

    #[test]
    fn omnibox_text_is_clipped_to_the_field_and_scrolls_to_the_caret() {
        let layout = layout::compute(600, 120, 96, 1, false);
        let tabs = [TabView {
            title: "T",
            loading: false,
            favicon: None,
            emphasis: rest(),
            group_size: None,
            group_control: false,
        }];
        let pal = palette(Appearance::Light, true);
        let mut surface = ChromeSurface::new(600, 120, 96);
        let dirty = Rect::new(0, 0, 600, surface.height());
        let long = "https://example.com/".to_owned() + &"segment/".repeat(60);
        let end = long.encode_utf16().count();

        let mut unfocused = omnibox("x", (0, 0), "");
        unfocused.focused = false;
        surface.paint(&test_view(&layout, &tabs, pal, unfocused), dirty);
        let short = surface.pixels().to_vec();
        let mut unfocused = omnibox(&long, (0, 0), "");
        unfocused.focused = false;
        surface.paint(&test_view(&layout, &tabs, pal, unfocused), dirty);
        assert_eq!(
            surface.omnibox_scroll, 0,
            "an unfocused field shows the start"
        );
        let o = layout.omnibox;
        let inset = layout.metrics.px(Metrics::OMNIBOX_TEXT_INSET);
        let right_margin = (o.right - inset + 1)..(o.right - 4);
        let middle = (o.top + o.bottom) / 2;
        for y in (middle - 6)..(middle + 6) {
            for x in right_margin.clone() {
                let i = y as usize * surface.stride() + x as usize * 4;
                assert_eq!(
                    surface.pixels()[i..i + 4],
                    short[i..i + 4],
                    "text leaked past the field at ({x},{y})"
                );
            }
        }

        surface.paint(
            &test_view(&layout, &tabs, pal, omnibox(&long, (end, end), "")),
            dirty,
        );
        assert!(
            surface.omnibox_scroll > 0,
            "the caret at the end must scroll the text"
        );
        let left_inset = layout.metrics.px(Metrics::OMNIBOX_TEXT_LEFT);
        let field_width = o.width() - left_inset - inset;
        let at_left = surface.index_at_x(&long, 0);
        let at_right = surface.index_at_x(&long, field_width);
        assert!(
            at_left > 0,
            "x = 0 maps past the scrolled-off start, got {at_left}"
        );
        assert!(
            at_right >= end - 1,
            "the right edge maps to the end, got {at_right}"
        );
        let caret = surface.caret_x(&long, end);
        assert_eq!(
            surface.omnibox_scroll + field_width - layout.metrics.px(1).max(1),
            caret,
            "the caret sits at the right edge of the field"
        );
    }

    #[test]
    fn a_clipped_last_tab_does_not_draw_into_the_gap_before_the_new_tab_button() {
        let layout = layout::compute_with_active(768, 700, 96, 40, 39, false);
        let tabs = vec![
            TabView {
                title: "A tab with a title",
                loading: false,
                favicon: None,
                emphasis: rest(),
                group_size: None,
                group_control: false,
            };
            40
        ];
        let pal = palette(Appearance::Dark, false);
        let mut surface = ChromeSurface::new(768, 700, 96);
        let mut view = test_view(&layout, &tabs, pal, omnibox("", (0, 0), ""));
        view.active = 39;
        view.hover = Hit::TabClose(39);
        surface.paint(&view, Rect::new(0, 0, 768, surface.height()));
        for x in layout.tab_zone.right..layout.tab_menu_button.left {
            for y in 0..layout.strip.bottom {
                assert_eq!(
                    alpha_at(&surface, x, y),
                    0,
                    "pixel ({x},{y}) between tabs and the tab list button"
                );
            }
        }
    }

    #[test]
    fn nearest_boundary_picks_the_closest_character_edge() {
        let text: Vec<u16> = "abc".encode_utf16().collect();
        let width = |i: usize| i as i32 * 10;
        let at = |x| nearest_boundary(&text, x, width);
        assert_eq!(at(-5), 0);
        assert_eq!(at(0), 0);
        assert_eq!(at(4), 0);
        assert_eq!(at(6), 1, "closer to the right edge of the first letter");
        assert_eq!(at(14), 1);
        assert_eq!(at(24), 2);
        assert_eq!(at(26), 3);
        assert_eq!(at(500), 3);
        assert_eq!(nearest_boundary(&[], 10, width), 0);
    }

    #[test]
    fn nearest_boundary_never_splits_a_surrogate_pair() {
        let text: Vec<u16> = "a\u{1F600}b".encode_utf16().collect();
        assert_eq!(text.len(), 4);
        let width = |i: usize| i as i32 * 10;
        for x in -5..60 {
            let index = nearest_boundary(&text, x, width);
            assert!(
                [0, 1, 3, 4].contains(&index),
                "x {x} gave {index}, inside the emoji"
            );
        }
        assert_eq!(nearest_boundary(&text, 12, width), 1);
        assert_eq!(nearest_boundary(&text, 28, width), 3);
    }

    #[test]
    fn scroll_keeps_the_caret_visible_and_stops_at_the_text_end() {
        // Field 100 px, text 400 px, 1 px caret.
        assert_eq!(scroll_to_keep_visible(0, 50, 400, 100, 1), 0);
        assert_eq!(scroll_to_keep_visible(0, 400, 400, 100, 1), 301);
        assert_eq!(scroll_to_keep_visible(301, 200, 400, 100, 1), 200);
        assert_eq!(scroll_to_keep_visible(150, 160, 400, 100, 1), 150);
        assert_eq!(scroll_to_keep_visible(900, 400, 400, 100, 1), 301);
        assert_eq!(scroll_to_keep_visible(50, 10, 60, 100, 1), 0);
        assert_eq!(scroll_to_keep_visible(5, 5, 400, 0, 1), 0);
    }

    #[ignore = "set BOWSER_DUMP_CHROME to render the offline visual QA matrix"]
    #[test]
    fn dump_visual_qa_matrix() {
        let Ok(dir) = std::env::var("BOWSER_DUMP_CHROME") else {
            return;
        };
        let dir = PathBuf::from(dir);
        fs::create_dir_all(&dir).unwrap();
        for (appearance, name) in [(Appearance::Light, "light"), (Appearance::Dark, "dark")] {
            for solid in [false, true] {
                let pal = palette(appearance, solid);
                for dpi in [96, 120, 144, 192] {
                    for width in [768, 1024, 1440] {
                        for count in [1, 5, 12] {
                            for state in 0..3 {
                                let layout =
                                    crate::chrome::layout::compute(width, 120, dpi, count, false);
                                let now = Instant::now();
                                let tabs=(0..count).map(|i|TabView{title:if i+1==count{"A remarkably long page title that should truncate elegantly"}else{"A quiet page"},loading:false,favicon:if i%2==0{Some(&QA_FAVICON)}else{None},emphasis:Emphasis::resting(i==0,i+1==count,now),group_size:None,group_control:false}).collect::<Vec<_>>();
                                let text = match state {
                                    0 => "",
                                    1 => "https://example.com/a-very-long-address",
                                    _ => "https://example.com",
                                };
                                let omni = OmniboxView {
                                    text,
                                    selection: if state == 1 { (8, 15) } else { (0, 0) },
                                    caret_visible: state == 2,
                                    focused: state == 2,
                                    completion_suffix: if state == 1 { ".net" } else { "" },
                                    progress: if state == 2 { Some(45) } else { None },
                                };
                                let view = ChromeView {
                                    layout: &layout,
                                    palette: pal,
                                    tabs: &tabs,
                                    active: count - 1,
                                    hover: Hit::Tab(0),
                                    pressed: Hit::None,
                                    caption_maximised: false,
                                    can_back: true,
                                    can_forward: false,
                                    omnibox: omni,
                                    now,
                                    departures: &[],
                                };
                                let mut surface = ChromeSurface::new(width, 120, dpi);
                                surface.paint(&view, Rect::new(0, 0, width, surface.height()));
                                let rgba = composite_on_background(
                                    surface.pixels(),
                                    width,
                                    surface.height(),
                                    surface.stride(),
                                    Some([pal.window.r, pal.window.g, pal.window.b]),
                                );
                                let file = dir.join(format!(
                                    "{name}-{}-{width}-dpi{dpi}-tabs{count}-state{state}.png",
                                    if solid { "solid" } else { "glass" }
                                ));
                                write_png(&file, width as u32, surface.height() as u32, &rgba);
                            }
                        }
                    }
                }
            }
        }
    }

    #[ignore = "set BOWSER_DUMP_MOTION to render transition frames"]
    #[test]
    fn dump_motion_frames() {
        let Ok(dir) = std::env::var("BOWSER_DUMP_MOTION") else {
            return;
        };
        let dir = PathBuf::from(dir);
        fs::create_dir_all(&dir).unwrap();
        let width = 1440;
        let layout = crate::chrome::layout::compute(width, 120, 96, 5, false);
        let tabs = (0..5)
            .map(|i| TabView {
                title: "A quiet page",
                loading: false,
                favicon: if i % 2 == 0 { Some(&QA_FAVICON) } else { None },
                emphasis: rest(),
                group_size: None,
                group_control: false,
            })
            .collect::<Vec<_>>();
        let start = Instant::now();
        let mut hovering = Emphasis::resting(false, false, start);
        hovering.retarget(true, false, start, false);
        let mut leaving = Emphasis::resting(false, true, start);
        leaving.retarget(false, false, start, false);
        let mut arriving = Emphasis::resting(false, false, start);
        arriving.retarget(false, true, start, false);
        for (frame, ms) in [0_u64, 40, 75, 150].into_iter().enumerate() {
            let now = start + std::time::Duration::from_millis(ms);
            let mut emphasis = [Emphasis::resting(false, false, start); 5];
            emphasis[2] = hovering;
            let tabs_hover = tabs
                .iter()
                .zip(emphasis)
                .map(|(tab, emphasis)| TabView { emphasis, ..*tab })
                .collect::<Vec<_>>();
            let view = ChromeView {
                layout: &layout,
                palette: palette(Appearance::Dark, false),
                tabs: &tabs_hover,
                active: 0,
                hover: Hit::None,
                pressed: Hit::None,
                caption_maximised: false,
                can_back: true,
                can_forward: false,
                omnibox: omnibox("", (0, 0), ""),
                now,
                departures: &[],
            };
            let mut surface = ChromeSurface::new(width, 120, 96);
            surface.paint(&view, Rect::new(0, 0, width, surface.height()));
            let rgba =
                composite_stripes(surface.pixels(), width, surface.height(), surface.stride());
            write_png(
                &dir.join(format!("hover-{frame}-{ms}ms.png")),
                width as u32,
                surface.height() as u32,
                &rgba,
            );
        }
        for (frame, ms) in [0_u64, 40, 75, 150].into_iter().enumerate() {
            let now = start + std::time::Duration::from_millis(ms);
            let mut emphasis = [Emphasis::resting(false, false, start); 5];
            emphasis[0] = leaving;
            emphasis[4] = arriving;
            let tabs_select = tabs
                .iter()
                .zip(emphasis)
                .map(|(tab, emphasis)| TabView { emphasis, ..*tab })
                .collect::<Vec<_>>();
            let view = ChromeView {
                layout: &layout,
                palette: palette(Appearance::Dark, false),
                tabs: &tabs_select,
                active: 4,
                hover: Hit::None,
                pressed: Hit::None,
                caption_maximised: false,
                can_back: true,
                can_forward: false,
                omnibox: omnibox("", (0, 0), ""),
                now,
                departures: &[],
            };
            let mut surface = ChromeSurface::new(width, 120, 96);
            surface.paint(&view, Rect::new(0, 0, width, surface.height()));
            let rgba =
                composite_stripes(surface.pixels(), width, surface.height(), surface.stride());
            write_png(
                &dir.join(format!("select-{frame}-{ms}ms.png")),
                width as u32,
                surface.height() as u32,
                &rgba,
            );
        }
    }

    #[ignore = "explicit performance probe; run in release mode"]
    #[test]
    fn paint_performance_probe() {
        let layout = crate::chrome::layout::compute(1000, 800, 96, 5, false);
        let tabs = vec![
            TabView {
                title: "Fast tab",
                loading: false,
                favicon: None,
                emphasis: rest(),
                group_size: None,
                group_control: false,
            };
            5
        ];
        let view = ChromeView {
            layout: &layout,
            palette: palette(Appearance::Light, false),
            tabs: &tabs,
            active: 0,
            hover: Hit::Tab(1),
            pressed: Hit::None,
            caption_maximised: false,
            can_back: true,
            can_forward: false,
            omnibox: OmniboxView {
                text: "",
                selection: (0, 0),
                caret_visible: false,
                focused: false,
                completion_suffix: "",
                progress: None,
            },
            now: Instant::now(),
            departures: &[],
        };
        let mut s = ChromeSurface::new(1000, 800, 96);
        let full_dirty = Rect::new(0, 0, 1000, s.height());
        for _ in 0..20 {
            s.paint(&view, full_dirty);
        }
        let mut samples = Vec::with_capacity(201);
        for _ in 0..201 {
            let start = Instant::now();
            s.paint(&view, full_dirty);
            samples.push(start.elapsed());
        }
        samples.sort_unstable();
        let median = samples[samples.len() / 2];
        let start = Instant::now();
        for _ in 0..100 {
            s.paint(&view, Rect::new(0, 0, 1000, s.height()));
        }
        let full = start.elapsed() / 100;
        let start = Instant::now();
        for _ in 0..1000 {
            s.paint(&view, Rect::new(90, 8, 220, 32));
        }
        let hover = start.elapsed() / 1000;
        let start = Instant::now();
        for _ in 0..200 {
            s.paint(&view, Rect::new(0, 0, 1000, 40));
        }
        let strip = start.elapsed() / 200;
        let start = Instant::now();
        for _ in 0..200 {
            s.paint(&view, Rect::new(0, 40, 1000, 92));
        }
        let omnibox = start.elapsed() / 200;
        let start = Instant::now();
        for _ in 0..1000 {
            s.paint(&view, Rect::new(100, 50, 102, 80));
        }
        let caret = start.elapsed() / 1000;
        println!(
            "median-full={median:?}; full-strip={full:?}; hover-dirty={hover:?}; strip-row={strip:?}; omnibox-row={omnibox:?}; caret-dirty={caret:?}"
        );
    }
}

#[cfg(test)]
static QA_FAVICON: std::sync::LazyLock<FaviconBitmap> =
    std::sync::LazyLock::new(|| FaviconBitmap {
        width: 2,
        height: 2,
        bgra_premultiplied: vec![
            0xA0, 0x55, 0x20, 0xFF, 0x20, 0x80, 0xDD, 0xFF, 0x40, 0xB0, 0x30, 0xFF, 0xCC, 0x55,
            0xA0, 0xFF,
        ],
    });

#[cfg(test)]
fn composite_stripes(pixels: &[u8], width: i32, height: i32, stride: usize) -> Vec<u8> {
    composite_on_background(pixels, width, height, stride, None)
}

#[cfg(test)]
fn composite_on_background(
    pixels: &[u8],
    width: i32,
    height: i32,
    stride: usize,
    background: Option<[u8; 3]>,
) -> Vec<u8> {
    let mut out = vec![0; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let t = ((x + y) / 24) % 4;
            let back = background.unwrap_or(match t {
                0 => [45_u8, 30, 220],
                1 => [220, 70, 20],
                2 => [25, 190, 30],
                _ => [220, 200, 20],
            });
            let i = y as usize * stride + x as usize * 4;
            let a = u32::from(pixels[i + 3]);
            let o = (y as usize * width as usize + x as usize) * 4;
            for c in 0..3 {
                let src = pixels[i + 2 - c];
                out[o + c] =
                    ((u32::from(src) + u32::from(back[c]) * (255 - a) / 255).min(255)) as u8;
            }
            out[o + 3] = 255;
        }
    }
    out
}

#[cfg(test)]
fn write_png(path: &std::path::Path, width: u32, height: u32, rgba: &[u8]) {
    use std::io::Write;
    let mut raw = Vec::with_capacity((width as usize * 4 + 1) * height as usize);
    for y in 0..height {
        raw.push(0);
        let a = (y * width * 4) as usize;
        raw.extend_from_slice(&rgba[a..a + width as usize * 4]);
    }
    let mut z = vec![0x78, 0x01];
    let mut at = 0;
    while at < raw.len() {
        let len = (raw.len() - at).min(65535);
        let last = at + len == raw.len();
        z.push(if last { 1 } else { 0 });
        z.extend_from_slice(&(len as u16).to_le_bytes());
        z.extend_from_slice(&(!(len as u16)).to_le_bytes());
        z.extend_from_slice(&raw[at..at + len]);
        at += len;
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    png_chunk(&mut png, *b"IHDR", &ihdr);
    png_chunk(&mut png, *b"IDAT", &z);
    png_chunk(&mut png, *b"IEND", &[]);
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(&png).unwrap();
}
#[cfg(test)]
fn png_chunk(out: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(&kind);
    out.extend_from_slice(data);
    let start = out.len() - data.len() - 4;
    out.extend_from_slice(&crc32(&out[start..]).to_be_bytes());
}
#[cfg(test)]
fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for b in data {
        c ^= u32::from(*b);
        for _ in 0..8 {
            c = (c >> 1) ^ ((0u32.wrapping_sub(c & 1)) & 0xedb88320);
        }
    }
    !c
}
#[cfg(test)]
fn adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for x in data {
        a = (a + u32::from(*x)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}
