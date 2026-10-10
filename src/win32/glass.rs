//! Window backdrop and system appearance.
//!
//! On Windows 10 the whole client area is an accent-policy acrylic window. The chrome is composited
//! on top of it with its own per-pixel alpha (see `chrome::paint`), so every chrome pixel must be
//! written with `BitBlt(SRCCOPY)` of premultiplied BGRA: DWM then blends the alpha over the blurred
//! backdrop. `AlphaBlend` onto the window DC does not work here. It makes DWM drop the blur behind
//! every pixel it touched, which shows the sharp desktop and renders translucent fills as black.
//!
//! Set `BOWSER_FORCE_SOLID=1` to skip glass entirely and use the opaque themed fill. Windows also
//! falls back to solid on its own when transparency is disabled, in a remote session, or in
//! high-contrast mode. Debug builds additionally honour `BOWSER_FORCE_THEME=light|dark` and
//! `BOWSER_GLASS_VARIANT=blur`.

use std::sync::atomic::{AtomicIsize, AtomicU8, Ordering};

use windows::Win32::{
    Foundation::{COLORREF, HWND},
    Graphics::Gdi::{CreateSolidBrush, HBRUSH},
    System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
};
use windows::core::s;

use crate::{
    chrome::layout::Metrics,
    theme::{self, Appearance, Palette},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackdropMode {
    Glass,
    Solid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptionRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl CaptionRect {
    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptionButtons {
    pub min: CaptionRect,
    pub max: CaptionRect,
    pub close: CaptionRect,
}

impl CaptionButtons {
    /// The caption buttons are the same rectangles the painter and `layout::hit_test` use: three
    /// `CAPTION_BUTTON_WIDTH` columns the height of the strip, flush with the right edge.
    pub fn for_client_width(width: i32, dpi: u32) -> Self {
        let m = Metrics::new(dpi);
        let button_width = m.px(Metrics::CAPTION_BUTTON_WIDTH);
        let button_height = m.px(Metrics::STRIP_HEIGHT);
        let right = width.max(0);
        let rect = |right: i32| CaptionRect {
            left: right.saturating_sub(button_width),
            top: 0,
            right,
            bottom: button_height,
        };
        let close = rect(right);
        let max = rect(close.left);
        let min = rect(max.left);
        Self { min, max, close }
    }
}

const UNKNOWN: u8 = 0;
const MODE_GLASS: u8 = 1;
const MODE_SOLID: u8 = 2;
static APPLIED_MODE: AtomicU8 = AtomicU8::new(UNKNOWN);
static WINDOW_DRAGGING: AtomicU8 = AtomicU8::new(0);

/// Cached system state. The registry is read once, then again after `refresh_system_state`.
const ALLOWED_YES: u8 = 1;
const ALLOWED_NO: u8 = 2;
static GLASS_ALLOWED: AtomicU8 = AtomicU8::new(UNKNOWN);
const APPEARANCE_LIGHT: u8 = 1;
const APPEARANCE_DARK: u8 = 2;
static APPEARANCE: AtomicU8 = AtomicU8::new(UNKNOWN);

static LIGHT_BRUSH: AtomicIsize = AtomicIsize::new(0);
static DARK_BRUSH: AtomicIsize = AtomicIsize::new(0);

/// Forgets the cached appearance and transparency state. Call on `WM_SETTINGCHANGE`,
/// `WM_THEMECHANGED` and `WM_DWMCOLORIZATIONCOLORCHANGED`, before `apply`.
pub fn refresh_system_state() {
    GLASS_ALLOWED.store(UNKNOWN, Ordering::Relaxed);
    APPEARANCE.store(UNKNOWN, Ordering::Relaxed);
}

pub fn backdrop_mode() -> BackdropMode {
    if !system_allows_glass() {
        BackdropMode::Solid
    } else {
        match APPLIED_MODE.load(Ordering::Relaxed) {
            MODE_SOLID => BackdropMode::Solid,
            _ => BackdropMode::Glass,
        }
    }
}

/// The app colour mode. The debug-only override is applied here so the glass tint, the immersive
/// title mode and the chrome palette can never disagree.
pub fn appearance() -> Appearance {
    match APPEARANCE.load(Ordering::Relaxed) {
        APPEARANCE_LIGHT => Appearance::Light,
        APPEARANCE_DARK => Appearance::Dark,
        _ => {
            let value = forced_appearance().unwrap_or_else(theme::system_appearance);
            APPEARANCE.store(
                if value == Appearance::Dark {
                    APPEARANCE_DARK
                } else {
                    APPEARANCE_LIGHT
                },
                Ordering::Relaxed,
            );
            value
        }
    }
}

/// The palette for the current appearance and backdrop mode.
pub fn palette() -> &'static Palette {
    theme::palette(appearance(), backdrop_mode() == BackdropMode::Solid)
}

pub fn apply(hwnd: HWND) -> BackdropMode {
    let appearance = appearance();
    let dark = appearance == Appearance::Dark;
    APPLIED_MODE.store(UNKNOWN, Ordering::Relaxed);
    if !system_allows_glass() {
        unsafe { apply_solid(hwnd, dark) };
        APPLIED_MODE.store(MODE_SOLID, Ordering::Relaxed);
        return BackdropMode::Solid;
    }

    let result = unsafe {
        set_immersive_dark(hwnd, dark);
        if os_build() >= 22_621 {
            // The system backdrop is only drawn inside the extended frame, so cover the client.
            let margins = [-1_i32; 4];
            apply_windows_11(hwnd) && DwmExtendFrameIntoClientArea(hwnd, margins.as_ptr()) >= 0
        } else {
            // Accent acrylic covers the client uniformly. Extending even a narrow custom frame
            // splits the backdrop into DWM frame and client surfaces, so leave it unextended and
            // draw the chrome in the separate layered overlay.
            apply_windows_10(
                hwnd,
                theme::glass_tint_abgr(theme::palette(appearance, false)),
            )
        }
    };
    if result {
        APPLIED_MODE.store(MODE_GLASS, Ordering::Relaxed);
        BackdropMode::Glass
    } else {
        unsafe { apply_solid(hwnd, dark) };
        APPLIED_MODE.store(MODE_SOLID, Ordering::Relaxed);
        BackdropMode::Solid
    }
}

/// Use the lighter Win10 blur while the native move/size loop is active. The atomic state also
/// lets theme refreshes reapply the current tint without reverting the drag backdrop.
pub fn set_window_dragging(hwnd: HWND, dragging: bool) {
    let next = u8::from(dragging);
    if WINDOW_DRAGGING.swap(next, Ordering::Relaxed) != next && os_build() < 22_621 {
        let _ = apply(hwnd);
    }
}

/// Opaque fill that matches `palette().window` of the solid palette, so the viewport, the strip
/// and the gaps around the omnibox are one colour.
pub fn solid_background(appearance: Appearance) -> COLORREF {
    let window = theme::palette(appearance, true).window;
    COLORREF(u32::from(window.r) | u32::from(window.g) << 8 | u32::from(window.b) << 16)
}

pub fn solid_brush(appearance: Appearance) -> HBRUSH {
    let storage = if appearance == Appearance::Dark {
        &DARK_BRUSH
    } else {
        &LIGHT_BRUSH
    };
    let mut raw = storage.load(Ordering::Relaxed);
    if raw == 0 {
        let created = unsafe { CreateSolidBrush(solid_background(appearance)) };
        raw = created.0 as isize;
        if storage
            .compare_exchange(0, raw, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::DeleteObject(created.into());
            }
            raw = storage.load(Ordering::Relaxed);
        }
    }
    HBRUSH(raw as *mut std::ffi::c_void)
}

#[cfg(debug_assertions)]
fn forced_appearance() -> Option<Appearance> {
    let value = std::env::var_os("BOWSER_FORCE_THEME")?;
    if value.eq_ignore_ascii_case("dark") {
        Some(Appearance::Dark)
    } else if value.eq_ignore_ascii_case("light") {
        Some(Appearance::Light)
    } else {
        None
    }
}

#[cfg(not(debug_assertions))]
fn forced_appearance() -> Option<Appearance> {
    None
}

#[cfg(debug_assertions)]
fn blur_variant_requested() -> bool {
    std::env::var_os("BOWSER_GLASS_VARIANT").is_some_and(|value| value.eq_ignore_ascii_case("blur"))
}

#[cfg(not(debug_assertions))]
fn blur_variant_requested() -> bool {
    false
}

fn system_allows_glass() -> bool {
    match GLASS_ALLOWED.load(Ordering::Relaxed) {
        ALLOWED_YES => true,
        ALLOWED_NO => false,
        _ => {
            let allowed = compute_system_allows_glass();
            GLASS_ALLOWED.store(
                if allowed { ALLOWED_YES } else { ALLOWED_NO },
                Ordering::Relaxed,
            );
            allowed
        }
    }
}

fn compute_system_allows_glass() -> bool {
    if environment_forces_solid() || unsafe { GetSystemMetrics(SM_REMOTESESSION) } != 0 {
        return false;
    }
    if registry_dword(
        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        "EnableTransparency",
    ) == Some(0)
    {
        return false;
    }
    !high_contrast_enabled()
}

fn high_contrast_enabled() -> bool {
    #[repr(C)]
    struct HighContrast {
        cb_size: u32,
        flags: u32,
        scheme: *mut u16,
    }
    const SPI_GETHIGHCONTRAST: u32 = 0x0042;
    const HCF_HIGHCONTRASTON: u32 = 0x0000_0001;
    let mut value = HighContrast {
        cb_size: std::mem::size_of::<HighContrast>() as u32,
        flags: 0,
        scheme: std::ptr::null_mut(),
    };
    unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            value.cb_size,
            (&mut value as *mut HighContrast).cast(),
            0,
        ) != 0
            && value.flags & HCF_HIGHCONTRASTON != 0
    }
}

