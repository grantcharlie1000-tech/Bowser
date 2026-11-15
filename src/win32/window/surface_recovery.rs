//! A single bounded recovery episode per window. Only the timer grants another paint/upload.
use super::*;
use std::{cell::Cell, rc::Weak};
use crate::chrome::paint::SurfaceError;
use windows::Win32::{
    Graphics::Gdi::{DrawFrameControl, DrawTextW, GetSysColor, GetSysColorBrush, SetBkMode, SetTextColor,
        COLOR_WINDOW, COLOR_WINDOWTEXT, DFC_CAPTION, DFCS_CAPTIONCLOSE, DFCS_CAPTIONMAX,
        DFCS_CAPTIONMIN, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, HDC, TRANSPARENT},
    UI::WindowsAndMessaging::IsWindowVisible,
};

pub(super) const TIMER: usize = 0x5f2;
pub(super) const RETRY_ID: usize = 0x6f20;
pub(super) const CLOSE_ID: usize = 0x6f21;
const RETRY_MS: [u32; 3] = [250, 1_000, 3_000];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Healthy,
    Schedule(usize),
    Waiting(usize),
    Painting(usize),
    Exhausted,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fault {
    Storage(SurfaceError),
    Text(SurfaceError),
    Overlay,
    Upload,
    Timer,
}

#[derive(Default)]
struct Emergency {
    retry: HWND,
    close: HWND,
    applied: Option<(i32, i32, u32, isize, bool)>,
}

pub(super) struct Recovery {
    phase: Rc<Cell<Phase>>,
    pub(super) full_repaint: bool,
    notice: Option<Fault>,
    emergency: Option<Emergency>,
}

/// Reads the authoritative phase without borrowing WINDOWS during native EDIT painting.
/// A weak owner expires on teardown; a recycled HWND cannot revive an old subclass.
pub(super) struct EditPaintLease(Weak<Cell<Phase>>);
impl EditPaintLease {
    pub(super) fn state(&self) -> (bool, bool) {
        let Some(owner) = self.0.upgrade() else { return (false, false); };
        let phase = owner.get();
        (phase != Phase::Closed, !matches!(phase, Phase::Healthy | Phase::Closed))
    }
}

impl Recovery {
    pub(super) fn new() -> Self {
        Self { phase: Rc::new(Cell::new(Phase::Healthy)), full_repaint: true, notice: None, emergency: None }
    }
    pub(super) fn edit_paint_lease(&self) -> EditPaintLease {
        EditPaintLease(Rc::downgrade(&self.phase))
    }
    pub(super) fn can_draw(&self) -> bool {
        matches!(self.phase.get(), Phase::Healthy | Phase::Painting(_))
    }
    pub(super) fn needs_emergency(&self) -> bool {
        !matches!(self.phase.get(), Phase::Healthy | Phase::Closed)
    }
    pub(super) fn fail(&mut self, fault: Fault) {
        let next = match self.phase.get() {
            Phase::Healthy => 0,
            Phase::Painting(attempt) => attempt + 1,
            _ => return, // Repaint and duplicate fault messages never replenish the budget.
        };
        self.notice = Some(fault);
        self.full_repaint = true;
        self.phase.set(if next < RETRY_MS.len() { Phase::Schedule(next) } else { Phase::Exhausted });
    }
    fn schedule(&mut self) -> Option<u32> {
        let Phase::Schedule(attempt) = self.phase.get() else { return None; };
        self.phase.set(Phase::Waiting(attempt));
        Some(RETRY_MS[attempt])
    }
    fn begin_retry(&mut self) -> bool {
        let Phase::Waiting(attempt) = self.phase.get() else { return false; };
        self.phase.set(Phase::Painting(attempt));
        self.full_repaint = true;
        true
    }
    pub(super) fn succeeded(&mut self) {
        if self.can_draw() { self.phase.set(Phase::Healthy); self.full_repaint = false; }
    }
    pub(super) fn close(&mut self) { self.phase.set(Phase::Closed); self.notice = None; }
}

