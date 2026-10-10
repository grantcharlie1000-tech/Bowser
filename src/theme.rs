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

const LIGHT_GLASS: Palette = Palette {
    translucent: true,
    window: Rgba::hex(0xF4F6F8, 0xCC),
    tab_inactive: Rgba::hex(0xFFFFFF, 0x1C),
    tab_hover: Rgba::hex(0xFFFFFF, 0x50),
    tab_active: Rgba::hex(0xFFFFFF, 0x9C),
    tab_active_border: Rgba::hex(0x000000, 0x40),
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

pub fn palette(appearance: Appearance, solid_fallback: bool) -> &'static Palette {
    match (appearance, solid_fallback) {
        (Appearance::Light, false) => &LIGHT_GLASS,
        (Appearance::Dark, false) => &DARK_GLASS,
        (Appearance::Light, true) => &LIGHT_SOLID,
        (Appearance::Dark, true) => &DARK_SOLID,
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
}
