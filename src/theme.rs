//! Palette tokens for the translucent chrome: a see-through header over the DWM backdrop with
//! restrained island fills, and a fully opaque solid fallback.
//!
//! Translucent surfaces are contrast-checked as composites: backdrop, then window tint, then the
//! surface fill, then the text or glyph drawn on it. The backdrop is arbitrary blurred content, so
//! the tests sweep every backdrop grey from black to white in 1/16 steps. Contrast is not monotonic
//! in the backdrop: it falls to 1:1 where the composite's luminance matches the foreground's and
//! rises again on either side. Pure black and pure white therefore do not bound the worst case.
//! The model assumes the Windows 10 accent-policy composite used by `win32::glass`; the Windows 11
//! transient material applies its own tint and must be checked visually.

use std::fmt;

/// Non-premultiplied 8-bit sRGB colour with alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn hex(rgb: u32, alpha: u8) -> Self {
        Self {
            r: (rgb >> 16) as u8,
            g: (rgb >> 8) as u8,
            b: rgb as u8,
            a: alpha,
        }
    }

    /// Parses `#rrggbb` (the `theme_accent` setting's form). `None` for anything else.
    pub fn parse_hex(value: &str) -> Option<Self> {
        let bytes = value.as_bytes();
        // Six hex digits exactly: `from_str_radix` alone would also take a leading `+`.
        if bytes.len() != 7 || bytes[0] != b'#' || !bytes[1..].iter().all(u8::is_ascii_hexdigit) {
            return None;
        }
        let byte = |i: usize| -> Option<u8> {
            u8::from_str_radix(std::str::from_utf8(&bytes[i..i + 2]).ok()?, 16).ok()
        };
        Some(Self { r: byte(1)?, g: byte(3)?, b: byte(5)?, a: 0xFF })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appearance {
    Light,
    Dark,
}

impl fmt::Display for Appearance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Light => f.write_str("light"),
            Self::Dark => f.write_str("dark"),
        }
    }
}

/// Glass palettes use translucent fills over the window tint. Solid palettes use the same
/// hierarchy with every token opaque, for when glass is unavailable or too costly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palette {
    /// `true` when fills are meant to be composited over the backdrop.
    pub translucent: bool,
    /// Window tint over the backdrop, or the opaque window fill in solid palettes.
    pub window: Rgba,
    pub tab_inactive: Rgba,
    pub tab_hover: Rgba,
    pub tab_active: Rgba,
    pub tab_active_border: Rgba,
    pub tab_group_accent: Rgba,
    pub tab_text_inactive: Rgba,
    pub tab_text_active: Rgba,
    pub omnibox_fill: Rgba,
    pub omnibox_border: Rgba,
    pub omnibox_focus_border: Rgba,
    pub omnibox_selection: Rgba,
    pub omnibox_text: Rgba,
    pub omnibox_placeholder: Rgba,
    /// Inline completion suffix text. It is drawn beyond the typed text on `omnibox_fill`, never on
    /// the selection, so it is text-contrast tested over the fill only.
    pub omnibox_completion: Rgba,
    pub omnibox_caret: Rgba,
    pub progress_fill: Rgba,
    pub caption_glyph: Rgba,
    pub caption_hover: Rgba,
    pub close_hover: Rgba,
    pub close_glyph_hover: Rgba,
    pub hairline: Rgba,
}

/// Solid dots for the five tab markers, in `tab_card::MARKERS` order. GDI cannot draw colour emoji,
/// so each marker is a flat colour; the set is kept distinct on both light and dark surfaces.
pub const MARKER_COLOURS: [Rgba; 5] = [
    Rgba::hex(0xE0457B, 0xFF),
    Rgba::hex(0xF08A24, 0xFF),
    Rgba::hex(0x2E9E5B, 0xFF),
    Rgba::hex(0x1FA2C7, 0xFF),
    Rgba::hex(0x7B5CD6, 0xFF),
];

/// Tab island colours, indexed by the stored colour id (`tab_groups::COLOUR_COUNT` entries).
/// Id `None` means the theme accent instead.
pub const ISLAND_COLOURS: [Rgba; 8] = [
    Rgba::hex(0xB5525B, 0xFF),
    Rgba::hex(0xA85E2C, 0xFF),
    Rgba::hex(0x8D6920, 0xFF),
    Rgba::hex(0x2F7D56, 0xFF),
    Rgba::hex(0x2F7C88, 0xFF),
    Rgba::hex(0x4A6CAF, 0xFF),
    Rgba::hex(0x6D5C9E, 0xFF),
    Rgba::hex(0xB35A78, 0xFF),
];

const LIGHT_GLASS: Palette = Palette {
    translucent: true,
    window: Rgba::hex(0xF4F6F8, 0xCC),
    tab_inactive: Rgba::hex(0xFFFFFF, 0x1C),
    tab_hover: Rgba::hex(0xFFFFFF, 0x50),
    tab_active: Rgba::hex(0xFFFFFF, 0x9C),
    tab_active_border: Rgba::hex(0x000000, 0x40),
    tab_group_accent: Rgba::hex(0xE74F5F, 0xFF),
    tab_text_inactive: Rgba::hex(0x3A4048, 0xFF),
    tab_text_active: Rgba::hex(0x14181D, 0xFF),
    omnibox_fill: Rgba::hex(0xFFFFFF, 0x50),
    omnibox_border: Rgba::hex(0x168F98, 0x48),
    omnibox_focus_border: Rgba::hex(0x4A6D73, 0xFF),
    omnibox_selection: Rgba::hex(0x0067C0, 0x40),
    omnibox_text: Rgba::hex(0x14181D, 0xFF),
    omnibox_placeholder: Rgba::hex(0x4B5159, 0xFF),
    omnibox_completion: Rgba::hex(0x50575F, 0xFF),
    omnibox_caret: Rgba::hex(0x14181D, 0xFF),
    progress_fill: Rgba::hex(0x0067C0, 0xFF),
    caption_glyph: Rgba::hex(0x14181D, 0xFF),
    caption_hover: Rgba::hex(0x000000, 0x14),
    close_hover: Rgba::hex(0xC42B1C, 0xFF),
    close_glyph_hover: Rgba::hex(0xFFFFFF, 0xFF),
    hairline: Rgba::hex(0x000000, 0x1F),
};

const DARK_GLASS: Palette = Palette {
    translucent: true,
    window: Rgba::hex(0x20242C, 0xCC),
    tab_inactive: Rgba::hex(0xFFFFFF, 0x08),
    tab_hover: Rgba::hex(0xFFFFFF, 0x14),
    tab_active: Rgba::hex(0xFFFFFF, 0x2E),
    tab_active_border: Rgba::hex(0xFFFFFF, 0x48),
    tab_group_accent: Rgba::hex(0xE74F5F, 0xFF),
    tab_text_inactive: Rgba::hex(0xE4E8EE, 0xFF),
    tab_text_active: Rgba::hex(0xFFFFFF, 0xFF),
    omnibox_fill: Rgba::hex(0xFFFFFF, 0x1C),
    omnibox_border: Rgba::hex(0x5FD8DB, 0x48),
    omnibox_focus_border: Rgba::hex(0xB3C9CE, 0xFF),
    omnibox_selection: Rgba::hex(0x4DA3FF, 0x40),
    omnibox_text: Rgba::hex(0xFFFFFF, 0xFF),
    omnibox_placeholder: Rgba::hex(0xDDE2E9, 0xFF),
    omnibox_completion: Rgba::hex(0xDDE2E9, 0xFF),
    omnibox_caret: Rgba::hex(0xF3F5F7, 0xFF),
    progress_fill: Rgba::hex(0x84C8FF, 0xFF),
    caption_glyph: Rgba::hex(0xF3F5F7, 0xFF),
    caption_hover: Rgba::hex(0xFFFFFF, 0x14),
    close_hover: Rgba::hex(0xC42B1C, 0xFF),
    close_glyph_hover: Rgba::hex(0xFFFFFF, 0xFF),
    hairline: Rgba::hex(0xFFFFFF, 0x1F),
};