fn environment_forces_solid() -> bool {
    std::env::var_os("BOWSER_FORCE_SOLID").is_some_and(|value| value == "1")
}

fn registry_dword(path: &str, name: &str) -> Option<u32> {
    let key: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    let value: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let mut data = 0_u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    const RRF_RT_REG_DWORD: u32 = 0x0000_0010;
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

fn os_build() -> u32 {
    #[repr(C)]
    struct OsVersionInfo {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        service_pack: [u16; 128],
    }
    let mut info = OsVersionInfo {
        size: std::mem::size_of::<OsVersionInfo>() as u32,
        major: 0,
        minor: 0,
        build: 0,
        platform: 0,
        service_pack: [0; 128],
    };
    unsafe { (RtlGetVersion)((&mut info as *mut OsVersionInfo).cast()) };
    info.build
}

unsafe fn apply_windows_11(hwnd: HWND) -> bool {
    const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;
    const DWMSBT_TRANSIENTWINDOW: i32 = 3;
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            (&DWMSBT_TRANSIENTWINDOW as *const i32).cast(),
            4,
        ) >= 0
    }
}

const ACCENT_DISABLED: u32 = 0;
const ACCENT_ENABLE_BLURBEHIND: u32 = 3;
const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;

/// `tint_abgr` is the accent `GradientColor`, `0xAABBGGRR`.
unsafe fn apply_windows_10(hwnd: HWND, tint_abgr: u32) -> bool {
    let state = windows_10_accent_state(
        WINDOW_DRAGGING.load(Ordering::Relaxed) != 0,
        blur_variant_requested(),
    );
    unsafe { set_accent(hwnd, state, tint_abgr) }
}