pub(super) fn create_overlay(hwnd: HWND, size: (i32, i32)) -> HWND {
    #[cfg(test)]
    if super::surface_recovery_tests::take_overlay_failure() { return HWND::default(); }
    unsafe {
        // Do not expose an unpainted popup. A successful upload makes it visible with its owner.
        CreateWindowExW(WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            w!("STATIC"), w!(""), WS_POPUP, 0, 0, size.0, size.1,
            Some(hwnd), None, None, None).unwrap_or_default()
    }
}

pub(super) fn tick(hwnd: HWND) {
    unsafe { let _ = KillTimer(Some(hwnd), TIMER); }
    if window_closing(hwnd) {
        let _ = with_chrome(hwnd, |s| s.surface_recovery.close());
        return;
    }
    let Some((overlay, size)) = with_chrome(hwnd, |s| {
        if !s.surface_recovery.begin_retry() { return None; }
        match s.surface.try_resize(s.client.0, s.client.1, s.dpi) {
            Ok(_) => Some((s.overlay, s.client)),
            Err(error) => { s.surface_recovery.fail(Fault::Storage(error)); None }
        }
    }).flatten() else { sync(hwnd); return; };
    if overlay.is_invalid() || !unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(overlay)) }.as_bool() {
        let overlay = create_overlay(hwnd, size);
        let stored = with_chrome(hwnd, |s| {
            if !s.surface_recovery.can_draw() { return false; }
            s.overlay = overlay;
            if overlay.is_invalid() { s.surface_recovery.fail(Fault::Overlay); }
            true
        }).unwrap_or(false);
        if !stored && !overlay.is_invalid() { unsafe { let _ = DestroyWindow(overlay); } }
    }
    sync(hwnd);
    unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
}

pub(super) fn retry(hwnd: HWND) {
    if window_closing(hwnd) { return; }
    let retry = with_chrome(hwnd, |s| {
        if s.surface_recovery.phase.get() != Phase::Exhausted { return false; }
        s.surface_recovery.phase.set(Phase::Waiting(0));
        true
    }).unwrap_or(false);
    if retry { tick(hwnd); }
}