const LIGHT_SOLID: Palette = Palette {
    translucent: false,
    window: Rgba::hex(0xF4F6F8, 0xFF),
    tab_inactive: Rgba::hex(0xF4F6F8, 0xFF),
    tab_hover: Rgba::hex(0xEEF1F4, 0xFF),
    tab_active: Rgba::hex(0xFFFFFF, 0xFF),
    tab_active_border: Rgba::hex(0xBFC5CC, 0xFF),
    tab_group_accent: Rgba::hex(0xE74F5F, 0xFF),
    tab_text_inactive: Rgba::hex(0x3A4048, 0xFF),
    tab_text_active: Rgba::hex(0x14181D, 0xFF),
    omnibox_fill: Rgba::hex(0xFFFFFF, 0xFF),
    omnibox_border: Rgba::hex(0xC9CED5, 0xFF),
    omnibox_focus_border: Rgba::hex(0x008A94, 0xFF),
    omnibox_selection: Rgba::hex(0xBFD9EF, 0xFF),
    omnibox_text: Rgba::hex(0x14181D, 0xFF),
    omnibox_placeholder: Rgba::hex(0x4B5159, 0xFF),
    omnibox_completion: Rgba::hex(0x50575F, 0xFF),
    omnibox_caret: Rgba::hex(0x14181D, 0xFF),
    progress_fill: Rgba::hex(0x0067C0, 0xFF),
    caption_glyph: Rgba::hex(0x14181D, 0xFF),
    caption_hover: Rgba::hex(0xE1E4E7, 0xFF),
    close_hover: Rgba::hex(0xC42B1C, 0xFF),
    close_glyph_hover: Rgba::hex(0xFFFFFF, 0xFF),
    hairline: Rgba::hex(0xD5DADF, 0xFF),
};

const DARK_SOLID: Palette = Palette {
    translucent: false,
    window: Rgba::hex(0x20242C, 0xFF),
    tab_inactive: Rgba::hex(0x20242C, 0xFF),
    tab_hover: Rgba::hex(0x32353D, 0xFF),
    tab_active: Rgba::hex(0x2C3F44, 0xFF),
    tab_active_border: Rgba::hex(0x777D86, 0xFF),
    tab_group_accent: Rgba::hex(0xE74F5F, 0xFF),
    tab_text_inactive: Rgba::hex(0xE4E8EE, 0xFF),
    tab_text_active: Rgba::hex(0xFFFFFF, 0xFF),
    omnibox_fill: Rgba::hex(0x2A3A3E, 0xFF),
    omnibox_border: Rgba::hex(0x2F7F83, 0xFF),
    omnibox_focus_border: Rgba::hex(0x5FD8DB, 0xFF),
    omnibox_selection: Rgba::hex(0x1B314A, 0xFF),
    omnibox_text: Rgba::hex(0xF3F5F7, 0xFF),
    omnibox_placeholder: Rgba::hex(0xA3AAB4, 0xFF),
    omnibox_completion: Rgba::hex(0x9EA6B0, 0xFF),
    omnibox_caret: Rgba::hex(0xF3F5F7, 0xFF),
    progress_fill: Rgba::hex(0x6CB4FF, 0xFF),
    caption_glyph: Rgba::hex(0xF3F5F7, 0xFF),
    caption_hover: Rgba::hex(0x32353D, 0xFF),
    close_hover: Rgba::hex(0xC42B1C, 0xFF),
    close_glyph_hover: Rgba::hex(0xFFFFFF, 0xFF),
    hairline: Rgba::hex(0x2F343C, 0xFF),
};

/// Bowser's own palette for an appearance, whatever the person has customised.
pub fn palette_base(appearance: Appearance, solid_fallback: bool) -> &'static Palette {
    match (appearance, solid_fallback) {
        (Appearance::Light, false) => &LIGHT_GLASS,
        (Appearance::Dark, false) => &DARK_GLASS,
        (Appearance::Light, true) => &LIGHT_SOLID,
        (Appearance::Dark, true) => &DARK_SOLID,
    }
}

/// The palette for an appearance, with the person's colours (thread T8) in it once they are
/// installed: every surface that reads a palette (the chrome, menus, dialogs, panels) follows them.
/// With nothing customised it is [`palette_base`].
pub fn palette(appearance: Appearance, solid_fallback: bool) -> &'static Palette {
    match installed() {
        Some(set) => set.get(appearance, solid_fallback),
        None => palette_base(appearance, solid_fallback),
    }
}

// ---- the person's colours (thread T8) ----

use crate::theme_custom::{Colouring, Contrast};

/// What the person chose, resolved: the accent already follows the Theme, Accent colour, page colour
/// and saturation settings. The default is Bowser's own look and changes nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UserTheme {
    pub accent: Option<Rgba>,
    pub background: Option<Rgba>,
    pub foreground: Option<Rgba>,
    pub highlight: Option<Rgba>,
    pub colouring: Colouring,
    pub contrast: Contrast,
    /// The accent is the active page's colour ("Get accent colour from the webpage"), not one the
    /// person picked: a page's colour can be anything, so it is kept visible like the other colours.
    pub accent_from_page: bool,
}

impl UserTheme {
    /// Nothing chosen: the palettes are Bowser's own.
    pub fn is_plain(&self) -> bool {
        *self == Self::default()
    }

    /// Only an accent the person picked (`theme=system` or `theme_accent`, the settings Bowser had
    /// before the theme options): it colours the focus ring and the progress bar exactly as chosen,
    /// as [`with_accent`] always has, and nothing else in the palette moves.
    pub fn is_chosen_accent_only(&self) -> bool {
        self.accent.is_some() && !self.accent_from_page && UserTheme { accent: None, ..*self }.is_plain()
    }
}

/// Below this relative luminance a background is dark: white text reads on it better than black
/// does (the two have equal contrast at about 0.179).
const POLARITY_LUMINANCE: f64 = 0.179;

/// The light or dark palette family a background colour belongs to.
pub fn polarity(background: Rgba) -> Appearance {
    if luminance(background) > POLARITY_LUMINANCE {
        Appearance::Light
    } else {
        Appearance::Dark
    }
}

fn channel_linear(value: f64) -> f64 {
    let value = value / 255.0;
    if value <= 0.03928 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn luminance_rgb(rgb: [f64; 3]) -> f64 {
    0.2126 * channel_linear(rgb[0]) + 0.7152 * channel_linear(rgb[1]) + 0.0722 * channel_linear(rgb[2])
}

fn luminance(colour: Rgba) -> f64 {
    luminance_rgb([colour.r, colour.g, colour.b].map(f64::from))
}

fn ratio(a: f64, b: f64) -> f64 {
    let (lighter, darker) = if a > b { (a, b) } else { (b, a) };
    (lighter + 0.05) / (darker + 0.05)
}

fn opaque(colour: Rgba) -> Rgba {
    Rgba { a: 0xFF, ..colour }
}

/// `top` over an opaque `bottom`, in sRGB like the compositor.
fn over_opaque(top: Rgba, bottom: Rgba) -> Rgba {
    let alpha = u32::from(top.a);
    let mix = |t: u8, b: u8| ((u32::from(t) * alpha + u32::from(b) * (255 - alpha) + 127) / 255) as u8;
    Rgba { r: mix(top.r, bottom.r), g: mix(top.g, bottom.g), b: mix(top.b, bottom.b), a: 255 }
}

/// `a` moved `t` (0..=1) of the way to `b` in each colour channel; `a`'s alpha stays.
fn mix(a: Rgba, b: Rgba, t: f64) -> Rgba {
    let channel = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * t).round().clamp(0.0, 255.0) as u8;
    Rgba { r: channel(a.r, b.r), g: channel(a.g, b.g), b: channel(a.b, b.b), a: a.a }
}

/// The luminance of `layers` (bottom to top) over a backdrop of grey `grey`, unrounded.
fn stack_luminance(grey: f64, layers: &[Rgba]) -> f64 {
    let rgb = layers.iter().fold([grey; 3], |below, layer| {
        let alpha = f64::from(layer.a) / 255.0;
        let top = [layer.r, layer.g, layer.b].map(f64::from);
        std::array::from_fn(|i| top[i] * alpha + below[i] * (1.0 - alpha))
    });
    luminance_rgb(rgb)
}

/// The lowest contrast of the opaque `foreground` over any of `surfaces` on any backdrop grey from
/// black to white. A surface's luminance rises with the backdrop, so the contrast is lowest where the
/// two meet (1:1, when the foreground sits inside the surface's range) or at one end of the range.
/// That makes the check exact with two evaluations, not a sweep.
pub fn worst_contrast(foreground: Rgba, surfaces: &[Vec<Rgba>]) -> f64 {
    let foreground = luminance(foreground);
    surfaces
        .iter()
        .map(|layers| {
            let (low, high) = (stack_luminance(0.0, layers), stack_luminance(255.0, layers));
            if (low..=high).contains(&foreground) {
                1.0
            } else {
                ratio(foreground, low).min(ratio(foreground, high))
            }
        })
        .fold(f64::INFINITY, f64::min)
}