fn windows_10_accent_state(dragging: bool, blur_variant: bool) -> u32 {
    if dragging || blur_variant {
        ACCENT_ENABLE_BLURBEHIND
    } else {
        ACCENT_ENABLE_ACRYLICBLURBEHIND
    }
}

unsafe fn set_accent(hwnd: HWND, state: u32, gradient_color: u32) -> bool {
    const WCA_ACCENT_POLICY: u32 = 19;
    #[repr(C)]
    struct AccentPolicy {
        state: u32,
        flags: u32,
        gradient_color: u32,
        animation_id: u32,
    }
    #[repr(C)]
    struct AttributeData {
        attribute: u32,
        data: *mut std::ffi::c_void,
        size: usize,
    }
    let mut policy = AccentPolicy {
        state,
        flags: 2,
        gradient_color,
        animation_id: 0,
    };
    let mut data = AttributeData {
        attribute: WCA_ACCENT_POLICY,
        data: (&mut policy as *mut AccentPolicy).cast(),
        size: std::mem::size_of::<AccentPolicy>(),
    };
    unsafe {
        let Ok(user32) = GetModuleHandleW(windows::core::w!("user32.dll")) else {
            return false;
        };
        let Some(function) = GetProcAddress(user32, s!("SetWindowCompositionAttribute")) else {
            return false;
        };
        let function: unsafe extern "system" fn(HWND, *mut std::ffi::c_void) -> i32 =
            std::mem::transmute(function);
        function(hwnd, (&mut data as *mut AttributeData).cast()) != 0
    }
}