/// Native effects run outside WINDOWS borrows: window creation/visibility may reenter dispatch.
pub(super) fn sync(hwnd: HWND) {
    if window_closing(hwnd) {
        let _ = with_chrome(hwnd, |s| s.surface_recovery.close());
        unsafe { let _ = KillTimer(Some(hwnd), TIMER); }
        return;
    }
    let Some((enter, leave, delay, notice, overlay, edit, show_edit)) = with_chrome(hwnd, |s| {
        let r = &mut s.surface_recovery;
        let enter = r.needs_emergency() && r.emergency.is_none();
        let leave = if !r.needs_emergency() { r.emergency.take() } else { None };
        if enter { r.emergency = Some(Emergency::default()); }
        (enter, leave, r.schedule(), r.notice.take(), s.overlay, s.edit, !s.html_toolbar_active)
    }) else { return; };
    if let Some(fault) = notice {
        if let Some(app) = app_of(hwnd) {
            let event = match fault {
                Fault::Storage(_) => "native_surface_allocation_failed",
                Fault::Text(_) => "native_surface_text_failed",
                Fault::Overlay => "native_surface_overlay_failed",
                Fault::Upload => "native_surface_upload_failed",
                Fault::Timer => "native_surface_timer_failed",
            };
            let stage = match fault {
                Fault::Storage(error) | Fault::Text(error) => match error {
                    SurfaceError::InvalidSize => "invalid_size",
                    SurfaceError::CreateDc => "create_dc",
                    SurfaceError::CreateBitmap => "create_bitmap",
                    SurfaceError::SelectBitmap => "select_bitmap",
                },
                _ => "native",
            };
            let _ = app.borrow().log.record_detail(event, stage, 0, 0, 0);
            if with_chrome(hwnd, |s| s.surface_recovery.phase.get() == Phase::Exhausted).unwrap_or(false) {
                let _ = app.borrow().log.record("native_surface_recovery_exhausted");
            }
        }
    }
    unsafe {
        if enter {
            make_opaque(hwnd);
            if !overlay.is_invalid() { let _ = ShowWindow(overlay, SW_HIDE); }
            if show_edit && !edit.is_invalid() {
                let _ = SetWindowRgn(edit, None, false);
                let _ = ShowWindow(edit, SW_SHOW);
            }
            let emergency = Emergency {
                retry: button(hwnd, RETRY_ID, w!("Retry chrome")),
                close: button(hwnd, CLOSE_ID, w!("Close")),
                applied: None,
            };
            let mut emergency = Some(emergency);
            let _ = with_chrome(hwnd, |s| {
                if s.surface_recovery.needs_emergency() { s.surface_recovery.emergency = emergency.take(); }
            });
            if let Some(emergency) = emergency { destroy_buttons(emergency); }
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
        if let Some(emergency) = leave {
            destroy_buttons(emergency);
            if !window_closing(hwnd) {
                // Erase the emergency GDI pixels (including their alpha) before restoring glass.
                let dc = windows::Win32::Graphics::Gdi::GetDC(Some(hwnd));
                let mut client = RECT::default();
                let _ = GetClientRect(hwnd, &mut client);
                let _ = windows::Win32::Graphics::Gdi::PatBlt(dc, 0, 0, client.right, client.bottom, windows::Win32::Graphics::Gdi::BLACKNESS);
                let _ = windows::Win32::Graphics::Gdi::ReleaseDC(Some(hwnd), dc);
                let _ = glass::apply(hwnd);
                if !edit.is_invalid() {
                    let hidden = CreateRectRgn(0, 0, 0, 0);
                    if SetWindowRgn(edit, Some(hidden), false) == 0 { let _ = DeleteObject(hidden.into()); }
                }
                let _ = InvalidateRect(Some(hwnd), None, true);
                if let Some(app) = app_of(hwnd) { let _ = app.borrow().log.record("native_surface_recovered"); }
            }
        }
        if let Some(delay) = delay {
            #[cfg(test)]
            super::surface_recovery_tests::record_timer();
            #[cfg(test)]
            let timer_failed = super::surface_recovery_tests::take_timer_failure();
            #[cfg(not(test))]
            let timer_failed = false;
            if timer_failed || SetTimer(Some(hwnd), TIMER, delay, None) == 0 {
                let _ = with_chrome(hwnd, |s| {
                    s.surface_recovery.phase.set(Phase::Exhausted);
                    s.surface_recovery.notice = Some(Fault::Timer);
                });
                // No message fallback that could grow a queue when the timer cannot be created.
                sync(hwnd);
            }
        }
        if !needs_emergency(hwnd) && !overlay.is_invalid() && IsWindowVisible(hwnd).as_bool() && !IsWindowVisible(overlay).as_bool() {
            let _ = ShowWindow(overlay, windows::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE);
        }
    }
    position_buttons(hwnd);
}

pub(super) fn needs_emergency(hwnd: HWND) -> bool {
    with_chrome(hwnd, |s| s.surface_recovery.needs_emergency()).unwrap_or(false)
}

unsafe fn button(hwnd: HWND, id: usize, title: windows::core::PCWSTR) -> HWND {
    #[cfg(test)]
    if super::surface_recovery_tests::take_button_failure() { return HWND::default(); }
    unsafe { CreateWindowExW(Default::default(), w!("BUTTON"), title, WS_CHILD | WS_VISIBLE,
        0, 0, 1, 1, Some(hwnd), Some(HMENU(id as *mut _)), None, None).unwrap_or_default() }
}

unsafe fn destroy_buttons(emergency: Emergency) {
    for button in [emergency.retry, emergency.close] {
        if !button.is_invalid() { unsafe { let _ = DestroyWindow(button); } }
    }
}

fn position_buttons(hwnd: HWND) {
    let font = app_of(hwnd).map(|app| app.borrow().input_font).unwrap_or_default();
    let Some((retry, close, size, dpi, exhausted)) = with_chrome(hwnd, |s| {
        let exhausted = s.surface_recovery.phase.get() == Phase::Exhausted;
        let e = s.surface_recovery.emergency.as_mut()?;
        let wanted = (s.client.0, s.client.1, s.dpi, font.0 as isize, exhausted);
        if e.applied == Some(wanted) { return None; }
        e.applied = Some(wanted);
        Some((e.retry, e.close, s.client, s.dpi, exhausted))
    }).flatten() else { return; };
    if !retry.is_invalid() { unsafe { let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(retry, exhausted); } }
    let m = Metrics::new(dpi);
    let rect = glass::CaptionButtons::for_client_width(size.0, dpi).close;
    for (button, bounds) in [
        (close, Rect::new(rect.left, rect.top, rect.right, rect.bottom)),
        (retry, Rect::new(m.px(8), (size.1 - m.px(36)).max(0), m.px(126), (size.1 - m.px(8)).max(1))),
    ] {
        if !button.is_invalid() { unsafe {
            if !font.is_invalid() {
                let _ = SendMessageW(button, windows::Win32::UI::WindowsAndMessaging::WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(0)));
            }
            let _ = SetWindowPos(button, Some(HWND_TOP), bounds.left, bounds.top, bounds.width(), bounds.height(), SWP_NOACTIVATE);
        } }
    }
}

