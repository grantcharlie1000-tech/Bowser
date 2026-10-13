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
        group_ui::{GroupFrame, GroupGeometry, GroupState, group_geometry, shrink_tab},
        layout::{self, Hit, Layout, Metrics, Rect},
        motion::{Departure, Emphasis, IconFrame, Motion},
        rail::{self, RailAmounts, RailEntry, RailFrame, RailIcon, RailKey, RailMotion, RailView},
        rail_icons::{self, IconCache, Primitive},
        tab_card::Marker,
    },
    favicon::FaviconBitmap,
    site_security::SiteSecurity,
    theme::{ISLAND_COLOURS, MARKER_COLOURS, Palette, Rgba},
};

/// Warning colour for a certificate problem. The palette has no alert colour of its own.
const CERTIFICATE_WARNING: Rgba = Rgba {
    r: 0xC4,
    g: 0x2B,
    b: 0x1C,
    a: 0xFF,
};

/// Logical diameter of a tab's marker dot.
const MARKER_DOT: i32 = 8;

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
    /// Marker dot drawn over the start of the favicon area.
    pub marker: Option<Marker>,
    /// Island colour id for the group ball, drawn when this is the group leader. `None` uses the
    /// theme accent.
    pub group_colour: Option<u8>,
    /// State and animation sample for the group's shared shell and leading ball.
    pub group_frame: Option<GroupFrame>,
}

/// Colour of a group ball: the island's chosen colour, or the theme accent when none is set.
pub fn island_colour(chosen: Option<u8>, accent: Rgba) -> Rgba {
    chosen
        .and_then(|id| ISLAND_COLOURS.get(usize::from(id)).copied())
        .unwrap_or(accent)
}

fn move_rect(rect: Rect, x: i32) -> Rect {
    Rect::new(rect.left + x, rect.top, rect.right + x, rect.bottom)
}

/// State of the ghost native EDIT rendered by the chrome painter.
pub struct OmniboxView<'a> {
    pub text: &'a str,
    /// UTF-16 code-unit offsets, matching `EM_SETSEL`.
    pub selection: (usize, usize),
    pub caret_visible: bool,
    pub focused: bool,
    pub completion_suffix: &'a str,
    pub progress: Option<f32>,
    /// Connection state drawn as the icon at the left of the field.
    pub site: SiteSecurity,
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
    /// The left rail's buttons. `None` paints the empty pill, as before the rail had entries.
    pub rail: Option<RailView<'a>>,
    /// Fixed toolbar and caption icon tracks driven by the window's shared frame timer.
    pub icon_motion: Option<&'a Motion>,
}

impl ChromeView<'_> {
    fn icon_frame(&self, hit: Hit) -> IconFrame {
        self.icon_motion.map_or_else(
            || IconFrame { hover: (self.hover == hit) as u8 as f32,
                press: (self.pressed == hit) as u8 as f32, active: 0.0,
                rotation: if hit == Hit::Reload && self.pressed == hit { 0.61 } else { 0.0 } },
            |motion| motion.icon_frame(hit, self.now),
        )
    }
}

/// Reusable top-down premultiplied BGRA image for chrome and the overlaid page boundary.
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
    page_regions: Option<(Rect, Option<layout::SplitPaneLayout>, bool)>,
    /// The viewport of a tab that has no page (a new tab), painted as an opaque themed surface so
    /// it never shows the desktop through the glass. `None` while a page is on screen.
    empty_viewport: Option<Rect>,
    // --- split view ---
    /// How the divider grip is drawn, and the empty side's pane when no HTML surface covers it.
    split_grip: SplitGrip,
    split_empty: [Option<Rect>; 2],
    // --- end split view ---
    /// Press and active motion of the left rail's buttons, advanced as the rail paints.
    rail_motion: RailMotion,
    /// The rail icons' outlines, built once and reused by every frame.
    rail_icons: IconCache,
    /// The HTML chrome draws the toolbar row. Its back, forward, reload, omnibox and main menu are
    /// not painted here, and its region is left transparent so the page shows through.
    html_toolbar: bool,
    /// The HTML chrome draws the tab row. Its tabs, islands and group balls are not painted here,
    /// and its region is left transparent. The strip, captions and "+" stay native.
    html_tabs: bool,
    /// The HTML chrome draws the left rail. Its pill, buttons, tooltips and fades are not painted
    /// here, and its region is left transparent. Native panels still open beside it.
    html_rail: bool,
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