unsafe fn set_immersive_dark(hwnd: HWND, dark: bool) {
    const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
    let value: i32 = i32::from(dark);
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&value as *const i32).cast(),
            std::mem::size_of::<i32>() as u32,
        );
    }
}

unsafe fn apply_solid(hwnd: HWND, dark: bool) {
    const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;
    const DWMSBT_NONE: i32 = 1;
    unsafe {
        // The accent may be left over from an earlier glass `apply`; solid mode must not blur.
        if os_build() < 22_621 {
            let _ = set_accent(hwnd, ACCENT_DISABLED, 0);
        }
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            (&DWMSBT_NONE as *const i32).cast(),
            4,
        );
        set_immersive_dark(hwnd, dark);
        let margins = [0_i32; 4];
        let _ = DwmExtendFrameIntoClientArea(hwnd, margins.as_ptr());
    }
}

#[allow(non_snake_case)]
const HKEY_CURRENT_USER: isize = -2147483647;
const SM_REMOTESESSION: i32 = 4096;

#[link(name = "user32")]
unsafe extern "system" {
    fn GetSystemMetrics(index: i32) -> i32;
    fn SystemParametersInfoW(
        action: u32,
        param: u32,
        value: *mut std::ffi::c_void,
        flags: u32,
    ) -> i32;
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

#[link(name = "ntdll")]
unsafe extern "system" {
    fn RtlGetVersion(info: *mut std::ffi::c_void) -> i32;
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(
        hwnd: HWND,
        attribute: u32,
        value: *const std::ffi::c_void,
        size: u32,
    ) -> i32;
    fn DwmExtendFrameIntoClientArea(hwnd: HWND, margins: *const i32) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caption_buttons_are_right_aligned_and_dpi_scaled() {
        let one = CaptionButtons::for_client_width(1000, 96);
        assert_eq!(
            (one.min.left, one.close.right, one.close.bottom),
            (862, 1000, 30)
        );
        let fractional = CaptionButtons::for_client_width(1000, 120);
        assert_eq!(
            (
                fractional.min.left,
                fractional.close.right,
                fractional.close.bottom
            ),
            (826, 1000, 38)
        );
        let two = CaptionButtons::for_client_width(2000, 192);
        assert_eq!(
            (two.min.left, two.close.right, two.close.bottom),
            (1724, 2000, 60)
        );
    }

    #[test]
    fn caption_buttons_match_the_layout_caption_area() {
        for dpi in [96, 120, 144, 192] {
            let l = crate::chrome::layout::compute(1000, 700, dpi, 1, false);
            let b = CaptionButtons::for_client_width(1000, dpi);
            assert_eq!(b.min.left, l.caption_area.left, "dpi {dpi}");
            assert_eq!(b.close.right, l.caption_area.right, "dpi {dpi}");
            assert_eq!(b.close.bottom, l.strip.bottom, "dpi {dpi}");
        }
    }

    #[test]
    fn solid_background_is_the_solid_palette_window_colour() {
        // 0x00BBGGRR for 0x20242C and 0xF4F6F8.
        assert_eq!(solid_background(Appearance::Dark).0, 0x002C_2420);
        assert_eq!(solid_background(Appearance::Light).0, 0x00F8_F6F4);
    }

    #[test]
    fn drag_backdrop_uses_blur_and_restores_selected_normal_variant() {
        assert_eq!(
            windows_10_accent_state(true, false),
            ACCENT_ENABLE_BLURBEHIND
        );
        assert_eq!(
            windows_10_accent_state(false, false),
            ACCENT_ENABLE_ACRYLICBLURBEHIND
        );
        assert_eq!(
            windows_10_accent_state(true, true),
            ACCENT_ENABLE_BLURBEHIND
        );
        assert_eq!(
            windows_10_accent_state(false, true),
            ACCENT_ENABLE_BLURBEHIND
        );
    }
}