/// `colour`, or the least change to it (towards black or white) that reaches `minimum` over every
/// surface. When nothing reaches it, whichever of the colour, black and white comes closest.
fn ensure_contrast(colour: Rgba, surfaces: &[Vec<Rgba>], minimum: f64) -> Rgba {
    let colour = opaque(colour);
    if worst_contrast(colour, surfaces) >= minimum {
        return colour;
    }
    const BLACK: Rgba = Rgba::hex(0x000000, 0xFF);
    const WHITE: Rgba = Rgba::hex(0xFFFFFF, 0xFF);
    let mut best: Option<(u32, Rgba)> = None;
    for target in [BLACK, WHITE] {
        for step in 1..=20_u32 {
            let candidate = mix(colour, target, f64::from(step) / 20.0);
            if worst_contrast(candidate, surfaces) >= minimum {
                if best.is_none_or(|(taken, _)| step < taken) {
                    best = Some((step, candidate));
                }
                break;
            }
        }
    }
    if let Some((_, found)) = best {
        return found;
    }
    [colour, BLACK, WHITE]
        .into_iter()
        .max_by(|a, b| worst_contrast(*a, surfaces).total_cmp(&worst_contrast(*b, surfaces)))
        .unwrap_or(colour)
}

/// `colour` with its HSL saturation limited to `max_percent` (hue and lightness kept). 100 or more
/// leaves it as it is.
pub fn limit_saturation(colour: Rgba, max_percent: u8) -> Rgba {
    if max_percent >= 100 {
        return colour;
    }
    let (r, g, b) = (f64::from(colour.r) / 255.0, f64::from(colour.g) / 255.0, f64::from(colour.b) / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let lightness = (max + min) / 2.0;
    let delta = max - min;
    if delta == 0.0 {
        return colour;
    }
    let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs());
    let cap = f64::from(max_percent) / 100.0;
    if saturation <= cap {
        return colour;
    }
    let hue = if max == r {
        ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * cap;
    let x = chroma * (1.0 - (hue % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match hue as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let offset = lightness - chroma / 2.0;
    let byte = |v: f64| ((v + offset) * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgba { r: byte(r1), g: byte(g1), b: byte(b1), a: colour.a }
}

/// The colour that stands for a favicon: the most saturated common colour of its opaque pixels, or
/// `None` for an image with none. `bgra_premultiplied` is top-down BGRA, as `favicon::decode` gives.
pub fn dominant_colour(bgra_premultiplied: &[u8]) -> Option<Rgba> {
    // Sixteen levels per channel; each bucket weighs its pixels by how colourful they are.
    let mut buckets = vec![(0.0_f64, [0.0_f64; 3], 0.0_f64); 16 * 16 * 16];
    for pixel in bgra_premultiplied.chunks_exact(4) {
        let alpha = f64::from(pixel[3]);
        if alpha < 128.0 {
            continue;
        }
        let unmultiply = |v: u8| (f64::from(v) * 255.0 / alpha).min(255.0);
        let (b, g, r) = (unmultiply(pixel[0]), unmultiply(pixel[1]), unmultiply(pixel[2]));
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let saturation = if max == 0.0 { 0.0 } else { (max - min) / max };
        let index = ((r as usize >> 4) << 8) | ((g as usize >> 4) << 4) | (b as usize >> 4);
        let weight = saturation + 0.05;
        let bucket = &mut buckets[index];
        bucket.0 += weight;
        bucket.1[0] += r * weight;
        bucket.1[1] += g * weight;
        bucket.1[2] += b * weight;
        bucket.2 += 1.0;
    }
    let (weight, sums, _) = buckets.into_iter().max_by(|a, b| a.0.total_cmp(&b.0))?;
    (weight > 0.0).then(|| Rgba {
        r: (sums[0] / weight).round() as u8,
        g: (sums[1] / weight).round() as u8,
        b: (sums[2] / weight).round() as u8,
        a: 0xFF,
    })
}

/// How strongly the accent washes the bar it tints, by surface.
const TAB_BAR_TINT: f64 = 0.22;
const ADDRESS_TINT_SOLID: f64 = 0.16;
const ADDRESS_TINT_GLASS: f64 = 0.75;
/// Opacity of a highlight colour over the field, and the steps it fades to when no text colour reads
/// on both the field and the selection.
const SELECTION_ALPHA: u8 = 0x59;
const SELECTION_FADE: [u8; 5] = [0x40, 0x30, 0x20, 0x10, 0x00];

/// The palette for `appearance` with `user`'s colours in it, and the appearance it really is: a
/// custom background brings the light or dark palette family that reads on it, whatever Windows or
/// the schedule asked for. A custom background is opaque (it replaces the glass tint), so every
/// surface over it is known; the text and focus colours are then kept at the contrast the
/// `contrast` choice names, over every backdrop. With nothing chosen this is `palette_base`.
pub fn themed(appearance: Appearance, solid: bool, user: &UserTheme) -> (Palette, Appearance) {
    let appearance = user.background.map_or(appearance, polarity);
    let base = palette_base(appearance, solid);
    if user.is_plain() {
        return (base.clone(), appearance);
    }
    // A picked accent alone keeps its colour: pushing it to 3:1 over every backdrop from black to
    // white (what a glass palette needs) moved it most of the way to white or black, so a red like
    // #bf2b2b showed as a washed-out pink (#e5aaaa) on dark glass. v0.0.18 used it as chosen.
    if user.is_chosen_accent_only() {
        return (with_accent(base, user.accent), appearance);
    }
    let glass = palette_base(appearance, false);
    let mut p = base.clone();
    let tint = user.accent.unwrap_or(base.progress_fill);

    if let Some(background) = user.background {
        let window = opaque(background);
        let fold = |overlay: Rgba| if solid { over_opaque(overlay, window) } else { overlay };
        p.window = window;
        p.tab_inactive = fold(glass.tab_inactive);
        p.tab_hover = fold(glass.tab_hover);
        p.tab_active = fold(glass.tab_active);
        p.tab_active_border = fold(glass.tab_active_border);
        p.omnibox_fill = fold(glass.omnibox_fill);
        p.omnibox_border = fold(glass.omnibox_border);
        p.caption_hover = fold(glass.caption_hover);
        p.hairline = fold(glass.hairline);
    }
    if user.colouring.tints_tab_bar() {
        p.window = mix(p.window, tint, TAB_BAR_TINT);
        if solid {
            p.tab_inactive = mix(p.tab_inactive, tint, TAB_BAR_TINT);
            p.tab_hover = mix(p.tab_hover, tint, TAB_BAR_TINT);
            p.tab_active = mix(p.tab_active, tint, TAB_BAR_TINT / 2.0);
        }
        p.tab_active_border = Rgba { a: if solid { 0xFF } else { 0xA0 }, ..tint };
    }
    if user.colouring.tints_address_bar() {
        p.omnibox_fill = mix(p.omnibox_fill, tint, if solid { ADDRESS_TINT_SOLID } else { ADDRESS_TINT_GLASS });
        p.omnibox_border = Rgba { a: if solid { 0xFF } else { 0xA0 }, ..tint };
    }
    if let Some(accent) = user.accent {
        p.omnibox_focus_border = accent;
        p.progress_fill = accent;
    }
    // The selection is an overlay on the field; solid palettes show it flattened.
    let selection_rgb = user.highlight.unwrap_or(glass.omnibox_selection);
    let own_selection = user.highlight.is_some() || user.background.is_some() || user.colouring.tints_address_bar();
    let selection_start = if user.highlight.is_some() { SELECTION_ALPHA } else { glass.omnibox_selection.a };

    if let Some(foreground) = user.foreground {
        let foreground = opaque(foreground);
        let surface = opaque(p.window);
        p.tab_text_active = foreground;
        p.omnibox_text = foreground;
        p.caption_glyph = foreground;
        p.omnibox_caret = foreground;
        p.tab_text_inactive = mix(foreground, surface, 0.18);
        p.omnibox_placeholder = mix(foreground, surface, 0.32);
        p.omnibox_completion = mix(foreground, surface, 0.30);
    }
    enforce_contrast(&mut p, user.contrast, solid, own_selection.then_some((selection_rgb, selection_start)));
    (p, appearance)
}

/// Keeps the text and focus colours of `p` at the contrast `contrast` asks for, over every backdrop.
fn enforce_contrast(p: &mut Palette, contrast: Contrast, solid: bool, selection: Option<(Rgba, u8)>) {
    let text = contrast.text_minimum();
    let non_text = contrast.non_text_minimum();
    let window = p.window;
    p.tab_text_inactive = ensure_contrast(p.tab_text_inactive, &[vec![window, p.tab_inactive], vec![window, p.tab_hover]], text);
    p.tab_text_active = ensure_contrast(p.tab_text_active, &[vec![window, p.tab_active]], text);
    p.omnibox_placeholder = ensure_contrast(p.omnibox_placeholder, &[vec![window, p.omnibox_fill]], text);
    p.omnibox_completion = ensure_contrast(p.omnibox_completion, &[vec![window, p.omnibox_fill]], text);
    p.caption_glyph = ensure_contrast(p.caption_glyph, &[vec![window], vec![window, p.caption_hover]], text);

    // The text of the address field also sits on the selection. When no one colour reads on both the
    // field and the selection, the highlight fades until one does.
    let field = vec![window, p.omnibox_fill];
    let steps: Vec<u8> = match selection {
        Some((_, start)) => std::iter::once(start).chain(SELECTION_FADE.into_iter().filter(|alpha| *alpha < start)).collect(),
        None => vec![0],
    };
    let mut chosen = None;
    for alpha in steps {
        let selected = match selection {
            Some((rgb, _)) => {
                let overlay = Rgba { a: alpha, ..rgb };
                if solid { over_opaque(overlay, over_opaque(p.omnibox_fill, window)) } else { overlay }
            }
            None => p.omnibox_selection,
        };
        let surfaces = [field.clone(), vec![window, p.omnibox_fill, selected]];
        let colour = ensure_contrast(p.omnibox_text, &surfaces, text);
        let reads = worst_contrast(colour, &surfaces) >= text;
        chosen = Some((colour, selected));
        if reads || selection.is_none() {
            break;
        }
    }
    if let Some((colour, selected)) = chosen {
        p.omnibox_text = colour;
        p.omnibox_selection = selected;
    }
    let fill = [vec![window], vec![window, p.omnibox_fill]];
    p.omnibox_focus_border = ensure_contrast(p.omnibox_focus_border, &fill, non_text);
    p.progress_fill = ensure_contrast(p.progress_fill, &fill, non_text);
    p.omnibox_caret = ensure_contrast(p.omnibox_caret, &[vec![window, p.omnibox_fill]], non_text);
}

/// `themed`, for an appearance that was asked for by name (a private tab's own light or dark look,
/// the installed palettes of the other family): a background colour applies only to the family it
/// belongs to, and the other family takes the rest of the person's colours on Bowser's own window.
pub fn themed_for(appearance: Appearance, solid: bool, user: &UserTheme) -> Palette {
    match user.background {
        Some(background) if polarity(background) != appearance => {
            themed(appearance, solid, &UserTheme { background: None, ..*user }).0
        }
        _ => themed(appearance, solid, user).0,
    }
}

/// The four palettes of one `UserTheme`.
pub struct Installed {
    user: UserTheme,
    palettes: [Palette; 4],
}

impl Installed {
    fn build(user: UserTheme) -> Self {
        Self {
            user,
            palettes: [
                themed_for(Appearance::Light, false, &user),
                themed_for(Appearance::Dark, false, &user),
                themed_for(Appearance::Light, true, &user),
                themed_for(Appearance::Dark, true, &user),
            ],
        }
    }

    fn get(&self, appearance: Appearance, solid: bool) -> &Palette {
        let index = usize::from(appearance == Appearance::Dark) + if solid { 2 } else { 0 };
        &self.palettes[index]
    }
}

#[cfg(not(test))]
mod store {
    use super::{Installed, UserTheme};
    use std::sync::{Mutex, RwLock};

    static CURRENT: RwLock<Option<&'static Installed>> = RwLock::new(None);
    static CACHE: Mutex<Vec<&'static Installed>> = Mutex::new(Vec::new());

    pub fn current() -> Option<&'static Installed> {
        CURRENT.read().ok().and_then(|guard| *guard)
    }

    pub fn set(user: Option<UserTheme>) -> bool {
        let value = user.map(|user| {
            let mut cache = CACHE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(found) = cache.iter().find(|installed| installed.user == user) {
                return *found;
            }
            let leaked: &'static Installed = Box::leak(Box::new(Installed::build(user)));
            cache.push(leaked);
            leaked
        });
        let mut current = CURRENT.write().unwrap_or_else(|poisoned| poisoned.into_inner());
        let changed = current.map(|installed| installed.user) != value.map(|installed| installed.user);
        *current = value;
        changed
    }
}

/// Under test each thread has its own installed theme, so a test that installs one never changes
/// what the palette tests of other threads read.
#[cfg(test)]
mod store {
    use super::{Installed, UserTheme};
    use std::cell::RefCell;

    thread_local! {
        static CURRENT: RefCell<Option<&'static Installed>> = const { RefCell::new(None) };
    }

    pub fn current() -> Option<&'static Installed> {
        CURRENT.with(|current| *current.borrow())
    }

    pub fn set(user: Option<UserTheme>) -> bool {
        let value: Option<&'static Installed> = user.map(|user| &*Box::leak(Box::new(Installed::build(user))));
        CURRENT.with(|current| {
            let changed = current.borrow().map(|installed| installed.user) != value.map(|installed| installed.user);
            *current.borrow_mut() = value;
            changed
        })
    }
}

fn installed() -> Option<&'static Installed> {
    store::current()
}

/// Makes `user`'s colours the ones every palette read returns. A plain theme removes them. Returns
/// whether the installed theme changed.
pub fn install_user_theme(user: UserTheme) -> bool {
    store::set((!user.is_plain()).then_some(user))
}

/// The appearance to draw with: `requested` (Windows, or the schedule), unless a custom background
/// belongs to the other family.
pub fn effective_appearance(requested: Appearance) -> Appearance {
    match installed().and_then(|set| set.user.background) {
        Some(background) => polarity(background),
        None => requested,
    }
}

/// `palette` with its focus and progress colours replaced by `accent`, when the `theme`/
/// `theme_accent` settings choose one. The rest of the palette (and its contrast guarantees) is
/// unchanged: an explicit accent trades the curated ring/progress contrast for the colour chosen,
/// exactly as a browser's own "accent colour" picker does.
pub fn with_accent(palette: &Palette, accent: Option<Rgba>) -> Palette {
    let mut palette = palette.clone();
    if let Some(accent) = accent {
        palette.omnibox_focus_border = accent;
        palette.progress_fill = accent;
    }
    palette
}

const DWM_KEY: &str = r"Software\Microsoft\Windows\DWM";
const COLORIZATION_COLOR: &str = "ColorizationColor";

/// Windows' own accent colour (`theme=system`), from the DWM colorization value (`0xAARRGGBB`,
/// read as opaque since the setting never wants a translucent accent). `None` when it cannot be
/// read.
pub fn system_accent_colour() -> Option<Rgba> {
    let value = registry_dword(DWM_KEY, COLORIZATION_COLOR)?;
    Some(Rgba { r: (value >> 16) as u8, g: (value >> 8) as u8, b: value as u8, a: 0xFF })
}

/// The accent to show, combining the `theme`/`theme_accent` settings with the live system colour:
/// an explicit `theme_accent` always wins; otherwise `theme=system` uses `system_accent`, and
/// `theme=helium` keeps the palette's own curated colours (`None`).
pub fn resolved_accent(theme: crate::settings::Theme, accent_hex: Option<&str>, system_accent: Option<Rgba>) -> Option<Rgba> {
    if let Some(hex) = accent_hex {
        if let Some(colour) = Rgba::parse_hex(hex) {
            return Some(colour);
        }
    }
    match theme {
        crate::settings::Theme::System => system_accent,
        crate::settings::Theme::Helium => None,
    }
}

/// Packs the window tint as an accent-policy `GradientColor`, which is `0xAABBGGRR` (red in the low byte).
pub fn glass_tint_abgr(palette: &Palette) -> u32 {
    let window = palette.window;
    (u32::from(window.a) << 24)
        | (u32::from(window.b) << 16)
        | (u32::from(window.g) << 8)
        | u32::from(window.r)
}

const PERSONALIZE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const APPS_USE_LIGHT_THEME: &str = "AppsUseLightTheme";

/// Reads the app-mode preference. Call on startup and when the system colour set changes.
pub fn system_appearance() -> Appearance {
    appearance_from_registry_value(registry_dword(PERSONALIZE_KEY, APPS_USE_LIGHT_THEME))
}

/// Windows writes `0` for dark apps. A missing or other value means light, the Windows default.
pub fn appearance_from_registry_value(value: Option<u32>) -> Appearance {
    match value {
        Some(0) => Appearance::Dark,
        _ => Appearance::Light,
    }
}

// Win32 registry access, declared directly as in `win32::glass` so no `windows` feature flag is needed.
fn registry_dword(path: &str, name: &str) -> Option<u32> {
    const HKEY_CURRENT_USER: isize = -2147483647;
    const RRF_RT_REG_DWORD: u32 = 0x0000_0010;
    let key: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    let value: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let mut data = 0_u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut data as *mut u32).cast(),
            &mut size,
        )
    };
    (status == 0 && size == std::mem::size_of::<u32>() as u32).then_some(data)
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegGetValueW(
        key: isize,
        subkey: *const u16,
        value: *const u16,
        flags: u32,
        kind: *mut u32,
        data: *mut std::ffi::c_void,
        size: *mut u32,
    ) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    const AA_TEXT: f64 = 4.5;
    const AA_NON_TEXT: f64 = 3.0;
    /// Backdrop greys are swept in steps of 1/16 of a grey level, from black to white.
    const BACKDROP_STEPS_PER_LEVEL: u32 = 16;
    const BLACK: Rgba = Rgba::hex(0x000000, 0xFF);
    const WHITE: Rgba = Rgba::hex(0xFFFFFF, 0xFF);
    const VARIANTS: [(Appearance, bool); 4] = [
        (Appearance::Light, false),
        (Appearance::Dark, false),
        (Appearance::Light, true),
        (Appearance::Dark, true),
    ];

    /// Alpha-composites `top` over an opaque `bottom`.
    fn over(top: Rgba, bottom: Rgba) -> Rgba {
        let alpha = u32::from(top.a);
        let mix = |t: u8, b: u8| {
            ((u32::from(t) * alpha + u32::from(b) * (255 - alpha) + 127) / 255) as u8
        };
        Rgba {
            r: mix(top.r, bottom.r),
            g: mix(top.g, bottom.g),
            b: mix(top.b, bottom.b),
            a: 255,
        }
    }

    /// Composites `layers` bottom-to-top over an opaque backdrop.
    fn stack(backdrop: Rgba, layers: &[Rgba]) -> Rgba {
        layers
            .iter()
            .fold(backdrop, |below, &layer| over(layer, below))
    }

    /// Relative luminance of an sRGB colour with channels in `0.0..=255.0`.
    fn luminance_of(rgb: [f64; 3]) -> f64 {
        fn channel(value: f64) -> f64 {
            let value = value / 255.0;
            if value <= 0.03928 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * channel(rgb[0]) + 0.7152 * channel(rgb[1]) + 0.0722 * channel(rgb[2])
    }

    fn rgba_luminance(colour: Rgba) -> f64 {
        luminance_of([colour.r, colour.g, colour.b].map(f64::from))
    }

    fn contrast_ratio(a: f64, b: f64) -> f64 {
        let (lighter, darker) = if a > b { (a, b) } else { (b, a) };
        (lighter + 0.05) / (darker + 0.05)
    }

    /// Composites `layers` bottom-to-top over an opaque grey backdrop. Unrounded, so fractional
    /// backdrop levels are represented exactly.
    fn composite_grey(grey: f64, layers: &[Rgba]) -> [f64; 3] {
        layers.iter().fold([grey; 3], |below, layer| {
            let alpha = f64::from(layer.a) / 255.0;
            let top = [layer.r, layer.g, layer.b].map(f64::from);
            std::array::from_fn(|i| top[i] * alpha + below[i] * (1.0 - alpha))
        })
    }

    /// Every swept backdrop grey, from black to white in 1/16 steps.
    fn backdrop_greys() -> impl Iterator<Item = f64> {
        let steps = f64::from(BACKDROP_STEPS_PER_LEVEL);
        (0..=255 * BACKDROP_STEPS_PER_LEVEL).map(move |step| f64::from(step) / steps)
    }

    /// The lowest contrast of `foreground` over `surface` across the backdrop sweep, with the
    /// backdrop grey that produced it.
    fn worst_swept_contrast(foreground: Rgba, surface: &[Rgba]) -> (f64, f64) {
        let foreground = rgba_luminance(foreground);
        backdrop_greys()
            .map(|grey| {
                let ratio = contrast_ratio(foreground, luminance_of(composite_grey(grey, surface)));
                (ratio, grey)
            })
            .fold((f64::INFINITY, 0.0), |worst, candidate| {
                if candidate.0 < worst.0 {
                    candidate
                } else {
                    worst
                }
            })
    }

    fn assert_contrast(label: &str, foreground: Rgba, surfaces: &[&[Rgba]], minimum: f64) {
        assert_eq!(foreground.a, 255, "{label}: foreground must be opaque");
        for surface in surfaces {
            let (ratio, grey) = worst_swept_contrast(foreground, surface);
            assert!(
                ratio >= minimum,
                "{label}: {ratio:.2}:1 is below {minimum}:1 over {surface:?} at backdrop grey {grey}"
            );
        }
    }

    fn tokens(palette: &Palette) -> [Rgba; 21] {
        [
            palette.window,
            palette.tab_inactive,
            palette.tab_hover,
            palette.tab_active,
            palette.tab_active_border,
            palette.tab_text_inactive,
            palette.tab_text_active,
            palette.omnibox_fill,
            palette.omnibox_border,
            palette.omnibox_focus_border,
            palette.omnibox_selection,
            palette.omnibox_text,
            palette.omnibox_placeholder,
            palette.omnibox_completion,
            palette.omnibox_caret,
            palette.progress_fill,
            palette.caption_glyph,
            palette.caption_hover,
            palette.close_hover,
            palette.close_glyph_hover,
            palette.hairline,
        ]
    }

    #[test]
    fn contrast_formula_matches_wcag_reference_values() {
        let (black, white) = (rgba_luminance(BLACK), rgba_luminance(WHITE));
        assert!((contrast_ratio(black, white) - 21.0).abs() < 1e-9);
        assert!((contrast_ratio(white, white) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn backdrop_sweep_covers_every_sixteenth_grey_level() {
        let greys: Vec<f64> = backdrop_greys().collect();
        assert_eq!(greys.len(), 255 * 16 + 1);
        assert_eq!(greys.first(), Some(&0.0));
        assert_eq!(greys.last(), Some(&255.0));
        assert!((greys[1] - 0.0625).abs() < 1e-12);
    }

    /// Shows why the sweep is needed. Over a half-transparent black layer, a mid-grey foreground
    /// falls to about 1:1 where the backdrop composite matches its luminance (backdrop ~128.5). Both
    /// pure extremes still read above 2:1, so a black-and-white check would miss the dip.
    #[test]
    fn contrast_dips_between_the_extreme_backdrops() {
        let foreground = Rgba::hex(0x404040, 0xFF);
        let surface = [Rgba::hex(0x000000, 0x80)];
        let foreground_luminance = rgba_luminance(foreground);
        for grey in [0.0, 255.0] {
            let composite = luminance_of(composite_grey(grey, &surface));
            assert!(contrast_ratio(foreground_luminance, composite) > 1.9);
        }
        let (ratio, grey) = worst_swept_contrast(foreground, &surface);
        assert!(ratio < 1.1, "expected a dip near 1:1, got {ratio:.3}:1");
        assert!((grey - 128.5).abs() <= 1.0, "dip at backdrop grey {grey}");
    }

    #[test]
    fn glass_tint_is_packed_as_abgr_for_accent_policy() {
        let light = glass_tint_abgr(palette(Appearance::Light, false));
        assert_eq!(light, 0xCCF8_F6F4);
        assert_eq!(light.to_le_bytes(), [0xF4, 0xF6, 0xF8, 0xCC]);
        assert_eq!(
            glass_tint_abgr(palette(Appearance::Dark, false)),
            0xCC2C_2420
        );
        assert_eq!(
            glass_tint_abgr(palette(Appearance::Light, true)),
            0xFFF8_F6F4
        );
    }

    #[test]
    fn compositing_blends_in_srgb_and_keeps_opaque_result() {
        let half_white = Rgba::hex(0xFFFFFF, 0x80);
        assert_eq!(over(half_white, BLACK), Rgba::hex(0x808080, 0xFF));
        assert_eq!(stack(WHITE, &[]), WHITE);
    }

    #[test]
    fn text_tokens_meet_aa_across_backdrop_sweep() {
        for (appearance, solid) in VARIANTS {
            let p = palette(appearance, solid);
            let label = format!("{appearance}/solid={solid}");
            let window = p.window;
            assert_contrast(
                &format!("{label} tab_text_inactive"),
                p.tab_text_inactive,
                &[&[window, p.tab_inactive], &[window, p.tab_hover]],
                AA_TEXT,
            );
            assert_contrast(
                &format!("{label} tab_text_active"),
                p.tab_text_active,
                &[&[window, p.tab_active]],
                AA_TEXT,
            );
            assert_contrast(
                &format!("{label} omnibox_text"),
                p.omnibox_text,
                &[
                    &[window, p.omnibox_fill],
                    &[window, p.omnibox_fill, p.omnibox_selection],
                ],
                AA_TEXT,
            );
            assert_contrast(
                &format!("{label} omnibox_placeholder"),
                p.omnibox_placeholder,
                &[&[window, p.omnibox_fill]],
                AA_TEXT,
            );
            assert_contrast(
                &format!("{label} omnibox_completion"),
                p.omnibox_completion,
                &[&[window, p.omnibox_fill]],
                AA_TEXT,
            );
            assert_contrast(
                &format!("{label} caption_glyph"),
                p.caption_glyph,
                &[&[window], &[window, p.caption_hover]],
                AA_TEXT,
            );
            assert_contrast(
                &format!("{label} close_glyph_hover"),
                p.close_glyph_hover,
                &[&[p.close_hover]],
                AA_TEXT,
            );
        }
    }

    #[test]
    fn focus_caret_and_progress_meet_non_text_contrast_across_backdrop_sweep() {
        for (appearance, solid) in VARIANTS {
            let p = palette(appearance, solid);
            let label = format!("{appearance}/solid={solid}");
            let window = p.window;
            assert_contrast(
                &format!("{label} omnibox_focus_border"),
                p.omnibox_focus_border,
                &[&[window], &[window, p.omnibox_fill]],
                AA_NON_TEXT,
            );
            assert_contrast(
                &format!("{label} progress_fill"),
                p.progress_fill,
                &[&[window], &[window, p.omnibox_fill]],
                AA_NON_TEXT,
            );
            assert_contrast(
                &format!("{label} omnibox_caret"),
                p.omnibox_caret,
                &[&[window, p.omnibox_fill]],
                AA_NON_TEXT,
            );
        }
    }

    #[test]
    fn active_tab_is_the_lightest_surface_across_backdrop_sweep() {
        for (appearance, solid) in VARIANTS {
            let p = palette(appearance, solid);
            let label = format!("{appearance}/solid={solid}");
            for grey in backdrop_greys() {
                let luminance = |fill: Rgba| luminance_of(composite_grey(grey, &[p.window, fill]));
                let inactive = luminance(p.tab_inactive);
                let hover = luminance(p.tab_hover);
                let active = luminance(p.tab_active);
                // Inactive tabs blend into the window, so hover only has to be visibly distinct from
                // it. Light palettes darken hover, so it is not ordered between inactive and active.
                assert!(
                    active > inactive && active > hover && (hover - inactive).abs() > 1e-3,
                    "{label} over grey {grey}: inactive {inactive:.3}, hover {hover:.3}, active {active:.3}"
                );
            }
        }
    }

    #[test]
    fn solid_palettes_are_fully_opaque_and_glass_palettes_are_not() {
        for appearance in [Appearance::Light, Appearance::Dark] {
            let solid = palette(appearance, true);
            assert!(!solid.translucent);
            for token in tokens(solid) {
                assert_eq!(token.a, 255, "{appearance} solid token {token:?}");
            }

            let glass = palette(appearance, false);
            assert!(glass.translucent);
            assert!(glass.window.a < 255);
            assert!(glass.tab_inactive.a < 255);
            assert!(glass.omnibox_fill.a < 255);
        }
    }

    #[test]
    fn appearance_maps_registry_values_to_light_or_dark() {
        assert_eq!(appearance_from_registry_value(Some(0)), Appearance::Dark);
        assert_eq!(appearance_from_registry_value(Some(1)), Appearance::Light);
        assert_eq!(appearance_from_registry_value(Some(7)), Appearance::Light);
        assert_eq!(appearance_from_registry_value(None), Appearance::Light);
    }

    #[test]
    fn appearance_displays_as_lowercase_word() {
        assert_eq!(Appearance::Light.to_string(), "light");
        assert_eq!(Appearance::Dark.to_string(), "dark");
    }

    #[test]
    fn rgba_parses_exact_lower_case_hex_only() {
        assert_eq!(Rgba::parse_hex("#1a2b3c"), Some(Rgba { r: 0x1a, g: 0x2b, b: 0x3c, a: 0xFF }));
        assert_eq!(Rgba::parse_hex("#FFFFFF"), Some(Rgba { r: 0xFF, g: 0xFF, b: 0xFF, a: 0xFF }));
        for bad in ["1a2b3c", "#1a2b3", "#1a2b3cd", "", "#gggggg", "red"] {
            assert_eq!(Rgba::parse_hex(bad), None, "{bad}");
        }
    }

    #[test]
    fn with_accent_overrides_only_focus_and_progress() {
        let base = palette(Appearance::Light, true);
        let accent = Rgba::hex(0xFF0000, 0xFF);
        let accented = with_accent(base, Some(accent));
        assert_eq!(accented.omnibox_focus_border, accent);
        assert_eq!(accented.progress_fill, accent);
        assert_eq!(accented.window, base.window, "nothing else moves");
        assert_eq!(&with_accent(base, None), base);
    }

    #[test]
    fn resolved_accent_prefers_an_explicit_colour_then_system_then_heliums_own() {
        use crate::settings::Theme;
        let system = Rgba::hex(0x00FF00, 0xFF);
        let explicit = Rgba::hex(0x0000FF, 0xFF);
        assert_eq!(resolved_accent(Theme::Helium, Some("#0000ff"), Some(system)), Some(explicit), "an explicit accent wins under any theme");
        assert_eq!(resolved_accent(Theme::System, Some("#0000ff"), Some(system)), Some(explicit));
        assert_eq!(resolved_accent(Theme::System, None, Some(system)), Some(system));
        assert_eq!(resolved_accent(Theme::System, None, None), None, "no system colour could be read");
        assert_eq!(resolved_accent(Theme::Helium, None, Some(system)), None, "helium keeps its own curated colours");
        assert_eq!(resolved_accent(Theme::Helium, Some("not-a-colour"), Some(system)), None, "an invalid explicit value is ignored");
    }

    // ---- the person's colours (thread T8) ----

    fn user(background: Option<u32>, foreground: Option<u32>, highlight: Option<u32>, accent: Option<u32>, colouring: Colouring, contrast: Contrast) -> UserTheme {
        let colour = |value: Option<u32>| value.map(|rgb| Rgba::hex(rgb, 0xFF));
        UserTheme { accent: colour(accent), background: colour(background), foreground: colour(foreground), highlight: colour(highlight), colouring, contrast, accent_from_page: false }
    }

    /// `user`'s theme with the accent taken from the page instead of picked.
    fn from_page(theme: UserTheme) -> UserTheme {
        UserTheme { accent_from_page: true, ..theme }
    }

    /// Every text and focus check the palette tests make, as (label, colour, surfaces, is text).
    fn contrast_checks(p: &Palette) -> Vec<(&'static str, Rgba, Vec<Vec<Rgba>>, bool)> {
        let w = p.window;
        vec![
            ("tab_text_inactive", p.tab_text_inactive, vec![vec![w, p.tab_inactive], vec![w, p.tab_hover]], true),
            ("tab_text_active", p.tab_text_active, vec![vec![w, p.tab_active]], true),
            ("omnibox_text", p.omnibox_text, vec![vec![w, p.omnibox_fill], vec![w, p.omnibox_fill, p.omnibox_selection]], true),
            ("omnibox_placeholder", p.omnibox_placeholder, vec![vec![w, p.omnibox_fill]], true),
            ("omnibox_completion", p.omnibox_completion, vec![vec![w, p.omnibox_fill]], true),
            ("caption_glyph", p.caption_glyph, vec![vec![w], vec![w, p.caption_hover]], true),
            ("omnibox_focus_border", p.omnibox_focus_border, vec![vec![w], vec![w, p.omnibox_fill]], false),
            ("progress_fill", p.progress_fill, vec![vec![w], vec![w, p.omnibox_fill]], false),
            ("omnibox_caret", p.omnibox_caret, vec![vec![w, p.omnibox_fill]], false),
        ]
    }

    /// The best any one of black or white does over `surfaces`: what a text colour can reach at most.
    fn best_extreme(surfaces: &[Vec<Rgba>]) -> f64 {
        worst_contrast(BLACK, surfaces).max(worst_contrast(WHITE, surfaces))
    }

    fn assert_user_palette(label: &str, p: &Palette, contrast: Contrast, sweep: bool) {
        for (name, colour, surfaces, text) in contrast_checks(p) {
            let wanted = if text { contrast.text_minimum() } else { contrast.non_text_minimum() };
            // A colour can reach no more than the better of black and white allows.
            let reachable = best_extreme(&surfaces);
            let minimum = wanted.min(reachable - 1e-9);
            assert_eq!(colour.a, 255, "{label} {name}: opaque");
            let exact = worst_contrast(colour, &surfaces);
            assert!(exact >= minimum, "{label} {name}: {exact:.3}:1 is below {minimum:.2}:1 over {surfaces:?}");
            if text {
                // Standard contrast is a floor whatever was asked: never below AA where AA is reachable.
                assert!(exact >= AA_TEXT.min(reachable - 1e-9), "{label} {name}: {exact:.3}:1 below AA");
            }
            if sweep {
                let refs: Vec<&[Rgba]> = surfaces.iter().map(Vec::as_slice).collect();
                assert_contrast(&format!("{label} {name}"), colour, &refs, minimum);
            }
        }
    }

    #[test]
    fn the_exact_worst_case_matches_the_backdrop_sweep() {
        for (appearance, solid) in VARIANTS {
            let p = palette(appearance, solid);
            for (name, colour, surfaces, _) in contrast_checks(p) {
                let exact = worst_contrast(colour, &surfaces);
                for candidate in [colour, BLACK, WHITE, Rgba::hex(0x808080, 0xFF), Rgba::hex(0x404040, 0xFF)] {
                    let exact = if candidate == colour { exact } else { worst_contrast(candidate, &surfaces) };
                    let swept = surfaces.iter().map(|surface| worst_swept_contrast(candidate, surface).0).fold(f64::INFINITY, f64::min);
                    assert!(swept >= exact - 1e-9, "{appearance}/{solid} {name}: the sweep found {swept:.4} below the exact {exact:.4}");
                    assert!(swept - exact < 0.05, "{appearance}/{solid} {name}: exact {exact:.4} far from swept {swept:.4}");
                }
            }
        }
    }

    #[test]
    fn nothing_chosen_changes_nothing() {
        for (appearance, solid) in VARIANTS {
            let (p, look) = themed(appearance, solid, &UserTheme::default());
            assert_eq!(&p, palette_base(appearance, solid));
            assert_eq!(look, appearance);
        }
        assert!(UserTheme::default().is_plain());
        assert!(!user(None, None, None, None, Colouring::Off, Contrast::High).is_plain(), "contrast alone is a choice");
    }

    #[test]
    fn user_colours_keep_text_and_focus_contrast_across_the_backdrop_sweep() {
        let backgrounds = [None, Some(0xFFFFFF), Some(0x000000), Some(0x808080), Some(0x7F7F7F), Some(0xB0B0B0), Some(0xFFEE00), Some(0x1E1E2E)];
        let foregrounds = [None, Some(0x808080), Some(0xFF00FF), Some(0xFFFFFF)];
        let highlights = [None, Some(0xFFFF00), Some(0x000080)];
        let accents = [None, Some(0xFFEE00), Some(0xFF0000), Some(0x101010)];
        let mut count = 0_u32;
        for background in backgrounds {
            for foreground in foregrounds {
                for highlight in highlights {
                    for accent in accents {
                        for colouring in Colouring::ALL {
                            for contrast in Contrast::ALL {
                                let mut theme = user(background, foreground, highlight, accent, *colouring, *contrast);
                                if theme.is_chosen_accent_only() {
                                    // A picked accent alone is used as chosen (v0.0.18's `with_accent`);
                                    // the same colour from a page is held to every contrast check below.
                                    for (appearance, solid) in VARIANTS {
                                        let (p, look) = themed(appearance, solid, &theme);
                                        assert_eq!(p, with_accent(palette_base(appearance, solid), theme.accent), "{theme:?} {appearance}/solid={solid}");
                                        assert_eq!(look, appearance);
                                    }
                                    theme.accent_from_page = true;
                                }
                                for (appearance, solid) in VARIANTS {
                                    count += 1;
                                    let label = format!("{theme:?} {appearance}/solid={solid}");
                                    let (p, look) = themed(appearance, solid, &theme);
                                    if solid {
                                        for token in tokens(&p) {
                                            assert_eq!(token.a, 255, "{label}: solid token {token:?}");
                                        }
                                    }
                                    if let Some(background) = background {
                                        assert_eq!(look, polarity(Rgba::hex(background, 0xFF)), "{label}");
                                        assert_eq!(p.window.a, 255, "{label}: a custom background is opaque");
                                    }
                                    // Every combination is checked exactly; every 53rd also against the
                                    // full backdrop sweep.
                                    assert_user_palette(&label, &p, *contrast, count % 211 == 0);
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(count > 10_000, "{count} palettes checked");
    }

    #[test]
    fn a_background_brings_the_palette_family_that_reads_on_it() {
        let white = Rgba::hex(0xFFFFFF, 0xFF);
        let black = Rgba::hex(0x000000, 0xFF);
        assert_eq!(polarity(white), Appearance::Light);
        assert_eq!(polarity(black), Appearance::Dark);
        assert_eq!(polarity(Rgba::hex(0x808080, 0xFF)), Appearance::Light);
        let dark_on_light = user(Some(0x000000), None, None, None, Colouring::Off, Contrast::Standard);
        let (p, look) = themed(Appearance::Light, true, &dark_on_light);
        assert_eq!(look, Appearance::Dark, "windows asked for light; a black background is dark");
        assert_eq!(p.window, black);
        assert_eq!(p.tab_text_active, palette_base(Appearance::Dark, true).tab_text_active, "dark family text");
        let (p, look) = themed(Appearance::Dark, false, &user(Some(0xFFFFFF), None, None, None, Colouring::Off, Contrast::Standard));
        assert_eq!(look, Appearance::Light);
        assert_eq!(p.window, white, "glass takes the colour opaque, replacing the tint");
        assert!(p.translucent && p.omnibox_fill.a < 255, "the glass fills stay overlays");
    }

    #[test]
    fn each_colour_reaches_the_tokens_it_names() {
        let base = palette_base(Appearance::Dark, true);
        // A foreground that already reads is used exactly.
        let (p, _) = themed(Appearance::Dark, true, &user(None, Some(0xCCDDEE), None, None, Colouring::Off, Contrast::Standard));
        assert_eq!(p.tab_text_active, Rgba::hex(0xCCDDEE, 0xFF));
        assert_eq!(p.omnibox_text, Rgba::hex(0xCCDDEE, 0xFF));
        assert_eq!(p.caption_glyph, Rgba::hex(0xCCDDEE, 0xFF));
        assert_ne!(p.tab_text_inactive, base.tab_text_inactive, "the quieter text follows it");
        assert_eq!(p.window, base.window, "nothing else moves");
        // A highlight colours the selection and nothing else.
        let (p, _) = themed(Appearance::Light, false, &user(None, None, Some(0xFFCC00), None, Colouring::Off, Contrast::Standard));
        assert_eq!((p.omnibox_selection.r, p.omnibox_selection.g, p.omnibox_selection.b), (0xFF, 0xCC, 0x00));
        assert_ne!(p.omnibox_selection, palette_base(Appearance::Light, false).omnibox_selection);
        assert_eq!(p.window, palette_base(Appearance::Light, false).window);
        // A background colours the window and the fills over it.
        let (p, _) = themed(Appearance::Dark, true, &user(Some(0x102030), None, None, None, Colouring::Off, Contrast::Standard));
        assert_eq!(p.window, Rgba::hex(0x102030, 0xFF));
        assert_eq!(p.tab_inactive, over_opaque(palette_base(Appearance::Dark, false).tab_inactive, p.window));
    }

    #[test]
    fn colouring_tints_the_bar_it_names_and_only_that_bar() {
        let accent = Some(0xE03030);
        for (appearance, solid) in VARIANTS {
            let base = palette_base(appearance, solid);
            let themed_with = |mode| themed(appearance, solid, &user(None, None, None, accent, mode, Contrast::Standard)).0;
            let off = themed_with(Colouring::Off);
            assert_eq!((off.window, off.omnibox_fill), (base.window, base.omnibox_fill), "{appearance}/{solid}: off tints nothing");
            let tab = themed_with(Colouring::TabBar);
            assert_ne!(tab.window, base.window, "{appearance}/{solid}: the tab bar is tinted");
            assert_eq!(tab.omnibox_fill, base.omnibox_fill, "{appearance}/{solid}: the address bar is not");
            let address = themed_with(Colouring::AddressBar);
            assert_eq!(address.window, base.window);
            assert_ne!(address.omnibox_fill, base.omnibox_fill);
            assert_ne!(address.omnibox_border, base.omnibox_border);
            let both = themed_with(Colouring::Unified);
            assert_eq!((both.window, both.omnibox_fill), (tab.window, address.omnibox_fill), "{appearance}/{solid}: unified is both");
            // The tint leans towards the accent: more red than the base window.
            assert!(i32::from(tab.window.r) - i32::from(tab.window.b) > i32::from(base.window.r) - i32::from(base.window.b));
        }
        // Without an accent of its own, the tint is Bowser's curated one: the mode still acts.
        let (p, _) = themed(Appearance::Light, true, &user(None, None, None, None, Colouring::TabBar, Contrast::Standard));
        assert_ne!(p.window, palette_base(Appearance::Light, true).window);
    }

    #[test]
    fn the_accent_colours_focus_and_progress_unless_it_cannot_be_seen() {
        let (p, _) = themed(Appearance::Light, true, &user(None, None, None, Some(0xCC0000), Colouring::Off, Contrast::Standard));
        assert_eq!(p.omnibox_focus_border, Rgba::hex(0xCC0000, 0xFF), "a readable accent is used as chosen");
        assert_eq!(p.progress_fill, Rgba::hex(0xCC0000, 0xFF));
        let (p, _) = themed(Appearance::Light, true, &from_page(user(None, None, None, Some(0xCC0000), Colouring::Off, Contrast::Standard)));
        assert_eq!(p.progress_fill, Rgba::hex(0xCC0000, 0xFF), "and so is a readable page colour");
        // A page's pale yellow on a light window would vanish; the nearest visible colour stands in.
        let (p, _) = themed(Appearance::Light, true, &from_page(user(None, None, None, Some(0xFFEE00), Colouring::Off, Contrast::Standard)));
        assert_ne!(p.omnibox_focus_border, Rgba::hex(0xFFEE00, 0xFF));
        assert!(worst_contrast(p.omnibox_focus_border, &[vec![p.window], vec![p.window, p.omnibox_fill]]) >= 3.0);
        assert!(p.omnibox_focus_border.r > 0x80 && p.omnibox_focus_border.b < 0x40, "still yellowish: {:?}", p.omnibox_focus_border);
    }

    #[test]
    fn high_contrast_pushes_text_past_the_standard_floor() {
        let grey = Some(0x6B6B6B); // 5.3:1 on white: fine for AA, short of 7:1
        let standard = themed(Appearance::Light, true, &user(Some(0xFFFFFF), grey, None, None, Colouring::Off, Contrast::Standard)).0;
        let high = themed(Appearance::Light, true, &user(Some(0xFFFFFF), grey, None, None, Colouring::Off, Contrast::High)).0;
        assert_eq!(standard.tab_text_active, Rgba::hex(0x6B6B6B, 0xFF), "standard leaves a colour that reads");
        assert_ne!(high.tab_text_active, standard.tab_text_active);
        let on = |p: &Palette| worst_contrast(p.tab_text_active, &[vec![p.window, p.tab_active]]);
        assert!(on(&standard) < 7.0 && on(&standard) >= 4.5);
        assert!(on(&high) >= 7.0, "high contrast reaches 7:1: {:.2}", on(&high));
        assert!(worst_contrast(high.omnibox_placeholder, &[vec![high.window, high.omnibox_fill]]) >= 7.0);
        // Bowser's own palette is not changed by choosing the standard level.
        assert_eq!(&themed(Appearance::Light, true, &user(None, None, None, None, Colouring::Off, Contrast::Standard)).0, palette_base(Appearance::Light, true));
    }

    #[test]
    fn a_text_colour_that_cannot_read_is_moved_towards_black_or_white() {
        let (p, _) = themed(Appearance::Light, true, &user(Some(0xFFFFFF), Some(0xFFFFFF), None, None, Colouring::Off, Contrast::Standard));
        assert_ne!(p.tab_text_active, Rgba::hex(0xFFFFFF, 0xFF), "white text on a white window cannot be kept");
        assert!(worst_contrast(p.tab_text_active, &[vec![p.window, p.tab_active]]) >= 4.5);
        assert!(worst_contrast(p.omnibox_text, &[vec![p.window, p.omnibox_fill]]) >= 4.5);
    }

    #[test]
    fn a_highlight_fades_when_no_text_colour_reads_on_both_the_field_and_it() {
        // A mid-grey field with an opaque-looking dark highlight: no one text colour reads on both at full strength.
        let theme = user(Some(0x9A9A9A), None, Some(0x101010), None, Colouring::Off, Contrast::Standard);
        let (p, _) = themed(Appearance::Light, true, &theme);
        let surfaces = vec![vec![p.window, p.omnibox_fill], vec![p.window, p.omnibox_fill, p.omnibox_selection]];
        assert!(worst_contrast(p.omnibox_text, &surfaces) >= 4.5, "{p:?}");
        assert_ne!(p.omnibox_selection, p.omnibox_fill, "the selection is still told apart");
    }

    #[test]
    fn saturation_is_limited_without_moving_the_hue_or_the_lightness() {
        let hsl = |c: Rgba| {
            let (r, g, b) = (f64::from(c.r) / 255.0, f64::from(c.g) / 255.0, f64::from(c.b) / 255.0);
            let (max, min) = (r.max(g).max(b), r.min(g).min(b));
            let l = (max + min) / 2.0;
            let s = if max == min { 0.0 } else { (max - min) / (1.0 - (2.0 * l - 1.0).abs()) };
            (s, l)
        };
        let vivid = Rgba::hex(0xE03070, 0xFF);
        assert_eq!(limit_saturation(vivid, 100), vivid);
        let half = limit_saturation(vivid, 50);
        let (s, l) = hsl(half);
        assert!((s - 0.5).abs() < 0.02, "saturation {s}");
        assert!((l - hsl(vivid).1).abs() < 0.01, "lightness kept");
        assert!(half.r > half.g && half.r > half.b && half.b > half.g, "same hue ordering: {half:?}");
        let grey = limit_saturation(vivid, 0);
        assert!(grey.r.abs_diff(grey.g) <= 1 && grey.g.abs_diff(grey.b) <= 1, "{grey:?}");
        let already = Rgba::hex(0x806060, 0xFF);
        assert_eq!(limit_saturation(already, 90), already, "a colour under the limit is untouched");
        for channel in [0x00_u32, 0x7F, 0xFF] {
            let c = Rgba::hex(channel * 0x010101, 0xFF);
            assert_eq!(limit_saturation(c, 10), c, "greys are untouched");
        }
        for base in 0..=255_u32 {
            let c = Rgba::hex((base * 0x010000) | ((255 - base) * 0x000100) | ((base * 7) % 256), 0xFF);
            let (s, _) = hsl(limit_saturation(c, 30));
            assert!(s <= 0.31, "{c:?} -> {s}");
        }
    }

    #[test]
    fn a_favicons_colour_is_its_most_colourful_common_colour() {
        // 8x8: a white field, a red block of 20 pixels, a few blue ones, and a transparent corner.
        let mut pixels = vec![0xFF_u8; 8 * 8 * 4];
        for (i, px) in pixels.chunks_exact_mut(4).enumerate() {
            let (x, y) = (i % 8, i / 8);
            let bgra = if y < 2 && x < 2 {
                [0, 0, 0, 0]
            } else if (2..6).contains(&x) && (2..7).contains(&y) {
                [0x30, 0x30, 0xE0, 0xFF]
            } else if x == 7 && y == 7 {
                [0xE0, 0x30, 0x30, 0xFF]
            } else {
                [0xFF, 0xFF, 0xFF, 0xFF]
            };
            px.copy_from_slice(&bgra);
        }
        let colour = dominant_colour(&pixels).expect("a colour");
        assert!(colour.r > 0xC0 && colour.g < 0x50 && colour.b < 0x50, "{colour:?}");
        assert_eq!(dominant_colour(&vec![0; 64]), None, "a fully transparent icon has no colour");
        assert_eq!(dominant_colour(&[]), None);
        // Premultiplied half-opaque pixels are un-premultiplied before they count.
        let half: Vec<u8> = (0..16).flat_map(|_| [0x10_u8, 0x10, 0x70, 0xFF]).collect();
        assert_eq!(dominant_colour(&half), Some(Rgba::hex(0x701010, 0xFF)));
    }

    #[test]
    fn the_installed_theme_is_what_every_palette_read_returns() {
        assert_eq!(palette(Appearance::Dark, true), palette_base(Appearance::Dark, true));
        let dark = user(Some(0x101820), None, None, None, Colouring::Off, Contrast::Standard);
        assert!(install_user_theme(dark), "installing a theme is a change");
        assert!(!install_user_theme(dark), "installing it again is not");
        assert_eq!(palette(Appearance::Dark, true).window, Rgba::hex(0x101820, 0xFF));
        assert_eq!(palette(Appearance::Dark, false).window, Rgba::hex(0x101820, 0xFF));
        assert_eq!(effective_appearance(Appearance::Light), Appearance::Dark, "the background's family wins over Windows");
        assert_eq!(palette(Appearance::Light, true), palette_base(Appearance::Light, true), "the other family is Bowser's own");
        // No background: the appearance stays whatever was asked, and the colours apply to both.
        install_user_theme(user(None, None, Some(0xFFCC00), None, Colouring::Off, Contrast::Standard));
        assert_eq!(effective_appearance(Appearance::Light), Appearance::Light);
        assert_eq!(effective_appearance(Appearance::Dark), Appearance::Dark);
        assert_ne!(palette(Appearance::Light, true).omnibox_selection, palette_base(Appearance::Light, true).omnibox_selection);
        assert_ne!(palette(Appearance::Dark, true).omnibox_selection, palette_base(Appearance::Dark, true).omnibox_selection);
        assert!(install_user_theme(UserTheme::default()), "back to plain is a change");
        assert_eq!(palette(Appearance::Dark, true), palette_base(Appearance::Dark, true));
        assert_eq!(effective_appearance(Appearance::Dark), Appearance::Dark);
    }

    #[test]
    fn strict_hex_refuses_a_plus_sign() {
        assert_eq!(Rgba::parse_hex("#+a+b+c"), None);
        assert_eq!(Rgba::parse_hex("#-1-2-3"), None);
        assert!(Rgba::parse_hex("#0aF9c0").is_some());
    }
}