// --- split view ---
/// How the split divider's grip is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SplitGrip {
    #[default]
    Idle,
    Hover,
    Drag,
}
// --- end split view ---

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
            page_regions: None,
            empty_viewport: None,
            split_grip: SplitGrip::Idle,
            split_empty: [None, None],
            rail_motion: RailMotion::new(false),
            rail_icons: IconCache::default(),
            html_toolbar: false,
            html_tabs: false,
            html_rail: false,
        }
    }

    /// Whether any rail button's press or active motion is still running at `now`. The window
    /// keeps its frame timer going while this is true.
    pub fn rail_animating(&self, now: Instant) -> bool {
        self.rail_motion.animating(now)
    }

    /// Advances the rail's press and active motion one frame. The caller repaints the rail when
    /// `repaint` is set and stops its frame timer when `running` is clear.
    pub fn rail_step(&mut self, now: Instant) -> RailFrame {
        self.rail_motion.step(now)
    }

    /// Turns rail animation off, as when Windows has client-area animation off, or back on.
    /// Returns whether a running transition was landed, so the caller can repaint the rail.
    pub fn set_rail_snap(&mut self, snap: bool, now: Instant) -> bool {
        self.rail_motion.set_snap(snap, now)
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

    /// Sets the content edges painted above the WebView2 surface.
    pub fn set_page_regions(
        &mut self,
        content: Option<(Rect, Option<layout::SplitPaneLayout>, bool)>,
    ) {
        self.page_regions = content;
    }

    // --- split view ---
    /// Sets the divider grip state and the panes that have no page yet (painted as an opaque themed
    /// surface where no HTML surface covers them). Returns whether anything changed.
    pub fn set_split_chrome(&mut self, grip: SplitGrip, empty: [Option<Rect>; 2]) -> bool {
        let changed = self.split_grip != grip || self.split_empty != empty;
        self.split_grip = grip;
        self.split_empty = empty;
        changed
    }
    // --- end split view ---

    /// Sets the viewport of the page-less (new) tab, or `None` when a page covers it. Returns whether
    /// that changed, so the caller repaints the whole viewport (to put the surface up or take it
    /// down above a page that has just appeared).
    pub fn set_empty_viewport(&mut self, viewport: Option<Rect>) -> bool {
        std::mem::replace(&mut self.empty_viewport, viewport) != viewport
    }

    /// Hands the toolbar's back, forward, reload, omnibox and main menu to the HTML chrome.
    pub fn set_html_toolbar(&mut self, on: bool) {
        self.html_toolbar = on;
    }

    /// Hands the tab row to the HTML tab strip: its tabs and group balls are not painted natively.
    pub fn set_html_tabs(&mut self, on: bool) {
        self.html_tabs = on;
    }

    /// Hands the left rail to the HTML rail: its pill and buttons are not painted natively.
    pub fn set_html_rail(&mut self, on: bool) {
        self.html_rail = on;
    }

    /// Paints a themed viewport-half target while a tab is being dragged into split view (the
    /// native chrome's drop target, and the HTML chrome's fallback before its card is ready): the
    /// half the new pane will take, inset by the page gap, with an accent wash and edge.
    pub fn paint_split_drop(
        &mut self,
        viewport: Rect,
        pane: crate::split_view::Pane,
        palette: &Palette,
        dirty: Rect,
    ) {
        let m = Metrics::new(self.dpi);
        let gap = m.px(crate::split_view::GAP_DIP);
        let mid = viewport.left + viewport.width() / 2;
        let target = match pane {
            crate::split_view::Pane::Left => {
                Rect::new(viewport.left, viewport.top, mid - gap / 2, viewport.bottom)
            }
            crate::split_view::Pane::Right => {
                Rect::new(mid + gap - gap / 2, viewport.top, viewport.right, viewport.bottom)
            }
        };
        let radius = m.px(9);
        let tint = Rgba {
            a: if palette.translucent { 58 } else { 40 },
            ..palette.omnibox_focus_border
        };
        self.round(target, radius, tint, dirty);
        self.border(target, radius, m.px(2) as f32, palette.omnibox_focus_border, dirty);
    }
    pub fn width(&self) -> i32 {
        self.width
    }
    pub fn height(&self) -> i32 {
        self.height
    }

    /// Draws the independent vertical list into the same layered surface as the toolbar.
    pub fn paint_vertical_sidebar(
        &mut self,
        layout: &crate::vertical_tabs::SidebarLayout,
        rows: &[crate::chrome::vertical_paint::SidebarRow],
        workspace: &str,
        active: crate::tabs::TabId,
        scroll: i32,
        hover_row: Option<usize>,
        html_sidebar: bool,
        dirty: Rect,
        palette: &Palette,
    ) {
        let clip = layout.sidebar.intersect(dirty);
        if clip.is_empty() {
            return;
        }
        let m = Metrics::new(self.dpi);
        // --- vertical sidebar (lane S) ---
        if html_sidebar {
            self.fill_rect(layout.handle, palette.hairline, dirty);
            return;
        }
        // --- end vertical sidebar ---
        self.fill_rect(layout.sidebar, palette.window, dirty);
        self.fill_rect(layout.handle, palette.hairline, dirty);
        self.draw_text(
            workspace,
            Rect::new(
                layout.header.left + m.px(14),
                layout.header.top,
                layout.new_tab_button().left - m.px(6),
                layout.header.bottom,
            ),
            palette.tab_text_active,
            true,
            layout.header,
            dirty,
        );
        let plus = layout.new_tab_button();
        self.round(plus, plus.width() / 2, palette.tab_hover, dirty);
        self.draw_text(
            "+",
            plus,
            palette.tab_text_active,
            false,
            layout.header,
            dirty,
        );

        for (index, row) in rows.iter().enumerate() {
            let bounds = crate::chrome::vertical_paint::row_bounds(layout, index, m, scroll);
            if bounds.bottom <= layout.rows.top || bounds.top >= layout.rows.bottom {
                continue;
            }
            match row {
                crate::chrome::vertical_paint::SidebarRow::Group { name, colour, .. } => {
                    let Some(geometry) =
                        crate::chrome::vertical_paint::header_ball(layout, rows, index, scroll, m)
                    else {
                        continue;
                    };
                    let accent = island_colour(*colour, palette.tab_group_accent);
                    let brighten = if hover_row == Some(index) { 0.12 } else { 0.0 };
                    self.paint_ball_stack(&geometry, accent, brighten, dirty);
                    let label_left = crate::chrome::vertical_paint::header_label_left(&geometry, m);
                    self.draw_text(
                        name,
                        Rect::new(
                            label_left,
                            bounds.top,
                            bounds.right - m.px(8),
                            bounds.bottom,
                        ),
                        palette.tab_text_active,
                        true,
                        layout.rows,
                        dirty,
                    );
                }
                crate::chrome::vertical_paint::SidebarRow::Tab {
                    id,
                    title,
                    pinned,
                    loading,
                    grouped,
                } => {
                    let bounds = if *grouped {
                        crate::chrome::vertical_paint::member_bounds(bounds, m)
                    } else {
                        bounds
                    };
                    let selected = *id == active;
                    let fill = if selected {
                        palette.tab_active
                    } else if hover_row == Some(index) {
                        palette.tab_hover
                    } else {
                        palette.tab_inactive
                    };
                    self.round(bounds, m.px(7), fill, dirty);
                    if selected {
                        self.border(bounds, m.px(7), 1.0, palette.tab_active_border, dirty);
                    }
                    let mut label = String::new();
                    if *pinned {
                        label.push('◆');
                        label.push(' ');
                    }
                    label.push_str(title);
                    self.draw_text(
                        &label,
                        Rect::new(
                            bounds.left + m.px(10),
                            bounds.top,
                            bounds.right - m.px(28),
                            bounds.bottom,
                        ),
                        if selected {
                            palette.tab_text_active
                        } else {
                            palette.tab_text_inactive
                        },
                        true,
                        layout.rows,
                        dirty,
                    );
                    if *loading {
                        let progress = Rect::new(
                            bounds.left + m.px(10),
                            bounds.bottom - m.px(2),
                            bounds.right - m.px(10),
                            bounds.bottom - m.px(1),
                        );
                        self.fill_rect(progress, palette.progress_fill, dirty);
                    }
                    if hover_row == Some(index) {
                        self.draw_text(
                            "×",
                            Rect::new(
                                bounds.right - m.px(24),
                                bounds.top,
                                bounds.right - m.px(3),
                                bounds.bottom,
                            ),
                            palette.tab_text_active,
                            false,
                            layout.rows,
                            dirty,
                        );
                    }
                }
            }
        }
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
            self.fill_rect(
                Rect::new(
                    0,
                    0,
                    self.width,
                    layout::chrome_height(view.layout.metrics.dpi()),
                ),
                view.palette.window,
                dirty,
            );
        }
        // Each HTML region is cleared here: the overlay sits above the HTML chrome's child window, so
        // the page shows through. The native paint of that region is then skipped.
        if self.html_toolbar {
            self.clear_html_region(crate::chrome_host::toolbar_bounds(view.layout).map(Rect::from), dirty);
        }
        if self.html_tabs {
            self.clear_html_region(crate::chrome_host::tab_row_bounds(view.layout).map(Rect::from), dirty);
        }
        if self.html_rail {
            self.clear_html_region(crate::chrome_host::rail_bounds(view.layout).map(Rect::from), dirty);
        } else {
            self.paint_left_rail(view, dirty);
        }
        if !self.html_tabs {
            self.paint_departures(view, dirty);
            self.paint_tabs(view, dirty);
        }
        self.paint_tab_menu(view, dirty);
        self.paint_new_tab(view, dirty);
        if !self.html_toolbar {
            self.paint_main_menu(view, dirty);
            self.paint_nav(view, dirty);
        }
        self.paint_caption(view, dirty);
        if !self.html_toolbar {
            self.paint_omnibox(view, dirty);
        }
        self.paint_page_boundary(view, dirty);
        self.paint_empty_viewport(view, dirty);
    }

    /// The new-tab surface: the window colour made opaque (the glass tint alone would let the
    /// desktop through), inside the same rounded edge a page gets.
    fn paint_empty_viewport(&mut self, view: &ChromeView<'_>, dirty: Rect) {
        let Some(viewport) = self.empty_viewport else {
            return;
        };
        let radius = view.layout.metrics.px(9);
        let fill = Rgba { a: 255, ..view.palette.window };
        self.round(viewport, radius, fill, dirty);
        self.border(viewport, radius, 1.0, view.palette.omnibox_border, dirty);
    }

    fn paint_page_boundary(&mut self, view: &ChromeView<'_>, dirty: Rect) {
        let Some((content, panes, focused_left)) = self.page_regions else {
            return;
        };
        let m = view.layout.metrics;
        let radius = m.px(9);
        let colour = view.palette.omnibox_border;
        if let Some(panes) = panes {
            // --- split view ---
            // Two separate rounded pages with a gap between them (Chrome's split): every corner
            // is rounded, the gap takes the window colour like the margins, a pill grip marks the
            // divider and the focused page carries a 2 DIP accent edge.
            self.fill_rect(panes.divider, view.palette.window, dirty);
            self.mask_rounded(view.palette.window, panes.left, radius, dirty, true, true);
            self.mask_rounded(view.palette.window, panes.right, radius, dirty, true, true);
            for empty in self.split_empty.into_iter().flatten() {
                self.round(empty, radius, Rgba { a: 255, ..view.palette.window }, dirty);
            }
            let (focus, other) = if focused_left {
                (panes.left, panes.right)
            } else {
                (panes.right, panes.left)
            };
            self.border(other, radius, 1.0, colour, dirty);
            self.border(focus, radius, m.px(2) as f32, view.palette.omnibox_focus_border, dirty);
            let grip_w = m.px(4).min(panes.divider.width()).max(1);
            let grip_h = m.px(32).min(panes.divider.height()).max(1);
            let x = panes.divider.left + (panes.divider.width() - grip_w) / 2;
            let y = panes.divider.top + (panes.divider.height() - grip_h) / 2;
            let grip = Rect::new(x, y, x + grip_w, y + grip_h);
            let text = view.palette.tab_text_active;
            let fill = match self.split_grip {
                SplitGrip::Idle => Rgba { a: 56, ..text },
                SplitGrip::Hover => Rgba { a: 153, ..text },
                SplitGrip::Drag => view.palette.omnibox_focus_border,
            };
            self.round(grip, grip_w / 2, fill, dirty);
            // --- end split view ---
        } else {
            self.mask_rounded(view.palette.window, content, radius, dirty, true, true);
            self.border(content, radius, 1.0, colour, dirty);
        }
    }

    /// Covers only the pixels outside the page's rounded outer corners. The WebView remains a
    /// rectangular controller; this small themed mask is painted in the owned chrome overlay.
    fn mask_rounded(
        &mut self,
        colour: Rgba,
        rect: Rect,
        radius: i32,
        dirty: Rect,
        round_left: bool,
        round_right: bool,
    ) {
        let radius = radius.min(rect.width() / 2).min(rect.height() / 2).max(0);
        if radius == 0 {
            return;
        }
        let corners = [
            (rect.left, rect.top, round_left, true),
            (rect.right - radius, rect.top, round_right, true),
            (rect.left, rect.bottom - radius, round_left, false),
            (
                rect.right - radius,
                rect.bottom - radius,
                round_right,
                false,
            ),
        ];
        let stride = self.stride();
        let width = self.width;
        let height = self.height;
        let pixels = self.dib.pixels_mut();
        for (left, top, enabled, top_corner) in corners {
            if !enabled {
                continue;
            }
            let corner = Rect::new(left, top, left + radius, top + radius).intersect(dirty);
            let (cx, cy) = match (left == rect.left, top_corner) {
                (true, true) => (left + radius, top + radius),
                (false, true) => (left, top + radius),
                (true, false) => (left + radius, top),
                (false, false) => (left, top),
            };
            for y in corner.top.max(0)..corner.bottom.min(height) {
                for x in corner.left.max(0)..corner.right.min(width) {
                    let dx = x - cx;
                    let dy = y - cy;
                    if dx * dx + dy * dy > radius * radius {
                        let index = y as usize * stride + x as usize * 4;
                        blend_pixel(&mut pixels[index..index + 4], colour, 255);
                    }
                }
            }
        }
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

    /// Rounded fill with separate left and right corner radii.
    fn round_sides(&mut self, rect: Rect, left: i32, right: i32, colour: Rgba, dirty: Rect) {
        let (width, height, stride) = (self.width, self.height, self.stride());
        raster_rounded_rect_sides(
            self.dib.pixels_mut(),
            width,
            height,
            stride,
            rect,
            (left as f32, right as f32),
            colour,
            dirty,
        );
    }

    /// Rounded outline with separate left and right corner radii.
    fn border_sides(
        &mut self,
        rect: Rect,
        left: i32,
        right: i32,
        thickness: f32,
        colour: Rgba,
        dirty: Rect,
    ) {
        let (width, height, stride) = (self.width, self.height, self.stride());
        raster_round_border_sides(
            self.dib.pixels_mut(),
            width,
            height,
            stride,
            rect,
            (left as f32, right as f32),
            thickness,
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

    /// The left rail: the pill, then, when the app supplies a rail view, its buttons and section
    /// hairlines on top. Without a view the pill is painted alone.
    /// Zeroes the pixels of an HTML region where it meets `dirty`, so the overlay is transparent
    /// there and the HTML chrome shows through. `None` (no region) does nothing.
    fn clear_html_region(&mut self, region: Option<Rect>, dirty: Rect) {
        let Some(region) = region else {
            return;
        };
        let region = region.intersect(dirty);
        let stride = self.stride();
        let pixels = self.dib.pixels_mut();
        for y in region.top..region.bottom {
            let start = y as usize * stride + region.left as usize * 4;
            pixels[start..start + region.width() as usize * 4].fill(0);
        }
    }

    fn paint_left_rail(&mut self, view: &ChromeView<'_>, dirty: Rect) {
        let pill = view.layout.rail;
        let radius = view.layout.metrics.tab_radius();
        self.round(pill, radius, view.palette.tab_active, dirty);
        let stroke = if view.palette.translucent { 0.25 } else { 1.0 };
        let colour = scale_alpha(view.palette.tab_active_border, stroke);
        self.border(pill, radius, 1.0, colour, dirty);
        let Some(supplied) = view.rail else { return };
        let m = view.layout.metrics;
        let pressed = match view.pressed {
            Hit::Rail(key) => Some(key),
            _ => None,
        };
        self.rail_motion
            .sync(supplied.entries, pressed, supplied.open_panel, view.now);
        let geometry = rail::geometry(pill, supplied.entries, supplied.scroll, m.dpi());
        for rule in &geometry.rules {
            self.fill_rect(rule.visible(), view.palette.hairline, dirty);
        }
        for item in &geometry.items {
            let Some(entry) = supplied.entries.get(item.index) else {
                continue;
            };
            let clip = item.visible().intersect(dirty);
            if clip.is_empty() {
                continue;
            }
            let row = item.clip.intersect(dirty);
            self.paint_rail_button(view, &supplied, entry, item.rect, clip, row);
        }
        if let Some(focused) = supplied.focused
            && let Some(item) = geometry.item(focused)
        {
            self.border(
                item.rect,
                m.px(rail::BUTTON_RADIUS),
                1.5,
                view.palette.omnibox_focus_border,
                item.visible().intersect(dirty),
            );
        }
        if let Some(indicator) = supplied.drop_indicator {
            let x = pill.left + m.px(4);
            self.fill_rect(
                Rect::new(x, indicator.y - m.px(1), pill.right - m.px(4), indicator.y + m.px(1)),
                view.palette.progress_fill,
                dirty,
            );
        }
        if let Some(key) = supplied.tooltip
            && let Some(item) = geometry.item(key)
        {
            let label = rail::rail_tooltip(key, supplied.entries);
            let length = encode_into(&mut self.label_units, &label);
            let text_width = self.text.measure(&self.label_units[..length], length);
            let left = pill.right + m.px(7);
            let max_top = (self.height - m.px(28)).max(m.px(4));
            let top = (item.rect.top + (item.rect.height() - m.px(24)) / 2)
                .clamp(m.px(4).min(max_top), max_top);
            let bounds = Rect::new(left, top, (left + text_width + m.px(18)).min(self.width - m.px(4)), top + m.px(24));
            self.round(bounds, m.px(6), view.palette.tab_active, dirty);
            self.border(bounds, m.px(6), 1.0, view.palette.omnibox_border, dirty);
            self.draw_text(
                &label,
                Rect::new(bounds.left + m.px(9), bounds.top, bounds.right - m.px(6), bounds.bottom),
                view.palette.tab_text_active,
                true,
                bounds,
                dirty,
            );
        }
    }

    /// One rail button: its hover, press and open-panel fills, the active bar, the corner dot, and
    /// the icon, favicon or letter tile. `clip` is the visible part of the button within the dirty
    /// rect. `row` is the part of the rail the button may show in, which the active bar may use
    /// even though it sits left of the button.
    fn paint_rail_button(
        &mut self,
        view: &ChromeView<'_>,
        rail: &RailView<'_>,
        entry: &RailEntry<'_>,
        button: Rect,
        clip: Rect,
        row: Rect,
    ) {
        let palette = view.palette;
        let m = view.layout.metrics;
        let key = entry.key;
        let radius = m.px(rail::BUTTON_RADIUS);
        let open = rail.open_panel == Some(key);
        let pressed = view.pressed == Hit::Rail(key);
        let accent = entry.accent.unwrap_or(palette.tab_group_accent);
        let amounts = self
            .rail_motion
            .amounts(key, rail_hover_amount(view, rail, key), view.now);

        // The plate: an open panel holds it, a press lands it at once and lets it go as the press
        // eases out, and the pointer's hover fills it in between.
        if open {
            self.round(button, radius, palette.tab_active, clip);
        } else {
            if !pressed && amounts.hover > 0.0 {
                self.round(button, radius, scale_alpha(palette.tab_hover, amounts.hover), clip);
            }
            if amounts.press > 0.0 {
                self.round(button, radius, scale_alpha(palette.tab_active, amounts.press), clip);
            }
        }
        if open {
            self.border(button, radius, 1.0, palette.tab_active_border, clip);
        }
        // An open panel lights the icon but not the bar, which marks the current workspace or app.
        let bar_amount = if open && !entry.active { 0.0 } else { amounts.active };
        if bar_amount > 0.0 {
            let bar_width = m.px(rail::ACTIVE_BAR_WIDTH);
            let bar_scale = RailAmounts { active: bar_amount, ..amounts }.bar_scale();
            let grown = (m.px(rail::ACTIVE_BAR_HEIGHT) as f32 * bar_scale).round() as i32;
            let bar_height = grown.max(bar_width);
            let bar_left = view.layout.rail.left + m.px(1);
            let top = button.top + (button.height() - bar_height) / 2;
            let bar = Rect::new(bar_left, top, bar_left + bar_width, top + bar_height);
            self.round(bar, bar_width / 2, scale_alpha(accent, bar_amount), row);
        }

        let ink = mix(palette.tab_text_inactive, palette.tab_text_active, amounts.active);
        if let Some(image) = entry.favicon {
            // A favicon is a bitmap, so it only sinks under a press and does not lift on hover.
            let scale = RailAmounts { hover: 0.0, ..amounts }.icon_scale();
            let size = (m.px(Metrics::FAVICON_SIZE) as f32 * scale).round() as i32;
            self.blit_favicon(rail::centred(button, size.max(1)), image, clip);
        } else if entry.icon == RailIcon::Letter {
            let size = (m.px(rail::TILE) as f32 * amounts.icon_scale()).round() as i32;
            let tile = rail::centred(button, size.max(1));
            self.round(tile, m.px(rail::TILE_RADIUS), accent, clip);
            if let Some(letter) = entry.label.trim().chars().next() {
                let text = letter.to_uppercase().collect::<String>();
                self.draw_centred(&text, tile, Rgba::hex(0xFFFFFF, 0xFF), clip);
            }
        } else {
            // A workspace glyph already wears its colour, so its draw-on is a lighter pass of it.
            let (colour, trace) = match entry.icon {
                RailIcon::Glyph(_) if entry.accent.is_some() => {
                    (accent, mix(accent, palette.tab_text_active, 0.4))
                }
                _ => (ink, accent),
            };
            let square = rail::centred(button, m.px(rail::ICON));
            self.paint_rail_icon(entry.icon, square, colour, trace, amounts, clip);
        }

        if entry.dot {
            let size = m.px(rail::DOT) as f32;
            let centre_x = (button.right - m.px(6)) as f32;
            let centre_y = (button.top + m.px(6)) as f32;
            self.line_f(centre_x, centre_y, centre_x, centre_y, size, accent, clip);
        }
    }

    /// Strokes an outlined rail icon into `square`, which is the icon's 24-unit grid scaled to fit.
    /// The pen is the rail's logical stroke width at this surface's dpi. The icon scales about the
    /// square's centre with hover and press, and draws itself on in `trace` as it becomes active:
    /// strokes the trace has covered are drawn in `trace` alone, and the stroke it is crossing
    /// shows both. Outlines come from the cache, so a frame allocates nothing.
    fn paint_rail_icon(
        &mut self,
        icon: RailIcon,
        square: Rect,
        colour: Rgba,
        trace: Rgba,
        amounts: RailAmounts,
        clip: Rect,
    ) {
        let mut cache = std::mem::take(&mut self.rail_icons);
        let cached = cache.get(icon);
        let pen = rail::STROKE * self.dpi as f32 / 96.0 * amounts.pen_scale();
        let scale = square.width() as f32 / rail_icons::GRID * amounts.icon_scale();
        let left = (square.left + square.right) as f32 / 2.0 - rail_icons::GRID * scale / 2.0;
        let top = (square.top + square.bottom) as f32 / 2.0 - rail_icons::GRID * scale / 2.0;
        let at = |(x, y): (f32, f32)| (left + x * scale, top + y * scale);
        let covered = amounts.active.clamp(0.0, 1.0) * cached.span;
        let mut offset = 0.0;
        for primitive in &cached.primitives {
            let span = primitive.span();
            let drawn = rail_icons::traced_fraction(offset, span, covered);
            offset += span;
            match *primitive {
                Primitive::Segment { from, to } => {
                    let (x0, y0) = at(from);
                    let (x1, y1) = at(to);
                    if drawn < 1.0 {
                        self.line_f(x0, y0, x1, y1, pen, colour, clip);
                    }
                    if drawn > 0.0 {
                        let (x, y) = (x0 + (x1 - x0) * drawn, y0 + (y1 - y0) * drawn);
                        self.line_f(x0, y0, x, y, pen, trace, clip);
                    }
                }
                Primitive::Dot { centre, radius } => {
                    let (x, y) = at(centre);
                    if drawn < 1.0 {
                        self.line_f(x, y, x, y, 2.0 * radius * scale, colour, clip);
                    }
                    if drawn > 0.0 {
                        self.line_f(x, y, x, y, 2.0 * radius * scale * drawn, trace, clip);
                    }
                }
            }
        }
        self.rail_icons = cache;
    }

    /// One line of text centred in `rect`, clipped to `clip`.
    fn draw_centred(&mut self, value: &str, rect: Rect, colour: Rgba, clip: Rect) {
        let length = encode_into(&mut self.label_units, value);
        self.text.select_font();
        let width = self.text.measure(&self.label_units[..length], length);
        let left = rect.left + (rect.width() - width).max(0) / 2;
        let placed = Rect::new(left, rect.top, rect.right, rect.bottom);
        self.draw_text(value, placed, colour, false, clip, clip);
    }

    /// Closed tabs fade out and shrink slightly on the island they left. They take no input.
    fn paint_departures(&mut self, view: &ChromeView<'_>, dirty: Rect) {
        let m = view.layout.metrics;
        let radius = m.tab_radius();
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
        let visible_start = view.layout.first_visible_tab();
        let visible_end = visible_start + view.layout.tabs().count();
        let mut group_spans: Vec<(usize, usize, GroupGeometry)> = Vec::new();
        let mut index = visible_start;
        while index < visible_end {
            if !view
                .tabs
                .get(index)
                .is_some_and(|tab| tab.group_size.is_some())
            {
                index += 1;
                continue;
            }
            let start = index;
            index += 1;
            while index < visible_end
                && view
                    .tabs
                    .get(index)
                    .is_some_and(|tab| tab.group_size.is_some() && !tab.group_control)
            {
                index += 1;
            }
            let Some(first) = view.layout.tab(start) else {
                continue;
            };
            let Some(last) = view.layout.tab(index - 1) else {
                continue;
            };
            let shell = Rect::new(first.left, first.top, last.right, last.bottom)
                .intersect(view.layout.tab_zone);
            if !shell.is_empty() {
                let details = view.tabs.get(start);
                let frame = details.and_then(|tab| tab.group_frame).unwrap_or_else(|| {
                    GroupFrame::resting(
                        shell,
                        GroupState::Expanded,
                        details.and_then(|tab| tab.group_size).unwrap_or(1),
                    )
                });
                group_spans.push((start, index, group_geometry(frame, m)));
            }
        }

        // A group has one shared shell; each member remains 5% shorter inside it.
        let group_fill = mix(view.palette.tab_inactive, view.palette.tab_active, 0.30);
        for (start, end, geometry) in &group_spans {
            let shell = geometry.shell.intersect(view.layout.tab_zone);
            self.round_sides(
                shell,
                geometry.left_radius,
                geometry.right_radius,
                group_fill,
                dirty,
            );
            self.border_sides(
                shell,
                geometry.left_radius,
                geometry.right_radius,
                1.0,
                scale_alpha(view.palette.hairline, 0.8),
                dirty,
            );
            for member in *start + 1..*end {
                if geometry.shell.width() <= geometry.left_radius + geometry.right_radius {
                    break;
                }
                let Some(previous) = view.layout.tab(member - 1) else {
                    continue;
                };
                let Some(next) = view.layout.tab(member) else {
                    continue;
                };
                let x = (previous.right + next.left) / 2;
                let divider = Rect::new(
                    x,
                    shell.top + m.px(5),
                    x + m.px(1).max(1),
                    shell.bottom - m.px(5),
                );
                self.fill_rect(divider, view.palette.hairline, dirty);
            }
        }

        for (slot, tab_rect) in view.layout.tabs().enumerate() {
            let index = view.layout.first_visible_tab() + slot;
            let active = index == view.active;
            let details = view.tabs.get(index);
            let span = group_spans
                .iter()
                .find(|(start, end, _)| *start <= index && index < *end);
            let geometry = span.map(|(_, _, geometry)| geometry);
            let grouped = details.is_some_and(|tab| tab.group_size.is_some());
            let group_leader = details.is_some_and(|tab| tab.group_control)
                || span.is_some_and(|(start, _, _)| *start == index);
            let group_frame = details.and_then(|tab| tab.group_frame);
            let target_state = group_frame.map_or(GroupState::Expanded, |frame| frame.state);

            if grouped && target_state == GroupState::CollapsedWithoutSelection {
                if group_leader && let Some(geometry) = geometry {
                    let colour = island_colour(
                        details.and_then(|tab| tab.group_colour),
                        view.palette.tab_group_accent,
                    );
                    self.paint_group_balls(index, geometry, colour, view, dirty);
                }
                continue;
            }

            let base_rect = if grouped {
                shrink_tab(tab_rect, m)
            } else {
                tab_rect
            };
            let slide_x = match (group_frame, span) {
                (Some(frame), Some((start, _, geometry)))
                    if frame.state == GroupState::Expanded
                        && frame.from != GroupState::Expanded
                        && index != *start =>
                {
                    let source = geometry
                        .ball_rects
                        .first()
                        .map_or(geometry.shell.left, |ball| ball.right + m.px(4));
                    source - base_rect.left
                }
                _ => 0,
            };
            let draw_rect = move_rect(base_rect, slide_x).intersect(view.layout.tab_zone);
            if draw_rect.is_empty() || draw_rect.intersect(dirty).is_empty() {
                continue;
            }
            let reveal = match (group_frame, span) {
                (Some(frame), Some((start, _, _)))
                    if frame.state == GroupState::Expanded
                        && frame.from != GroupState::Expanded
                        && index != *start =>
                {
                    frame.progress
                }
                _ => 1.0,
            };
            let tab_opacity = reveal.clamp(0.0, 1.0);
            let (hover, selected) = details.map_or((0.0, 0.0), |t| t.emphasis.amounts(view.now));
            let fill = scale_alpha(
                mix(
                    mix(view.palette.tab_inactive, view.palette.tab_hover, hover),
                    view.palette.tab_active,
                    selected,
                ),
                tab_opacity,
            );
            let radius = m.tab_radius();
            if grouped {
                if hover > 0.0 || selected > 0.0 {
                    let face_left = if group_leader {
                        geometry.map_or(draw_rect.left, |g| g.shell.left + g.content_inset)
                    } else {
                        draw_rect.left
                    };
                    let selected_rect = Rect::new(
                        face_left,
                        draw_rect.top + m.px(1),
                        draw_rect.right - m.px(1),
                        draw_rect.bottom - m.px(1),
                    );
                    self.round_sides(selected_rect, m.px(6), m.px(6), fill, dirty);
                }
            } else {
                self.round_sides(draw_rect, radius, radius, fill, dirty);
                self.border_sides(
                    draw_rect,
                    radius,
                    radius,
                    1.0,
                    scale_alpha(view.palette.hairline, 0.62 * tab_opacity),
                    dirty,
                );
            }
            if selected > 0.0 && !grouped {
                // Translucent palettes keep only a faint outline on the selected island, so it
                // reads as a soft fill rather than a framed box.
                let stroke = if view.palette.translucent { 0.25 } else { 1.0 };
                let colour = scale_alpha(view.palette.tab_active_border, selected * stroke);
                self.border_sides(draw_rect, radius, radius, 1.0, colour, dirty);
            }
            let content_inset = if group_leader {
                geometry.map_or(0, |g| g.content_inset)
            } else {
                0
            };
            let mut parts = if grouped {
                view.layout.group_tab_parts(tab_rect, content_inset)
            } else {
                view.layout.tab_parts(tab_rect)
            };
            parts.favicon = move_rect(parts.favicon, slide_x);
            parts.title = move_rect(parts.title, slide_x);
            parts.close = move_rect(parts.close, slide_x);

            if group_leader && let Some(geometry) = geometry {
                let colour = island_colour(
                    details.and_then(|tab| tab.group_colour),
                    view.palette.tab_group_accent,
                );
                self.paint_group_balls(index, geometry, colour, view, dirty);
            }
            if !parts.title.is_empty() {
                // The favicon and title sit centred in the tab, as a group.
                let label = details.map_or("New tab", |value| value.title);
                let length = encode_into(&mut self.label_units, label);
                let text_width = self.text.measure(&self.label_units[..length], length);
                let gap = m.px(Metrics::TEXT_GAP);
                let shown = text_width.min(parts.title.width());
                let group = parts.favicon.width() + gap + shown;
                let area_left = parts.favicon.left;
                let area_right = parts.title.right;
                let start = ((area_left + area_right - group) / 2).max(area_left);
                let shift = start - area_left;
                parts.favicon.left += shift;
                parts.favicon.right += shift;
                parts.title.left += shift;
            }
            // A part that would be cut off by the tab zone is skipped, so a half-visible close
            // glyph never lands on a neighbouring control.
            let narrow_active_close = active && parts.active_close;
            if draw_rect.contains_rect(parts.favicon) && !narrow_active_close {
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
                // The marker sits on the favicon's top-left corner, so it never moves the title.
                if let Some(marker) = details.and_then(|value| value.marker) {
                    let size = m.px(MARKER_DOT).max(4);
                    let inset = size / 4;
                    let dot = Rect::new(
                        parts.favicon.left - inset,
                        parts.favicon.top - inset,
                        parts.favicon.left - inset + size,
                        parts.favicon.top - inset + size,
                    );
                    if draw_rect.contains_rect(dot) {
                        self.round(
                            dot,
                            size / 2,
                            MARKER_COLOURS[usize::from(marker.id())],
                            dirty,
                        );
                    }
                }
            }
            if !parts.title.is_empty() {
                let title = details.map_or("New tab", |value| value.title);
                let color = mix(
                    view.palette.tab_text_inactive,
                    view.palette.tab_text_active,
                    selected,
                );
                self.draw_text(
                    title,
                    parts.title,
                    scale_alpha(color, tab_opacity),
                    true,
                    draw_rect,
                    dirty,
                );
            }
            let close_rect = if narrow_active_close {
                parts.favicon
            } else {
                parts.close
            };
            if close_rect.is_empty() || !draw_rect.contains_rect(close_rect) {
                continue;
            }
            let close_hover = !parts.active_close
                && matches!(view.hover, Hit::Tab(i) | Hit::TabClose(i) if i == index);
            let close_pressed = matches!(view.pressed, Hit::TabClose(i) if i == index);
            if !narrow_active_close && !close_hover && !close_pressed {
                continue;
            }
            if close_hover || close_pressed {
                self.round(
                    close_rect,
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
            let c = mix(c, view.palette.progress_fill, close_pressed as u8 as f32);
            let (cx, cy) = center(close_rect);
            let hover_amount = details.map_or(0.0, |tab| tab.emphasis.amounts(view.now).0);
            let scale = m.px(16) as f32 / 24.0
                * if close_pressed { 0.96 } else { 1.0 + 0.06 * hover_amount };
            let half = 6.0 * scale;
            let (cx, cy) = (cx as f32, cy as f32);
            self.line_f(cx - half, cy - half, cx + half, cy + half, 1.5 * scale, c, dirty);
            self.line_f(cx - half, cy + half, cx + half, cy - half, 1.5 * scale, c, dirty);
        }
    }

    fn paint_group_balls(
        &mut self,
        index: usize,
        geometry: &GroupGeometry,
        colour: Rgba,
        view: &ChromeView<'_>,
        dirty: Rect,
    ) {
        let hovered = view.hover == Hit::GroupBall(index);
        let pressed = view.pressed == Hit::GroupBall(index);
        let brighten = if pressed {
            0.22
        } else if hovered {
            0.12
        } else {
            0.0
        };
        self.paint_ball_stack(geometry, colour, brighten, dirty);
    }

    /// Draws a group's fanned balls, leading ball last so it stays on top. `brighten` lifts the
    /// base colour toward white for hover and press. The vertical sidebar reuses this too.
    pub(crate) fn paint_ball_stack(
        &mut self,
        geometry: &GroupGeometry,
        colour: Rgba,
        brighten: f32,
        dirty: Rect,
    ) {
        for ball_index in (0..geometry.ball_rects.len()).rev() {
            let ball = geometry.ball_rects[ball_index];
            if ball.intersect(dirty).is_empty() {
                continue;
            }
            let alpha = geometry.ball_opacity[ball_index];
            let base = scale_alpha(mix(colour, Rgba::hex(0xFFFFFF, colour.a), brighten), alpha);
            self.round(ball, ball.height() / 2, base, dirty);

            // A compact upper-left glint gives the coloured control a spherical read without a
            // constant animated sheen or an idle repaint.
            let highlight_width = (ball.width() / 3).max(2);
            let highlight_height = (ball.height() / 5).max(2);
            let highlight = Rect::new(
                ball.left + ball.width() / 5,
                ball.top + ball.height() / 6,
                ball.left + ball.width() / 5 + highlight_width,
                ball.top + ball.height() / 6 + highlight_height,
            );
            self.round(
                highlight,
                highlight.height() / 2,
                scale_alpha(Rgba::hex(0xFFFFFF, 150), alpha * (0.42 + brighten)),
                dirty,
            );
            if ball_index + 1 == geometry.ball_rects.len()
                && let Some(overflow) = geometry.overflow_count
            {
                let label = format!("+{overflow}");
                let luminance =
                    u16::from(colour.r) * 3 + u16::from(colour.g) * 6 + u16::from(colour.b);
                let text = if luminance > 1275 {
                    Rgba::hex(0x20242A, 245)
                } else {
                    Rgba::hex(0xFFFFFF, 245)
                };
                self.draw_text(&label, ball, scale_alpha(text, alpha), false, ball, dirty);
            }
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
        let frame = view.icon_frame(Hit::NewTab);
        let c = mix(view.palette.caption_glyph, view.palette.progress_fill, frame.press);
        let scale = view.layout.metrics.px(16) as f32 / 24.0 * frame.scale();
        let arm = 8.0 * scale;
        let (x, y) = (x as f32, y as f32);
        self.line_f(x - arm, y, x + arm, y, 1.5 * scale, c, dirty);
        self.line_f(x, y - arm, x, y + arm, 1.5 * scale, c, dirty);
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
        let frame = view.icon_frame(Hit::TabMenu);
        let c = mix(view.palette.caption_glyph, view.palette.progress_fill, frame.press);
        let scale = view.layout.metrics.px(16) as f32 / 24.0 * frame.scale();
        let (x, y) = (x as f32, y as f32);
        self.line_f(x - 7.0 * scale, y - 3.0 * scale, x, y + 4.0 * scale, 1.5 * scale, c, dirty);
        self.line_f(x, y + 4.0 * scale, x + 7.0 * scale, y - 3.0 * scale, 1.5 * scale, c, dirty);
    }

    /// A three-line menu glyph in a bubble right of the address pill; the main menu opens from
    /// here. The bubble has the back / forward pill's fill and outline.
    fn paint_main_menu(&mut self, view: &ChromeView<'_>, dirty: Rect) {
        let rect = view.layout.main_menu_button;
        if rect.intersect(dirty).is_empty() {
            return;
        }
        let m = view.layout.metrics;
        let radius = m.island_radius();
        self.round(rect, radius, view.palette.tab_inactive, dirty);
        if view.hover == Hit::MainMenu || view.pressed == Hit::MainMenu {
            let colour = if view.pressed == Hit::MainMenu {
                view.palette.tab_active_border
            } else {
                view.palette.caption_hover
            };
            self.round(rect, radius, colour, dirty);
        }
        self.border(
            rect,
            radius,
            m.dpi() as f32 / 96.0,
            view.palette.hairline,
            dirty,
        );
        let frame = view.icon_frame(Hit::MainMenu);
        let colour = mix(view.palette.caption_glyph, view.palette.progress_fill, frame.press);
        let cx = (rect.left + rect.right) as f32 * 0.5;
        let cy = (rect.top + rect.bottom) as f32 * 0.5;
        let amount = frame.scale();
        for (left, right, y) in main_menu_bars(rect, m) {
            self.line_f(cx + (left - cx) * amount, cy + (y - cy) * amount,
                cx + (right - cx) * amount, cy + (y - cy) * amount,
                NAV_STROKE, colour, dirty);
        }
    }

    /// Back, forward and reload. Back and forward are dimmed and inert when there is no history.
    fn paint_nav(&mut self, view: &ChromeView<'_>, dirty: Rect) {
        let m = view.layout.metrics;
        let back = view.layout.back_button;
        let forward = view.layout.forward_button;
        let pill = Rect::new(back.left, back.top, forward.right, back.bottom);
        if !pill.intersect(dirty).is_empty() {
            self.round(pill, m.island_radius(), view.palette.tab_inactive, dirty);
            self.border(pill, m.island_radius(), 1.0, view.palette.hairline, dirty);
            if view.hover == Hit::Back || view.pressed == Hit::Back {
                self.round(back, m.px(7), view.palette.caption_hover, dirty);
            } else if view.hover == Hit::Forward || view.pressed == Hit::Forward {
                self.round(forward, m.px(7), view.palette.caption_hover, dirty);
            }
            let divider_x = forward.left;
            self.fill_rect(
                Rect::new(
                    divider_x,
                    pill.top + m.px(7),
                    divider_x + 1,
                    pill.bottom - m.px(7),
                ),
                view.palette.hairline,
                dirty,
            );
        }
        let buttons = [
            (Hit::Back, view.layout.back_button, view.can_back),
            (Hit::Forward, view.layout.forward_button, view.can_forward),
            (Hit::Reload, view.layout.reload_button, true),
        ];
        for (hit, rect, enabled) in buttons {
            if rect.intersect(dirty).is_empty() {
                continue;
            }
            let frame = if enabled { view.icon_frame(hit) } else { IconFrame::default() };
            let mut color = mix(view.palette.caption_glyph,
                view.palette.progress_fill, frame.press);
            if enabled {
                if hit == Hit::Reload && (view.hover == hit || view.pressed == hit) {
                    self.round(rect, m.island_radius(), view.palette.caption_hover, dirty);
                }
            } else {
                color.a = (u32::from(color.a) * 0x55 / 0xFF) as u8;
            }
            let (cx, _) = center(rect);
            let cy = (view.layout.strip.top + view.layout.strip.height() / 2) as f32;
            let cx = cx as f32;
            let scale = m.px(20) as f32 / 24.0 * frame.scale();
            let nudge = match hit {
                Hit::Back => -m.px(2) as f32 * frame.press,
                Hit::Forward => m.px(2) as f32 * frame.press,
                _ => 0.0,
            };
            let rotation = frame.rotation;
            let point = |x: f32, y: f32| {
                let dx = (x - 12.0) * scale;
                let dy = (y - 12.0) * scale;
                (cx + nudge + dx * rotation.cos() - dy * rotation.sin(),
                    cy + dx * rotation.sin() + dy * rotation.cos())
            };
            let w = 1.5 * scale;
            match hit {
                Hit::Back | Hit::Forward => {
                    let (tip, tail, barb) = if hit == Hit::Back {
                        (4.0, 20.0, 10.0)
                    } else {
                        (20.0, 4.0, 14.0)
                    };
                    let (x0, y0) = point(tail, 12.0);
                    let (x1, y1) = point(tip, 12.0);
                    self.line_f(x0, y0, x1, y1, w, color, dirty);
                    for y in [6.0, 18.0] {
                        let (x2, y2) = point(barb, y);
                        self.line_f(x1, y1, x2, y2, w, color, dirty);
                    }
                }
                _ => {
                    // The same 24-grid circular arrow used by Reset.
                    const SEGMENTS: usize = 20;
                    let mut previous = point(20.0, 12.0);
                    for step in 1..=SEGMENTS {
                        let angle = 5.75 * step as f32 / SEGMENTS as f32;
                        let next = point(12.0 + 8.0 * angle.cos(), 12.0 + 8.0 * angle.sin());
                        self.line_f(previous.0, previous.1, next.0, next.1, w, color, dirty);
                        previous = next;
                    }
                    let (x0, y0) = point(19.0, 3.0);
                    let (x1, y1) = point(19.0, 8.0);
                    let (x2, y2) = point(14.0, 8.0);
                    self.line_f(x0, y0, x1, y1, w, color, dirty);
                    self.line_f(x1, y1, x2, y2, w, color, dirty);
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
            let frame = view.icon_frame(button_hit);
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
            let base_color = if close && (hovered || pressed) {
                view.palette.close_glyph_hover
            } else {
                view.palette.caption_glyph
            };
            let color = mix(base_color, view.palette.progress_fill,
                frame.press.max(if i == 1 { frame.active } else { 0.0 }));
            let (cx, cy) = center(rect);
            let (cx, cy) = (cx as f32, cy as f32);
            let scale = m.px(20) as f32 / 24.0 * frame.scale();
            let glyph = |x: f32, y: f32| (cx + (x - 12.0) * scale, cy + (y - 12.0) * scale);
            let stroke = 1.5 * scale;
            match i {
                0 => {
                    let (x0, y0) = glyph(4.0, 12.0);
                    let (x1, y1) = glyph(20.0, 12.0);
                    self.line_f(x0, y0, x1, y1, stroke, color, dirty);
                }
                1 if view.caption_maximised => {
                    let (x0, y0) = glyph(7.0, 7.0);
                    let (x1, y1) = glyph(21.0, 21.0);
                    self.border(Rect::new(x0.round() as i32, y0.round() as i32,
                        x1.round() as i32, y1.round() as i32), m.px(2), stroke, color, dirty);
                    let (x0, y0) = glyph(4.0, 17.0);
                    let (x1, y1) = glyph(4.0, 5.0);
                    let (x2, y2) = glyph(18.0, 3.0);
                    self.line_f(x0, y0, x1, y1, stroke, color, dirty);
                    self.line_f(x1, y1, x2, y2, stroke, color, dirty);
                }
                1 => {
                    let (x0, y0) = glyph(4.0, 4.0);
                    let (x1, y1) = glyph(20.0, 20.0);
                    self.border(Rect::new(x0.round() as i32, y0.round() as i32,
                        x1.round() as i32, y1.round() as i32), m.px(2), stroke, color, dirty);
                }
                _ => {
                    let (x0, y0) = glyph(6.0, 6.0);
                    let (x1, y1) = glyph(18.0, 18.0);
                    let (x2, y2) = glyph(6.0, 18.0);
                    let (x3, y3) = glyph(18.0, 6.0);
                    self.line_f(x0, y0, x1, y1, stroke, color, dirty);
                    self.line_f(x2, y2, x3, y3, stroke, color, dirty);
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
        let left_inset = m.px(Metrics::OMNIBOX_TEXT_LEFT);
        let line = self.text.line_height();
        let top = rect.top + (rect.height() - line) / 2;
        let split_button = view.layout.split_button();
        let text_area = Rect::new(
            rect.left + left_inset,
            top,
            (split_button.left - m.px(4)).max(rect.left + left_inset),
            top + line,
        );
        let origin = text_area;
        // The connection icon at the left of the field: a magnifier stands in before any page loads.
        let icon_colour = match view.omnibox.site {
            SiteSecurity::Secure => view.palette.omnibox_text,
            SiteSecurity::CertificateProblem => CERTIFICATE_WARNING,
            _ => view.palette.omnibox_placeholder,
        };
        let button = view.layout.site_info_button();
        self.paint_site_icon(view.omnibox.site, button, icon_colour,
            view.icon_frame(Hit::SiteInfo), view.palette.progress_fill, dirty);
        self.paint_split_button(view, split_button, dirty);
        // Text, selection and caret never leave the field, however far the text is scrolled.
        let field = Rect::new(text_area.left, rect.top, text_area.right, rect.bottom);
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
            let placeholder = "Search or enter address";
            let placeholder_width = self.text.measure(
                &placeholder.encode_utf16().collect::<Vec<_>>(),
                placeholder.encode_utf16().count(),
            );
            let placeholder_origin = if view.omnibox.focused {
                origin
            } else {
                Rect::new(
                    centred_text_left(text_area.left, text_area.right, placeholder_width),
                    origin.top,
                    text_area.right,
                    origin.bottom,
                )
            };
            self.draw_text(
                placeholder,
                placeholder_origin,
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
            let text_left = if view.omnibox.focused {
                origin.left - self.omnibox_scroll
            } else {
                centred_text_left(text_area.left, text_area.right, text_width)
            };
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
            if progress > 0.0 {
                let bar_h = m.px(2).max(1);
                let bottom = rect.bottom;
                let top = bottom - bar_h;
                let width =
                    (rect.width() as f32 * progress.clamp(0.0, 100.0) / 100.0).round() as i32;
                let bar = Rect::new(rect.left, top, rect.left + width, bottom);
                let (surface_w, surface_h, stride) = (self.width, self.height, self.stride());
                let opacity = ((200.0 - progress) / 100.0).clamp(0.0, 1.0);
                for (extra, alpha) in [(m.px(4), 0.09), (m.px(2), 0.18), (0, 1.0)] {
                    let glow = Rect::new(bar.left, bar.top - extra, bar.right, bar.bottom);
                    raster_rounded_rect_clipped(
                        self.dib.pixels_mut(),
                        surface_w,
                        surface_h,
                        stride,
                        glow,
                        0.0,
                        rect,
                        radius as f32,
                        scale_alpha(view.palette.progress_fill, opacity * alpha),
                        dirty,
                    );
                }
                if (100.0..200.0).contains(&progress) {
                    let tail = m.px(14);
                    let sweep =
                        bar.left + (bar.width() as f32 * ((progress - 100.0) / 100.0)) as i32;
                    let shimmer = Rect::new(
                        (sweep - tail).max(bar.left),
                        top - m.px(2),
                        sweep.min(bar.right),
                        bottom,
                    );
                    raster_rounded_rect_clipped(
                        self.dib.pixels_mut(),
                        surface_w,
                        surface_h,
                        stride,
                        shimmer,
                        0.0,
                        rect,
                        radius as f32,
                        scale_alpha(view.palette.progress_fill, opacity * 0.22),
                        dirty,
                    );
                }
            }
        }
    }

    fn paint_split_button(&mut self, view: &ChromeView<'_>, rect: Rect, dirty: Rect) {
        if rect.intersect(dirty).is_empty() {
            return;
        }
        let m = view.layout.metrics;
        if view.hover == Hit::Split || view.pressed == Hit::Split {
            let colour = if view.pressed == Hit::Split {
                view.palette.tab_active_border
            } else {
                view.palette.caption_hover
            };
            self.round(rect, m.px(6), colour, dirty);
        }
        let inset = m.px(5).max(4);
        let left = Rect::new(
            rect.left + inset,
            rect.top + inset,
            rect.left + inset * 2,
            rect.bottom - inset,
        );
        let right = Rect::new(
            rect.right - inset * 2,
            rect.top + inset,
            rect.right - inset,
            rect.bottom - inset,
        );
        self.border(left, m.px(2), 1.0, view.palette.caption_glyph, dirty);
        self.border(right, m.px(2), 1.0, view.palette.caption_glyph, dirty);
        let (cx, _) = center(rect);
        self.line(
            cx,
            rect.top + inset,
            cx,
            rect.bottom - inset,
            1.0,
            view.palette.caption_glyph,
            dirty,
        );
    }

    /// Draws the connection icon centred in `button`, on the shared 24-unit grid: a padlock when the page is
    /// secure, an open padlock for plain HTTP, a warning sign for a certificate problem, an info mark
    /// for local or unknown pages, and a magnifier before any page is loaded.
    fn paint_site_icon(&mut self, state: SiteSecurity, button: Rect, colour: Rgba,
        frame: IconFrame, accent: Rgba, dirty: Rect) {
        const GRID: f32 = 24.0;
        const ARC_STEPS: usize = 12;
        let size = button.width() * 18 / 24;
        let (cx, cy) = center(button);
        let scale = size as f32 / GRID * frame.scale();
        let (cx, cy) = (cx as f32, cy as f32);
        let point = |x: f32, y: f32| (cx + (x - 12.0) * scale,
            cy + (y - 12.0) * scale);
        let colour = mix(colour, accent, frame.press.max(frame.active * 0.55));
        let grid_rect = |x0: f32, y0: f32, x1: f32, y1: f32| {
            let (a, b) = point(x0, y0);
            let (c, d) = point(x1, y1);
            Rect::new(
                a.round() as i32,
                b.round() as i32,
                c.round() as i32,
                d.round() as i32,
            )
        };
        let stroke = (1.5 * scale).max(1.0);
        match state {
            SiteSecurity::Secure | SiteSecurity::NotSecure => {
                let body = grid_rect(4.0, 10.0, 20.0, 21.0);
                self.border(body, (2.0 * scale).round() as i32, stroke, colour, dirty);
                let end = if state == SiteSecurity::Secure {
                    0.0
                } else {
                    std::f32::consts::PI / 3.0
                };
                let mut previous = point(7.0, 10.0);
                let next = point(7.0, 7.5);
                self.line_f(previous.0, previous.1, next.0, next.1, stroke, colour, dirty);
                previous = next;
                for step in 0..=ARC_STEPS {
                    let t = std::f32::consts::PI
                        - (std::f32::consts::PI - end) * step as f32 / ARC_STEPS as f32;
                    let next = point(12.0 + 5.0 * t.cos(), 7.5 - 5.0 * t.sin());
                    self.line_f(previous.0, previous.1, next.0, next.1, stroke, colour, dirty);
                    previous = next;
                }
                if state == SiteSecurity::Secure {
                    let next = point(17.0, 10.0);
                    self.line_f(previous.0, previous.1, next.0, next.1, stroke, colour, dirty);
                }
            }
            SiteSecurity::CertificateProblem => {
                let corners = [point(12.0, 3.0), point(21.0, 20.0), point(3.0, 20.0)];
                for index in 0..3 {
                    let ((x0, y0), (x1, y1)) = (corners[index], corners[(index + 1) % 3]);
                    self.line_f(x0, y0, x1, y1, stroke, colour, dirty);
                }
                let (x0, y0) = point(12.0, 9.0);
                let (x1, y1) = point(12.0, 14.0);
                self.line_f(x0, y0, x1, y1, stroke, colour, dirty);
                let dot = grid_rect(11.0, 16.5, 13.0, 18.5);
                self.round(dot, dot.width() / 2, colour, dirty);
            }
            SiteSecurity::Internal | SiteSecurity::Unknown => {
                let ring = grid_rect(3.0, 3.0, 21.0, 21.0);
                self.border(ring, ring.width() / 2, stroke, colour, dirty);
                let dot = grid_rect(11.0, 6.0, 13.0, 8.0);
                self.round(dot, dot.width() / 2, colour, dirty);
                let (x0, y0) = point(12.0, 11.0);
                let (x1, y1) = point(12.0, 17.0);
                self.line_f(x0, y0, x1, y1, stroke, colour, dirty);
            }
            SiteSecurity::NoSite => {
                let lens = grid_rect(4.0, 4.0, 17.0, 17.0);
                self.border(lens, lens.width() / 2, stroke, colour, dirty);
                let (x0, y0) = point(15.5, 15.5);
                let (x1, y1) = point(20.5, 20.5);
                self.line_f(x0, y0, x1, y1, stroke, colour, dirty);
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

/// Clamps a client size to the whole owned overlay and a DPI to at least 96.
fn surface_size(width: i32, height: i32, dpi: u32) -> (i32, i32, u32) {
    let dpi = dpi.max(96);
    (width.max(1), height.max(1), dpi)
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
/// Left edge that centres `text_width` between `field_left` and `field_right`.
/// Text wider than the field stays left aligned, so its start remains visible.
fn centred_text_left(field_left: i32, field_right: i32, text_width: i32) -> i32 {
    (field_left + (field_right - field_left - text_width) / 2).max(field_left)
}

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

/// Stroke width of the toolbar glyphs: back, forward, reload and the main menu.
const NAV_STROKE: f32 = 1.5;

/// The main menu glyph's three bars as `(left, right, y)` stroke centrelines, top to bottom.
///
/// Each bar sits on a pixel-row centre so it rasterises as one solid row rather than two
/// half-covered ones, and the bars share an integer pitch so all three look identical. The glyph
/// is centred horizontally; on an even-height button the middle bar takes the row just above the
/// centre line.
fn main_menu_bars(rect: Rect, m: Metrics) -> [(f32, f32, f32); 3] {
    let cx = (rect.left + rect.right) as f32 / 2.0;
    let middle = ((rect.top + rect.bottom) as f32 / 2.0 + 0.5).floor() - 0.5;
    let half = m.px(6) as f32;
    let pitch = m.px(4) as f32;
    [-1.0, 0.0, 1.0].map(|step| (cx - half, cx + half, middle + step * pitch))
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

/// How far a rail button's hover has faded in. A button still fading uses its fade, and a button the
/// pointer rests on without a fade is fully hovered.
fn rail_hover_amount(view: &ChromeView<'_>, rail: &RailView<'_>, key: RailKey) -> f32 {
    match rail.fades.iter().find(|fade| fade.key == key) {
        Some(fade) => fade.amount,
        None if view.hover == Hit::Rail(key) => 1.0,
        None => 0.0,
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

/// Signed distance to a rounded rectangle whose left and right corner pairs have their own radii.
/// Radii that cannot both fit across the width are scaled down together.
fn rounded_distance_sides(x: f32, y: f32, rect: Rect, radii: (f32, f32)) -> f32 {
    let width = rect.width() as f32;
    let half_height = (rect.height() as f32 / 2.0).max(0.0);
    let (mut left_r, mut right_r) = (radii.0.max(0.0), radii.1.max(0.0));
    let sum = left_r + right_r;
    if sum > width && sum > 0.0 {
        let k = width / sum;
        left_r *= k;
        right_r *= k;
    }
    left_r = left_r.min(half_height);
    right_r = right_r.min(half_height);
    let (left, right) = (rect.left as f32, rect.right as f32);
    let (r, cx) = if x < left + left_r {
        (left_r, left + left_r)
    } else if x > right - right_r {
        (right_r, right - right_r)
    } else {
        (0.0, x)
    };
    let qy = (y - (rect.top as f32 + half_height)).abs() - (half_height - r);
    if r <= 0.0 {
        // Between the corners only the top and bottom edges matter.
        qy
    } else {
        (x - cx).abs().hypot(qy.max(0.0)) - r
    }
}

#[allow(clippy::too_many_arguments)]
fn raster_rounded_rect_sides(
    pixels: &mut [u8],
    width: i32,
    height: i32,
    stride: usize,
    rect: Rect,
    radii: (f32, f32),
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
            let d = rounded_distance_sides(x as f32 + 0.5, y as f32 + 0.5, rect, radii);
            let cov = ((0.5 - d).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            if cov > 0 {
                let i = y as usize * stride + x as usize * 4;
                blend_pixel(&mut pixels[i..i + 4], color, cov);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn raster_round_border_sides(
    pixels: &mut [u8],
    width: i32,
    height: i32,
    stride: usize,
    rect: Rect,
    radii: (f32, f32),
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
            let d = rounded_distance_sides(x as f32 + 0.5, y as f32 + 0.5, rect, radii);
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
    fn centred_text_left_centres_text_within_field() {
        // Field 100..200 is 100 wide; a 40-wide text starts 30 in.
        assert_eq!(centred_text_left(100, 200, 40), 130);
        // Odd slack rounds down, so the text sits one pixel left of true centre.
        assert_eq!(centred_text_left(100, 200, 41), 129);
    }

    #[test]
    fn centred_text_left_falls_back_to_field_left_when_text_overflows() {
        assert_eq!(centred_text_left(100, 200, 100), 100);
        assert_eq!(centred_text_left(100, 200, 150), 100);
        assert_eq!(centred_text_left(100, 200, 500), 100);
    }

    #[test]
    fn centred_text_left_handles_empty_text() {
        assert_eq!(centred_text_left(100, 200, 0), 150);
        assert_eq!(centred_text_left(100, 100, 0), 100);
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
            marker: None,
            group_colour: None,
            group_frame: None,
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
                site: SiteSecurity::NoSite,
            },
            now: Instant::now(),
            departures: &[],
            rail: None,
            icon_motion: None,
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
            rail: None,
            icon_motion: None,
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
            site: SiteSecurity::Secure,
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
            marker: None,
            group_colour: None,
            group_frame: None,
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
            rail: None,
            icon_motion: None,
        };
        let mut surface = ChromeSurface::new(1000, 120, 96);
        surface.paint(&view, Rect::new(0, 0, 1000, surface.height()));
        surface
    }

    use crate::chrome::rail::{RailFade, WorkspaceGlyph};

    fn rail_entry(key: RailKey, icon: RailIcon) -> RailEntry<'static> {
        RailEntry {
            key,
            icon,
            label: "Work".to_owned(),
            active: false,
            dot: false,
            accent: None,
            favicon: None,
        }
    }

    /// Paints a rail of the given entries into `surface`, which is a 300 x 400 logical client.
    #[allow(clippy::too_many_arguments)]
    fn paint_rail_with(
        surface: &mut ChromeSurface,
        entries: &[RailEntry<'_>],
        dpi: u32,
        hover: Hit,
        pressed: Hit,
        fades: &[RailFade],
        open_panel: Option<RailKey>,
        dirty: Rect,
    ) {
        paint_rail_decorated(surface, entries, dpi, hover, pressed, fades, open_panel, None, None, None, dirty);
    }

    fn paint_rail_decorated(
        surface: &mut ChromeSurface,
        entries: &[RailEntry<'_>],
        dpi: u32,
        hover: Hit,
        pressed: Hit,
        fades: &[RailFade],
        open_panel: Option<RailKey>,
        focused: Option<RailKey>,
        drop_indicator: Option<crate::chrome::rail::RailDropIndicator>,
        tooltip: Option<RailKey>,
        dirty: Rect,
    ) {
        let m = Metrics::new(dpi);
        let layout = layout::compute_in_frame(m.px(300), m.px(400), dpi, 0, 0, false);
        let view = ChromeView {
            layout: &layout,
            palette: palette(Appearance::Light, false),
            tabs: &[],
            active: 0,
            hover,
            pressed,
            caption_maximised: false,
            can_back: false,
            can_forward: false,
            omnibox: omnibox("", (0, 0), ""),
            now: Instant::now(),
            departures: &[],
            rail: Some(RailView {
                entries,
                fades,
                open_panel,
                scroll: 0,
                drop_indicator,
                focused,
                tooltip,
            }),
            icon_motion: None,
        };
        surface.paint(&view, dirty);
    }

    fn paint_rail(
        entries: &[RailEntry<'_>],
        dpi: u32,
        hover: Hit,
        pressed: Hit,
        fades: &[RailFade],
        open_panel: Option<RailKey>,
    ) -> ChromeSurface {
        let m = Metrics::new(dpi);
        let (width, height) = (m.px(300), m.px(400));
        let mut surface = ChromeSurface::new(width, height, dpi);
        let whole = Rect::new(0, 0, width, height);
        paint_rail_with(
            &mut surface,
            entries,
            dpi,
            hover,
            pressed,
            fades,
            open_panel,
            whole,
        );
        surface
    }

    fn button_rect(entries: &[RailEntry<'_>], dpi: u32, key: RailKey) -> Rect {
        let m = Metrics::new(dpi);
        let layout = layout::compute_in_frame(m.px(300), m.px(400), dpi, 0, 0, false);
        rail::geometry(layout.rail, entries, 0, dpi)
            .item(key)
            .expect("the key is placed")
            .rect
    }

    /// The premultiplied BGRA bytes of `rect`, row by row.
    fn pixels_in(surface: &ChromeSurface, rect: Rect) -> Vec<u8> {
        let stride = surface.stride();
        let mut out = Vec::with_capacity(rect.width() as usize * rect.height() as usize * 4);
        for y in rect.top..rect.bottom {
            let start = y as usize * stride + rect.left as usize * 4;
            out.extend_from_slice(&surface.pixels()[start..start + rect.width() as usize * 4]);
        }
        out
    }

    fn pixel_at(surface: &ChromeSurface, x: i32, y: i32) -> [u8; 4] {
        let i = y as usize * surface.stride() + x as usize * 4;
        surface.pixels()[i..i + 4].try_into().expect("four bytes")
    }

    #[test]
    fn every_rail_icon_paints_visible_and_distinct_pixels() {
        let icons: Vec<RailIcon> = [
            RailIcon::Home,
            RailIcon::Customize,
            RailIcon::Plus,
            RailIcon::Heart,
            RailIcon::Clock,
            RailIcon::Dots,
            RailIcon::AppTile,
            RailIcon::Letter,
        ]
        .into_iter()
        .chain(WorkspaceGlyph::ALL.map(RailIcon::Glyph))
        .collect();
        let key = RailKey::Workspace(1);
        for dpi in [96, 144, 192] {
            let blank = paint_rail(&[], dpi, Hit::None, Hit::None, &[], None);
            let mut seen = Vec::new();
            for icon in icons.iter().copied() {
                let entries = [rail_entry(key, icon)];
                let rect = button_rect(&entries, dpi, key);
                let painted = paint_rail(&entries, dpi, Hit::None, Hit::None, &[], None);
                let pixels = pixels_in(&painted, rect);
                assert!(
                    pixels != pixels_in(&blank, rect),
                    "{icon:?} draws nothing at dpi {dpi}"
                );
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                std::hash::Hash::hash(&pixels, &mut hasher);
                let print = std::hash::Hasher::finish(&hasher);
                assert!(
                    !seen.contains(&print),
                    "{icon:?} draws the same pixels as an earlier icon at dpi {dpi}"
                );
                seen.push(print);
            }
        }
    }

    #[test]
    fn a_hover_fade_paints_between_rest_and_full_hover() {
        let key = RailKey::Workspace(1);
        let entries = [rail_entry(key, RailIcon::Glyph(WorkspaceGlyph::Star))];
        let rect = button_rect(&entries, 96, key);
        let rest = pixels_in(&paint_rail(&entries, 96, Hit::None, Hit::None, &[], None), rect);
        let half_fade = [RailFade { key, amount: 0.5 }];
        let half = pixels_in(
            &paint_rail(&entries, 96, Hit::None, Hit::None, &half_fade, None),
            rect,
        );
        let full = pixels_in(
            &paint_rail(&entries, 96, Hit::Rail(key), Hit::None, &[], None),
            rect,
        );
        let snapped_fade = [RailFade { key, amount: 1.0 }];
        let snapped = pixels_in(
            &paint_rail(&entries, 96, Hit::None, Hit::None, &snapped_fade, None),
            rect,
        );

        assert!(rest != half, "a half fade is visible");
        assert!(half != full, "a half fade is weaker than full hover");
        assert!(full == snapped, "a settled fade paints like a resting hover");
    }

    #[test]
    fn the_active_workspace_shows_a_bar_in_its_accent() {
        let key = RailKey::Workspace(1);
        let mut entry = rail_entry(key, RailIcon::Glyph(WorkspaceGlyph::Code));
        let rest = paint_rail(std::slice::from_ref(&entry), 96, Hit::None, Hit::None, &[], None);
        entry.active = true;
        entry.accent = Some(Rgba::hex(0x2E9E5B, 0xFF));
        let entries = [entry];
        let active = paint_rail(&entries, 96, Hit::None, Hit::None, &[], None);

        let rect = button_rect(&entries, 96, key);
        let bar_x = layout::left_rail(400, 96).left + Metrics::new(96).px(1) + 1;
        let mid_y = rect.top + rect.height() / 2;
        assert_eq!(
            pixel_at(&active, bar_x, mid_y),
            [0x5B, 0x9E, 0x2E, 0xFF],
            "the bar is the workspace accent"
        );
        assert_ne!(pixel_at(&rest, bar_x, mid_y), [0x5B, 0x9E, 0x2E, 0xFF]);
    }

    #[test]
    fn open_and_pressed_buttons_differ_from_rest() {
        let key = RailKey::History;
        let entries = [rail_entry(key, RailIcon::Clock)];
        let rect = button_rect(&entries, 96, key);
        let rest = pixels_in(&paint_rail(&entries, 96, Hit::None, Hit::None, &[], None), rect);
        let open = pixels_in(
            &paint_rail(&entries, 96, Hit::None, Hit::None, &[], Some(key)),
            rect,
        );
        let pressed = pixels_in(
            &paint_rail(&entries, 96, Hit::Rail(key), Hit::Rail(key), &[], None),
            rect,
        );
        assert!(open != rest, "an open panel is highlighted");
        assert!(pressed != rest, "a pressed button is highlighted");
        assert!(pressed != open, "press and open read differently");
    }

    /// Paints the whole 300 x 400 rail at 96 dpi at the instant `now`, into a surface that keeps its
    /// rail motion between calls, as the window's does.
    fn paint_rail_at(
        surface: &mut ChromeSurface,
        entries: &[RailEntry<'_>],
        hover: Hit,
        pressed: Hit,
        fades: &[RailFade],
        now: Instant,
    ) {
        let layout = layout::compute_in_frame(300, 400, 96, 0, 0, false);
        let view = ChromeView {
            layout: &layout,
            palette: palette(Appearance::Light, false),
            tabs: &[],
            active: 0,
            hover,
            pressed,
            caption_maximised: false,
            can_back: false,
            can_forward: false,
            omnibox: omnibox("", (0, 0), ""),
            now,
            departures: &[],
            rail: Some(RailView {
                entries,
                fades,
                open_panel: None,
                scroll: 0,
                drop_indicator: None,
                focused: None,
                tooltip: None,
            }),
            icon_motion: None,
        };
        surface.paint(&view, Rect::new(0, 0, 300, 400));
    }

    fn at_ms(start: Instant, ms: u64) -> Instant {
        start + std::time::Duration::from_millis(ms)
    }

    /// The area of ink inside the button, in pixels, away from its rounded corners. Each pixel's
    /// distance from the button's own plate colour is taken as a share of the largest distance, so
    /// plates that differ between states (rest, hover, press) still compare fairly: a fully inked
    /// pixel counts one and an antialiased edge counts its coverage.
    fn ink_mass(surface: &ChromeSurface, button: Rect) -> f32 {
        let plate = pixel_at(surface, button.left + 4, button.top + button.height() / 2);
        let mut distances = Vec::new();
        for y in button.top + 5..button.bottom - 5 {
            for x in button.left + 5..button.right - 5 {
                let p = pixel_at(surface, x, y);
                distances.push((0..4).map(|i| u32::from(p[i].abs_diff(plate[i]))).sum::<u32>());
            }
        }
        let full = distances.iter().copied().max().unwrap_or(0).max(1) as f32;
        distances.iter().map(|&d| d as f32 / full).sum()
    }

    /// Pixels inside the button that are the solid accent.
    fn accent_pixels(surface: &ChromeSurface, button: Rect, accent: Rgba) -> usize {
        let mut count = 0;
        for y in button.top + 5..button.bottom - 5 {
            for x in button.left + 5..button.right - 5 {
                let p = pixel_at(surface, x, y);
                let near = |have: u8, want: u8| have.abs_diff(want) <= 6;
                if near(p[0], accent.b) && near(p[1], accent.g) && near(p[2], accent.r) {
                    count += 1;
                }
            }
        }
        count
    }

    #[test]
    fn a_press_sinks_the_icon_at_once_and_eases_back_out() {
        let key = RailKey::Home;
        let entries = [rail_entry(key, RailIcon::Plus)];
        let button = button_rect(&entries, 96, key);
        let start = Instant::now();
        let mut surface = ChromeSurface::new(300, 400, 96);

        paint_rail_at(&mut surface, &entries, Hit::None, Hit::None, &[], start);
        let rest = ink_mass(&surface, button);
        let rest_pixels = pixels_in(&surface, button);

        let down = at_ms(start, 20);
        paint_rail_at(&mut surface, &entries, Hit::Rail(key), Hit::Rail(key), &[], down);
        let pressed = ink_mass(&surface, button);
        assert!(pressed < rest, "the icon is smaller on the very frame of the press");
        assert!(!surface.rail_animating(down), "an instant press needs no timer");

        let up = at_ms(start, 120);
        paint_rail_at(&mut surface, &entries, Hit::Rail(key), Hit::None, &[], up);
        assert!(surface.rail_animating(up), "the release eases back out");
        paint_rail_at(&mut surface, &entries, Hit::None, Hit::None, &[], at_ms(up, 70));
        let mid = ink_mass(&surface, button);
        assert!(pressed < mid && mid < rest, "mid-release is between sunk and rest: {pressed} < {mid} < {rest}");

        let settled = at_ms(up, 300);
        paint_rail_at(&mut surface, &entries, Hit::None, Hit::None, &[], settled);
        assert!(!surface.rail_animating(settled));
        assert!(pixels_in(&surface, button) == rest_pixels, "settled pixels equal the rest pixels");
    }

    #[test]
    fn hover_lifts_the_icon_and_weights_its_stroke() {
        let key = RailKey::Home;
        let entries = [rail_entry(key, RailIcon::Plus)];
        let button = button_rect(&entries, 96, key);
        let now = Instant::now();
        let mut rest = ChromeSurface::new(300, 400, 96);
        let mut hovered = ChromeSurface::new(300, 400, 96);
        paint_rail_at(&mut rest, &entries, Hit::None, Hit::None, &[], now);
        let full = [RailFade { key, amount: 1.0 }];
        paint_rail_at(&mut hovered, &entries, Hit::Rail(key), Hit::None, &full, now);
        assert!(
            ink_mass(&hovered, button) > ink_mass(&rest, button),
            "a hovered icon is bigger and heavier"
        );
    }

    #[test]
    fn an_activated_button_draws_itself_on_in_its_accent_then_settles() {
        let key = RailKey::Home;
        let mut entry = rail_entry(key, RailIcon::Home);
        let button = button_rect(std::slice::from_ref(&entry), 96, key);
        let accent = palette(Appearance::Light, false).tab_group_accent;
        let start = Instant::now();
        let mut surface = ChromeSurface::new(300, 400, 96);

        paint_rail_at(&mut surface, std::slice::from_ref(&entry), Hit::None, Hit::None, &[], start);
        assert_eq!(accent_pixels(&surface, button, accent), 0, "idle has no accent stroke");

        entry.active = true;
        let entries = [entry];
        let on = at_ms(start, 50);
        let mut counts = Vec::new();
        for offset in [0, 45, 90, 135, 400] {
            paint_rail_at(&mut surface, &entries, Hit::None, Hit::None, &[], at_ms(on, offset));
            counts.push(accent_pixels(&surface, button, accent));
        }
        assert_eq!(counts[0], 0, "the draw-on starts from nothing");
        assert!(counts.windows(2).all(|pair| pair[0] <= pair[1]), "it only grows: {counts:?}");
        assert!(counts[2] > 0 && counts[2] < counts[4], "it is part-drawn midway: {counts:?}");
        assert!(!surface.rail_animating(at_ms(on, 400)), "settled");

        let mut fresh = ChromeSurface::new(300, 400, 96);
        paint_rail_at(&mut fresh, &entries, Hit::None, Hit::None, &[], start);
        assert!(
            pixels_in(&surface, button) == pixels_in(&fresh, button),
            "a settled draw-on paints like an icon that was already active"
        );
    }

    #[test]
    fn deactivating_undraws_and_the_bar_shrinks_away() {
        let key = RailKey::Workspace(1);
        let mut entry = rail_entry(key, RailIcon::Glyph(WorkspaceGlyph::Code));
        entry.active = true;
        entry.accent = Some(Rgba::hex(0x2E9E5B, 0xFF));
        let button = button_rect(std::slice::from_ref(&entry), 96, key);
        let bar_x = layout::left_rail(400, 96).left + Metrics::new(96).px(1) + 1;
        let (bar_top, bar_bottom) = (button.top + 2, button.bottom - 2);
        let bar_rows = |surface: &ChromeSurface| {
            (bar_top..bar_bottom)
                .filter(|&y| pixel_at(surface, bar_x, y) == [0x5B, 0x9E, 0x2E, 0xFF])
                .count()
        };
        let start = Instant::now();
        let mut surface = ChromeSurface::new(300, 400, 96);
        paint_rail_at(&mut surface, std::slice::from_ref(&entry), Hit::None, Hit::None, &[], start);
        let full = bar_rows(&surface);
        assert!(full > 0);

        entry.active = false;
        let entries = [entry];
        let off = at_ms(start, 30);
        paint_rail_at(&mut surface, &entries, Hit::None, Hit::None, &[], off);
        assert_eq!(bar_rows(&surface), full, "it leaves from the full bar");
        paint_rail_at(&mut surface, &entries, Hit::None, Hit::None, &[], at_ms(off, 60));
        assert!(bar_rows(&surface) < full, "the bar is shrinking");
        paint_rail_at(&mut surface, &entries, Hit::None, Hit::None, &[], at_ms(off, 300));
        assert_eq!(bar_rows(&surface), 0, "and is gone once settled");
        assert!(!surface.rail_animating(at_ms(off, 300)));
    }

    #[test]
    fn reduced_motion_paints_press_and_active_end_states_without_a_frame() {
        let key = RailKey::Home;
        let mut entry = rail_entry(key, RailIcon::Home);
        let button = button_rect(std::slice::from_ref(&entry), 96, key);
        let start = Instant::now();
        let mut snapped = ChromeSurface::new(300, 400, 96);
        let _ = snapped.set_rail_snap(true, start);
        paint_rail_at(&mut snapped, std::slice::from_ref(&entry), Hit::None, Hit::None, &[], start);

        entry.active = true;
        let entries = [entry];
        let later = at_ms(start, 1);
        paint_rail_at(&mut snapped, &entries, Hit::Rail(key), Hit::Rail(key), &[], later);
        assert!(!snapped.rail_animating(later), "no frame timer is requested");
        let frame = snapped.rail_step(later);
        assert!(!frame.running && !frame.repaint);

        let mut settled = ChromeSurface::new(300, 400, 96);
        paint_rail_at(&mut settled, &entries, Hit::Rail(key), Hit::Rail(key), &[], start);
        assert!(
            pixels_in(&snapped, button) == pixels_in(&settled, button),
            "the very next frame is already the end state"
        );
    }

    #[test]
    fn the_rail_stops_asking_for_frames_once_press_and_active_settle() {
        let key = RailKey::Home;
        let mut entry = rail_entry(key, RailIcon::Home);
        let start = Instant::now();
        let mut surface = ChromeSurface::new(300, 400, 96);
        paint_rail_at(&mut surface, std::slice::from_ref(&entry), Hit::None, Hit::None, &[], start);
        assert!(!surface.rail_animating(start), "an idle rail has no timer");

        entry.active = true;
        let on = at_ms(start, 10);
        paint_rail_at(&mut surface, &[entry], Hit::None, Hit::None, &[], on);
        assert!(surface.rail_animating(on));
        let mut now = on;
        let mut frames = 0;
        loop {
            now = at_ms(now, 16);
            let frame = surface.rail_step(now);
            if !frame.running {
                assert!(frame.repaint, "the landing frame repaints");
                break;
            }
            frames += 1;
            assert!(frames < 30, "the timer must stop");
        }
        assert!(!surface.rail_animating(now));
    }

    #[test]
    fn rail_focus_drop_line_and_tooltip_are_painted() {
        use crate::chrome::rail::RailDropIndicator;
        let key = RailKey::Workspace(1);
        let entries = [rail_entry(key, RailIcon::Glyph(WorkspaceGlyph::Star))];
        let m = Metrics::new(96);
        let layout = layout::compute_in_frame(m.px(300), m.px(400), 96, 0, 0, false);
        let rect = button_rect(&entries, 96, key);
        let whole = Rect::new(0, 0, m.px(300), m.px(400));
        let mut base = ChromeSurface::new(whole.right, whole.bottom, 96);
        let mut decorated = ChromeSurface::new(whole.right, whole.bottom, 96);
        paint_rail_with(&mut base, &entries, 96, Hit::None, Hit::None, &[], None, whole);
        paint_rail_decorated(
            &mut decorated,
            &entries,
            96,
            Hit::None,
            Hit::None,
            &[],
            None,
            Some(key),
            Some(RailDropIndicator { y: rect.bottom }),
            Some(key),
            whole,
        );
        assert_ne!(pixels_in(&base, rect), pixels_in(&decorated, rect), "the focus ring changes the button pixels");
        let indicator_x = layout.rail.left + 8;
        assert_ne!(pixel_at(&base, indicator_x, rect.bottom), pixel_at(&decorated, indicator_x, rect.bottom));
        assert_ne!(base.pixels(), decorated.pixels(), "the tooltip paints outside the button");
    }

    #[test]
    fn repainting_only_a_buttons_dirty_rect_matches_a_full_paint() {
        let key = RailKey::History;
        let entries = [rail_entry(key, RailIcon::Clock)];
        let m = Metrics::new(144);
        let (width, height) = (m.px(300), m.px(400));
        let layout = layout::compute_in_frame(width, height, 144, 0, 0, false);
        let dirty = rail::geometry(layout.rail, &entries, 0, 144).dirty_for(&[key]);
        let rect = button_rect(&entries, 144, key);
        assert!(dirty.contains_rect(rect), "the dirty rect covers the whole button");

        let full = paint_rail(&entries, 144, Hit::Rail(key), Hit::None, &[], None);
        let mut incremental = ChromeSurface::new(width, height, 144);
        let whole = Rect::new(0, 0, width, height);
        paint_rail_with(
            &mut incremental,
            &entries,
            144,
            Hit::None,
            Hit::None,
            &[],
            None,
            whole,
        );
        paint_rail_with(
            &mut incremental,
            &entries,
            144,
            Hit::Rail(key),
            Hit::None,
            &[],
            None,
            dirty,
        );
        assert!(
            pixels_in(&incremental, rect) == pixels_in(&full, rect),
            "the hover repaints exactly the button"
        );
    }

    #[test]
    fn a_favicon_replaces_the_letter_tile() {
        let key = RailKey::App(4);
        let favicon = crate::favicon::FaviconBitmap {
            width: 2,
            height: 2,
            bgra_premultiplied: vec![0x20, 0x40, 0x80, 0xFF].repeat(4),
        };
        let letter = [rail_entry(key, RailIcon::Letter)];
        let with_favicon = [RailEntry {
            favicon: Some(&favicon),
            ..rail_entry(key, RailIcon::Letter)
        }];
        let rect = button_rect(&letter, 96, key);
        let tile = pixels_in(&paint_rail(&letter, 96, Hit::None, Hit::None, &[], None), rect);
        let icon = pixels_in(
            &paint_rail(&with_favicon, 96, Hit::None, Hit::None, &[], None),
            rect,
        );
        assert!(tile != icon, "a favicon is drawn in place of the letter");
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
        // Sample the fill near the leading edge, clear of the title centered in the tab.
        let (x, y) = (island.left + 12, (island.top + island.bottom) / 2);
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
        // Avoid the centered title so this tracks only the fading tab fill.
        let (x, y) = (island.left + 12, (island.top + island.bottom) / 2);
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
            marker: None,
            group_colour: None,
            group_frame: None,
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
            marker: None,
            group_colour: None,
            group_frame: None,
        }; 2];
        let mut surface = ChromeSurface::new(1000, 700, 96);
        let dirty = Rect::new(0, 0, 1000, surface.height());
        let glass = palette(Appearance::Dark, false);
        surface.paint(
            &test_view(&layout, &tabs, glass, omnibox("", (0, 0), "")),
            dirty,
        );
        // The toolbar is intentionally transparent outside controls. Check its left margin,
        // the gap between navigation buttons, the row gap, and the margin below the tab row.
        let nav_gap_x = (layout.forward_button.right + layout.reload_button.left) / 2;
        for (x, y) in [
            (0, 20),
            (
                nav_gap_x,
                (layout.back_button.top + layout.back_button.bottom) / 2,
            ),
            (500, layout.strip.bottom - 1),
            (500, layout.tab_row.bottom),
        ] {
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
        let chrome = layout::chrome_height(layout.metrics.dpi());
        for y in 0..chrome {
            for x in 0..surface.width() {
                assert_eq!(alpha_at(&surface, x, y), 255, "solid pixel ({x},{y})");
            }
        }
        for y in chrome..surface.height() {
            for x in 0..surface.width() {
                assert_eq!(
                    alpha_at(&surface, x, y),
                    0,
                    "transparent page overlay pixel ({x},{y})"
                );
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
            marker: None,
            group_colour: None,
            group_frame: None,
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
        let right_edge = layout.split_button().left - layout.metrics.px(4);
        let inset = o.right - right_edge;
        let right_margin = (right_edge + 1)..(o.right - 4);
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
                marker: None,
                group_colour: None,
                group_frame: None,
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
            for y in layout.tab_row.top..layout.tab_row.bottom {
                assert_eq!(
                    alpha_at(&surface, x, y),
                    0,
                    "pixel ({x},{y}) between tabs and the tab list button"
                );
            }
        }
    }

    #[test]
    fn tab_close_glyph_appears_when_hovering_the_tab_body() {
        let layout = layout::compute(768, 700, 96, 1, false);
        let tabs = [TabView {
            title: "A tab with a title",
            loading: false,
            favicon: None,
            emphasis: rest(),
            group_size: None,
            group_control: false,
            marker: None,
            group_colour: None,
            group_frame: None,
        }];
        let palette = palette(Appearance::Dark, false);
        let dirty = Rect::new(0, 0, 768, 64);
        let mut resting = ChromeSurface::new(768, 64, 96);
        let mut hovered = ChromeSurface::new(768, 64, 96);
        let resting_view = test_view(&layout, &tabs, palette, omnibox("", (0, 0), ""));
        let mut hovered_view = test_view(&layout, &tabs, palette, omnibox("", (0, 0), ""));
        hovered_view.hover = Hit::Tab(0);

        resting.paint(&resting_view, dirty);
        hovered.paint(&hovered_view, dirty);

        let close = layout.tab_parts(layout.tab(0).unwrap()).close;
        let differs = (close.top..close.bottom).any(|y| {
            let start = y as usize * resting.stride() + close.left as usize * 4;
            let end = y as usize * resting.stride() + close.right as usize * 4;
            resting.pixels()[start..end] != hovered.pixels()[start..end]
        });
        assert!(differs, "hovering a tab body should reveal its close glyph");
    }

    #[test]
    fn compact_active_tab_draws_close_in_the_favicon_slot() {
        let layout = layout::compute(768, 700, 96, 40, false);
        let tabs = [TabView {
            title: "A tab with a title",
            loading: false,
            favicon: None,
            emphasis: rest(),
            group_size: None,
            group_control: false,
            marker: None,
            group_colour: None,
            group_frame: None,
        }; 40];
        let palette = palette(Appearance::Dark, false);
        let mut resting = ChromeSurface::new(768, 64, 96);
        let view = test_view(&layout, &tabs, palette, omnibox("", (0, 0), ""));
        resting.paint(&view, Rect::new(0, 0, 768, 64));
        let parts = layout.tab_parts(layout.tab(0).unwrap());
        assert!(parts.active_close);
        assert!(parts.close.is_empty());
        assert!(
            alpha_at(
                &resting,
                (parts.favicon.left + parts.favicon.right) / 2,
                (parts.favicon.top + parts.favicon.bottom) / 2,
            ) > 0,
            "the active narrow tab draws the close glyph over its favicon slot"
        );
    }

    #[test]
    fn grouped_tabs_share_a_shell_and_show_the_leading_ball() {
        let layout = layout::compute(1024, 700, 96, 3, false);
        let first = layout.tab(0).unwrap();
        let second = layout.tab(1).unwrap();
        let shell = Rect::new(first.left, first.top, second.right, first.bottom);
        let frame = GroupFrame::resting(shell, GroupState::Expanded, 2);
        let tabs = [
            TabView {
                title: "Get Started | Docker",
                loading: false,
                favicon: None,
                emphasis: rest(),
                group_size: Some(2),
                group_control: true,
                marker: None,
                group_colour: None,
                group_frame: Some(frame),
            },
            TabView {
                title: "Install Docker Desktop",
                loading: false,
                favicon: None,
                emphasis: rest(),
                group_size: Some(2),
                group_control: false,
                marker: None,
                group_colour: None,
                group_frame: None,
            },
            TabView {
                title: "Opera",
                loading: false,
                favicon: None,
                emphasis: rest(),
                group_size: None,
                group_control: false,
                marker: None,
                group_colour: None,
                group_frame: None,
            },
        ];
        let palette = palette(Appearance::Dark, false);
        let mut surface = ChromeSurface::new(1024, 64, 96);
        surface.paint(
            &test_view(&layout, &tabs, palette, omnibox("", (0, 0), "")),
            Rect::new(0, 0, 1024, 64),
        );

        let x_in_join = first.right + 1;
        let y = (first.top + first.bottom) / 2;
        assert!(
            alpha_at(&surface, x_in_join, y) > 0,
            "the shared group shell fills the gap between member tabs"
        );

        let geometry = group_geometry(frame, Metrics::new(96));
        let ball = geometry.ball_rects[0];
        let accent_x = (ball.left + ball.right) / 2;
        let accent_y = (ball.top + ball.bottom) / 2;
        let pixel = accent_y as usize * surface.stride() + accent_x as usize * 4;
        assert!(
            surface.pixels()[pixel + 2] > surface.pixels()[pixel],
            "the group ball uses the warm red accent"
        );
        assert_eq!(
            shrink_tab(first, Metrics::new(96)).height(),
            (first.height() * 95 + 50) / 100
        );
        assert!(second.left > first.right);
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

    #[test]
    fn main_menu_bars_are_pixel_aligned_centred_and_inside_the_bubble() {
        for dpi in [96u32, 120, 144, 168, 192, 288] {
            let layout = crate::chrome::layout::compute(1024, 700, dpi, 1, false);
            let rect = layout.main_menu_button;
            assert!(!rect.is_empty(), "dpi {dpi}");
            let bars = main_menu_bars(rect, layout.metrics);
            let centre_x = (rect.left + rect.right) as f32 / 2.0;
            let centre_y = (rect.top + rect.bottom) as f32 / 2.0;
            let pitch = bars[1].2 - bars[0].2;
            assert_eq!(pitch, bars[2].2 - bars[1].2, "equal spacing at dpi {dpi}");
            assert_eq!(pitch.fract(), 0.0, "integer pitch at dpi {dpi}");
            assert!(pitch >= NAV_STROKE + 2.0, "bars stay distinct at dpi {dpi}");
            assert!(
                (bars[1].2 - centre_y).abs() <= 0.5,
                "middle bar centred at dpi {dpi}"
            );
            for (left, right, y) in bars {
                assert_eq!(y.fract(), 0.5, "bar on a pixel-row centre at dpi {dpi}");
                assert_eq!((left + right) / 2.0, centre_x, "centred at dpi {dpi}");
                assert_eq!(
                    right - left,
                    bars[0].1 - bars[0].0,
                    "equal bars at dpi {dpi}"
                );
                // Stroke and round caps keep clear of the outline.
                let reach = NAV_STROKE / 2.0 + 2.0;
                assert!(left - reach >= rect.left as f32, "dpi {dpi}");
                assert!(right + reach <= rect.right as f32, "dpi {dpi}");
                assert!(y - reach >= rect.top as f32, "dpi {dpi}");
                assert!(y + reach <= rect.bottom as f32, "dpi {dpi}");
            }
            // The glyph is wider than tall, like the arrows beside it.
            assert!(bars[0].1 - bars[0].0 > bars[2].2 - bars[0].2, "dpi {dpi}");
        }
    }

    #[test]
    fn main_menu_paints_three_identical_solid_bars() {
        let pal = palette(Appearance::Dark, true);
        for dpi in [96u32, 144] {
            let layout = crate::chrome::layout::compute(1024, 700, dpi, 1, false);
            let tabs = [TabView {
                title: "Tab",
                loading: false,
                favicon: None,
                emphasis: rest(),
                group_size: None,
                group_control: false,
                marker: None,
                group_colour: None,
                group_frame: None,
            }];
            let view = test_view(&layout, &tabs, pal, omnibox("", (0, 0), ""));
            let mut surface = ChromeSurface::new(1024, 700, dpi);
            surface.paint(&view, Rect::new(0, 0, 1024, surface.height()));
            let stride = surface.stride();
            let pixels = surface.pixels();
            let at = |x: i32, y: i32| {
                let i = y as usize * stride + x as usize * 4;
                [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
            };
            let bars = main_menu_bars(layout.main_menu_button, layout.metrics);
            // Sample a quarter of the way along, clear of the centre column and the caps.
            let x = ((bars[0].0 * 3.0 + bars[0].1) / 4.0) as i32;
            let rows = bars.map(|(_, _, y)| y.floor() as i32);
            let ink = at(x, rows[0]);
            assert_eq!(at(x, rows[1]), ink, "dpi {dpi}");
            assert_eq!(at(x, rows[2]), ink, "dpi {dpi}");
            let gap = (rows[0] + rows[1]) / 2;
            assert_ne!(
                at(x, gap),
                ink,
                "gap between bars is unpainted at dpi {dpi}"
            );
        }
    }

    #[ignore = "set BOWSER_DUMP_MENU_BUTTON to render the main menu button states"]
    #[test]
    fn dump_main_menu_button() {
        let Ok(dir) = std::env::var("BOWSER_DUMP_MENU_BUTTON") else {
            return;
        };
        let dir = PathBuf::from(dir);
        fs::create_dir_all(&dir).unwrap();
        let tabs = [TabView {
            title: "Tab",
            loading: false,
            favicon: None,
            emphasis: rest(),
            group_size: None,
            group_control: false,
            marker: None,
            group_colour: None,
            group_frame: None,
        }];
        for (appearance, name) in [(Appearance::Light, "light"), (Appearance::Dark, "dark")] {
            for solid in [false, true] {
                let pal = palette(appearance, solid);
                for dpi in [96u32, 120, 144, 192] {
                    let layout = crate::chrome::layout::compute(1024, 700, dpi, 1, false);
                    // Crop from the reload button through the menu bubble, one strip per state.
                    let menu = layout.main_menu_button;
                    let nav = layout.forward_button;
                    let crops = [
                        Rect::new(nav.left - 4, menu.top - 4, nav.right + 4, menu.bottom + 4),
                        Rect::new(menu.left - 4, menu.top - 4, menu.right + 4, menu.bottom + 4),
                    ];
                    let scale = 6usize;
                    let states = [
                        (Hit::None, Hit::None),
                        (Hit::MainMenu, Hit::None),
                        (Hit::MainMenu, Hit::MainMenu),
                    ];
                    let crop_w: i32 =
                        crops.iter().map(|crop| crop.width()).sum::<i32>() * states.len() as i32;
                    let crop_h = crops[0].height();
                    let (out_w, out_h) = (crop_w as usize * scale, crop_h as usize * scale);
                    let mut out = vec![0u8; out_w * out_h * 4];
                    let mut origin = 0usize;
                    for (hover, pressed) in states {
                        let mut view = test_view(&layout, &tabs, pal, omnibox("", (0, 0), ""));
                        view.hover = hover;
                        view.pressed = pressed;
                        let mut surface = ChromeSurface::new(1024, 700, dpi);
                        surface.paint(&view, Rect::new(0, 0, 1024, surface.height()));
                        let rgba = composite_on_background(
                            surface.pixels(),
                            1024,
                            surface.height(),
                            surface.stride(),
                            Some([pal.window.r, pal.window.g, pal.window.b]),
                        );
                        for crop in crops {
                            for y in 0..crop_h as usize * scale {
                                for x in 0..crop.width() as usize * scale {
                                    let sx = crop.left as usize + x / scale;
                                    let sy = crop.top as usize + y / scale;
                                    let src = (sy * 1024 + sx) * 4;
                                    let dst = (y * out_w + origin + x) * 4;
                                    out[dst..dst + 4].copy_from_slice(&rgba[src..src + 4]);
                                }
                            }
                            origin += crop.width() as usize * scale;
                        }
                    }
                    write_png(
                        &dir.join(format!(
                            "{name}-{}-dpi{dpi}.png",
                            if solid { "solid" } else { "glass" }
                        )),
                        out_w as u32,
                        out_h as u32,
                        &out,
                    );
                }
            }
        }
    }

    #[ignore = "set BOWSER_DUMP_GROUPS to render the group ball state scenarios"]
    #[test]
    fn dump_group_ball_scenarios() {
        let Ok(dir) = std::env::var("BOWSER_DUMP_GROUPS") else {
            return;
        };
        let dir = PathBuf::from(dir);
        fs::create_dir_all(&dir).unwrap();
        let pal = palette(Appearance::Dark, true);
        for dpi in [96u32, 144] {
            for scenario in 0..3 {
                let now = Instant::now();
                let view_tab = |title: &'static str, size: Option<usize>, control: bool| TabView {
                    title,
                    loading: false,
                    favicon: Some(&QA_FAVICON),
                    emphasis: Emphasis::resting(false, false, now),
                    group_size: size,
                    group_control: control,
                    marker: None,
                    group_colour: Some(0),
                    group_frame: None,
                };
                let (mut tabs, group_index, pill_slots, state, member_count) = match scenario {
                    0 => (
                        vec![
                            view_tab("Get Started", Some(2), true),
                            view_tab("Install Docker", Some(2), false),
                            view_tab("Plain tab", None, false),
                        ],
                        0,
                        [false, false, false],
                        GroupState::Expanded,
                        2,
                    ),
                    1 => (
                        vec![
                            view_tab("Plain tab", None, false),
                            view_tab("Hidden group", Some(7), true),
                            view_tab("Other tab", None, false),
                        ],
                        1,
                        [false, true, false],
                        GroupState::CollapsedWithoutSelection,
                        7,
                    ),
                    _ => (
                        vec![
                            view_tab("Get Started", Some(7), true),
                            view_tab("Plain tab", None, false),
                            view_tab("Other tab", None, false),
                        ],
                        0,
                        [false, false, false],
                        GroupState::CollapsedWithSelection,
                        7,
                    ),
                };
                let mut stack_slots = [false; 3];
                let mut ball_slots = [false; 3];
                ball_slots[group_index] = state == GroupState::Expanded;
                stack_slots[group_index] = state == GroupState::CollapsedWithSelection;
                let layout = crate::chrome::layout::compute(420, 120, dpi, 3, false)
                    .with_group_slots(pill_slots, ball_slots, stack_slots);
                let first = layout.tab(group_index).unwrap();
                let last = if state == GroupState::Expanded {
                    layout.tab(group_index + 1).unwrap()
                } else {
                    first
                };
                let shell = Rect::new(first.left, first.top, last.right, last.bottom);
                tabs[group_index].group_frame =
                    Some(GroupFrame::resting(shell, state, member_count));
                let mut view = test_view(&layout, &tabs, pal, omnibox("", (0, 0), ""));
                view.active = 0;
                let mut surface = ChromeSurface::new(420, 120, dpi);
                surface.paint(&view, Rect::new(0, 0, 420, surface.height()));
                let rgba = composite_on_background(
                    surface.pixels(),
                    420,
                    surface.height(),
                    surface.stride(),
                    Some([pal.window.r, pal.window.g, pal.window.b]),
                );
                // Crop the tab row's left 280px and upscale 6x with nearest sampling.
                let top = layout.tab_row.top as usize;
                let rows = layout.tab_row.height() as usize;
                let cols = 420usize;
                let scale = 4usize;
                let mut out = vec![0u8; cols * scale * rows * scale * 4];
                for y in 0..rows * scale {
                    for x in 0..cols * scale {
                        let src = ((top + y / scale) * 420 + x / scale) * 4;
                        let dst = (y * cols * scale + x) * 4;
                        out[dst..dst + 4].copy_from_slice(&rgba[src..src + 4]);
                    }
                }
                write_png(
                    &dir.join(format!("scenario{scenario}-dpi{dpi}.png")),
                    (cols * scale) as u32,
                    (rows * scale) as u32,
                    &out,
                );
            }
        }
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
                                let tabs=(0..count).map(|i|TabView{title:if i+1==count{"A remarkably long page title that should truncate elegantly"}else{"A quiet page"},loading:false,favicon:if i%2==0{Some(&QA_FAVICON)}else{None},emphasis:Emphasis::resting(i==0,i+1==count,now),group_size:None,group_control:false,marker:None,group_colour:None,group_frame:None}).collect::<Vec<_>>();
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
                                    progress: if state == 2 { Some(45.0) } else { None },
                                    site: SiteSecurity::Secure,
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
                                    rail: None,
                                    icon_motion: None,
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
                marker: None,
                group_colour: None,
                group_frame: None,
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
                rail: None,
                icon_motion: None,
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
                rail: None,
                icon_motion: None,
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
                marker: None,
                group_colour: None,
                group_frame: None,
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
                site: SiteSecurity::NoSite,
            },
            now: Instant::now(),
            departures: &[],
            rail: None,
            icon_motion: None,
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