/// No DIB, font or brush allocation is needed for the last-resort close route. Existing native
/// hit testing/Alt+F4 still work even if creating either standard BUTTON also fails.
pub(super) fn paint(hwnd: HWND, dc: HDC, dirty: RECT) {
    let Some((size, dpi)) = with_chrome(hwnd, |s| (s.client, s.dpi)) else { return; };
    unsafe {
        let _ = FillRect(dc, &dirty, GetSysColorBrush(COLOR_WINDOW));
        let copied = with_chrome(hwnd, |s| {
            s.surface.allocation_error().is_none()
                && (s.surface.width(), s.surface.height()) == s.client
                && s.surface.present_opaque(dc, Rect::from(dirty))
        }).unwrap_or(false);
        let captions = glass::CaptionButtons::for_client_width(size.0, dpi);
        for (r, kind) in [(captions.min, DFCS_CAPTIONMIN), (captions.max, DFCS_CAPTIONMAX), (captions.close, DFCS_CAPTIONCLOSE)] {
            let mut r = RECT { left: r.left, top: r.top, right: r.right, bottom: r.bottom };
            let _ = DrawFrameControl(dc, &mut r, DFC_CAPTION, kind);
        }
        if !copied {
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, windows::Win32::Foundation::COLORREF(GetSysColor(COLOR_WINDOWTEXT)));
            let m = Metrics::new(dpi);
            let mut r = RECT { left: m.px(12), top: m.px(64), right: size.0 - m.px(12), bottom: m.px(94) };
            let mut text: Vec<u16> = "Window drawing unavailable. Retry chrome or use Alt+F4 to close.".encode_utf16().collect();
            DrawTextW(dc, &mut text, &mut r, DT_LEFT | DT_SINGLELINE | DT_NOPREFIX);
        }
    }
}

/// Only this HWND's emergency presentation is changed. glass::apply restores its configured
/// backdrop after recovery. This also clears Win10's accent policy, as glass::apply_solid does.
pub(super) fn make_opaque(hwnd: HWND) {
    #[repr(C)]
    struct Accent { state: u32, flags: u32, color: u32, animation: u32 }
    #[repr(C)]
    struct Attribute { attribute: u32, data: *mut std::ffi::c_void, size: usize }
    unsafe {
        let none = 1_i32; // DWMSBT_NONE
        let _ = DwmSetWindowAttribute(hwnd, 38, (&none as *const i32).cast(), 4);
        let _ = DwmExtendFrameIntoClientArea(hwnd, [0_i32; 4].as_ptr());
        if let Ok(user32) = GetModuleHandleW(w!("user32.dll"))
            && let Some(function) = windows::Win32::System::LibraryLoader::GetProcAddress(user32, windows::core::s!("SetWindowCompositionAttribute"))
        {
            let mut accent = Accent { state: 0, flags: 2, color: 0, animation: 0 };
            let mut data = Attribute { attribute: 19, data: (&mut accent as *mut Accent).cast(), size: size_of::<Accent>() };
            let function: unsafe extern "system" fn(HWND, *mut Attribute) -> i32 = std::mem::transmute(function);
            let _ = function(hwnd, &mut data);
        }
    }
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(hwnd: HWND, attribute: u32, value: *const std::ffi::c_void, size: u32) -> i32;
    fn DwmExtendFrameIntoClientArea(hwnd: HWND, margins: *const i32) -> i32;
}
