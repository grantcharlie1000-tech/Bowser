//! Name prompt (rename profile/workspace, new workspace, save session). Synchronous for callers.
//!
//! With the HTML chrome the prompt is a dialog page (`?view=dialog&p=rename`) hosted in this
//! window by `html_dialog` (input, inline validation message, Save/Cancel). The window stays hidden
//! until the page is ready. NATIVE FALLBACK: when the HTML chrome is off, or the page is not ready
//! within `HTML_READY_TIMEOUT_MS`, the window shows its own GDI-painted EDIT control instead.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ffi::c_void,
};

use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect, HBRUSH,
            PAINTSTRUCT, SetBkColor, SetBkMode, SetTextColor, TRANSPARENT,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::SetFocus,
            Shell::{DefSubclassProc, SetWindowSubclass},
            WindowsAndMessaging::{
                CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                ES_AUTOHSCROLL, GWLP_USERDATA, GetClientRect, GetMessageW, GetWindowLongPtrW,
                GetWindowRect, GetWindowTextLengthW, GetWindowTextW, IDC_ARROW, LoadCursorW, MSG,
                RegisterClassW, SetForegroundWindow, SetWindowLongPtrW, SetWindowTextW,
                TranslateMessage, WINDOW_STYLE, WM_APP, WM_CTLCOLOREDIT, WM_ERASEBKGND, WM_KEYDOWN,
                WM_LBUTTONUP, WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WNDCLASSW, WS_CHILD,
                WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE,
            },
        },
    },
    core::{PCWSTR, w},
};

use crate::{
    theme,
    win32::{glass, smooth},
};

const CLASS: PCWSTR = w!("BowserProfileRename");
const EDIT_SUBCLASS: usize = 0x7A21;
const WM_FINISH: u32 = WM_APP + 0x51;
// --- popup surfaces (lane PP) ---
/// `WM_FINISH` wParam: the HTML page already stored the validated result.
const FINISH_STORED: usize = 2;
/// Timer that gives up on the HTML page and shows the native controls.
const HTML_READY_TIMER: usize = 0x7A22;
const HTML_READY_TIMEOUT_MS: u32 = 2500;
/// Logical size of the HTML dialog (title, input, message line, buttons).
const HTML_WIDTH: i32 = 400;
const HTML_HEIGHT: i32 = 200;

/// What the HTML page shows, keyed by the dialog window.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Request {
    pub title: String,
    pub save: String,
    pub value: String,
    pub hint: String,
    pub error: String,
    pub max: usize,
}

/// Checks a submitted name; the error text is shown under the input.
pub(crate) type Validator = Box<dyn Fn(&str) -> Result<(), String>>;

struct HtmlPrompt {
    request: Request,
    validate: Validator,
    result: Option<String>,
    shown: bool,
}

thread_local! { static HTML_PROMPTS: RefCell<HashMap<isize, HtmlPrompt>> = RefCell::new(HashMap::new()); }

/// The default check: what every caller stores (workspace and session names). Profile renames add
/// the profile-name rules (`prompt_validated`).
pub(crate) fn validate_plain(name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Enter a name.".to_owned());
    }
    if name.len() > 256 {
        return Err("Use at most 256 characters.".to_owned());
    }
    if name.chars().any(char::is_control) {
        return Err("Names cannot contain control characters.".to_owned());
    }
    Ok(())
}
// --- end popup surfaces ---
const SAVE: RECT = RECT {
    left: 284,
    top: 100,
    right: 388,
    bottom: 136,
};
const CANCEL: RECT = RECT {
    left: 168,
    top: 100,
    right: 272,
    bottom: 136,
};

struct Dialog {
    edit: HWND,
    result: Option<String>,
    done: bool,
    brush: HBRUSH,
    caption: String,
    save_label: String,
}

thread_local! { static REGISTERED: Cell<bool> = const { Cell::new(false) }; }

pub(crate) fn prompt(owner: HWND, current: &str, title: &str, save_label: &str) -> Option<String> {
    prompt_validated(owner, current, title, save_label, "", Box::new(validate_plain))
}

/// `prompt` with a caller check (shown inline in the HTML dialog) and a hint line under the input.
pub(crate) fn prompt_validated(
    owner: HWND,
    current: &str,
    title: &str,
    save_label: &str,
    hint: &str,
    validate: Validator,
) -> Option<String> {
    register_class()?;
    // --- popup surfaces (lane PP) ---
    let html = crate::chrome_host::html_ui_enabled() && super::window::html_environment(owner).is_some();
    // --- end popup surfaces ---
    let mut state = Box::new(Dialog {
        edit: HWND::default(),
        result: None,
        done: false,
        brush: unsafe {
            CreateSolidBrush(colorref(
                theme::palette(glass::appearance(), true).omnibox_fill,
            ))
        },
        caption: title.to_owned(),
        save_label: save_label.to_owned(),
    });
    let mut owner_rect = RECT::default();
    unsafe {
        let _ = GetWindowRect(owner, &mut owner_rect);
    }
    let (width, height) = if html {
        (scale(HTML_WIDTH, owner), scale(HTML_HEIGHT, owner))
    } else {
        (scale(420, owner), scale(160, owner))
    };
    let x = (owner_rect.left + owner_rect.right - width) / 2;
    let y = (owner_rect.top + owner_rect.bottom - height) / 2;
    let instance = unsafe { HINSTANCE(GetModuleHandleW(None).ok()?.0) };
    let hwnd = unsafe {
        match CreateWindowExW(
            WS_EX_TOOLWINDOW,
            CLASS,
            PCWSTR(wide(title).as_ptr()),
            if html {
                WS_POPUP | windows::Win32::UI::WindowsAndMessaging::WS_CLIPCHILDREN
            } else {
                WS_POPUP | WS_VISIBLE
            },
            x,
            y,
            width,
            height,
            Some(owner),
            None,
            Some(instance),
            Some((&mut *state as *mut Dialog).cast::<c_void>()),
        ) {
            Ok(hwnd) => hwnd,
            Err(_) => {
                let _ = DeleteObject(state.brush.into());
                return None;
            }
        }
    };
    unsafe {
        let _ = SetWindowTextW(state.edit, PCWSTR(wide(current).as_ptr()));
    }
    // --- popup surfaces (lane PP) ---
    if html {
        HTML_PROMPTS.with(|prompts| {
            prompts.borrow_mut().insert(
                hwnd.0 as isize,
                HtmlPrompt {
                    request: Request {
                        title: title.to_owned(),
                        save: save_label.to_owned(),
                        value: current.to_owned(),
                        hint: hint.to_owned(),
                        error: String::new(),
                        max: 256,
                    },
                    validate,
                    result: None,
                    shown: false,
                },
            );
        });
        unsafe {
            let radius = scale(8, hwnd) * 2;
            let region = windows::Win32::Graphics::Gdi::CreateRoundRectRgn(0, 0, width + 1, height + 1, radius, radius);
            if !region.is_invalid() {
                let _ = windows::Win32::Graphics::Gdi::SetWindowRgn(hwnd, Some(region), false);
            }
            let _ = windows::Win32::UI::WindowsAndMessaging::SetTimer(Some(hwnd), HTML_READY_TIMER, HTML_READY_TIMEOUT_MS, None);
        }
        super::window::attach_html_dialog(owner, hwnd, super::html_dialog::Kind::Rename);
    } else {
        show_native(hwnd, state.edit);
    }
    // --- end popup surfaces ---
    loop {
        // DispatchMessageW invokes the window procedure, which mutates this owned state.
        // Re-read after each dispatch; do not cache the callback-controlled completion flag.
        if state.done { break; }
        let mut msg = MSG::default();
        let got = unsafe { GetMessageW(&mut msg, None, 0, 0).0 };
        if got <= 0 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    if unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(hwnd)).as_bool() } {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
    unsafe {
        let _ = DeleteObject(state.brush.into());
    }
    state.result.take()
}

fn show_native(hwnd: HWND, edit: HWND) {
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(edit));
    }
}

// --- popup surfaces (lane PP) ---
/// What the HTML page should show for the dialog `parent`.
pub(crate) fn html_request(parent: HWND) -> Option<Request> {
    HTML_PROMPTS.with(|prompts| prompts.borrow().get(&(parent.0 as isize)).map(|p| p.request.clone()))
}

/// The page is ready: show the dialog with the page in place of the native controls. False when
/// the native fallback is already showing (the page is then closed by the caller).
pub(crate) fn html_ready(parent: HWND) -> bool {
    let first = HTML_PROMPTS.with(|prompts| {
        prompts.borrow_mut().get_mut(&(parent.0 as isize)).is_some_and(|p| !std::mem::replace(&mut p.shown, true))
    });
    if !first {
        return HTML_PROMPTS.with(|prompts| prompts.borrow().contains_key(&(parent.0 as isize)));
    }
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(parent), HTML_READY_TIMER);
        if let Some(state) = dialog(parent) {
            let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(state.edit, windows::Win32::UI::WindowsAndMessaging::SW_HIDE);
        }
        let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(parent, windows::Win32::UI::WindowsAndMessaging::SW_SHOW);
        let _ = SetForegroundWindow(parent);
    }
    true
}

/// Save from the page. Valid names close the dialog with that name; otherwise the error is stored
/// for the page to show and returned.
pub(crate) fn html_submit(parent: HWND, text: &str) -> Result<(), String> {
    let key = parent.0 as isize;
    let outcome = HTML_PROMPTS.with(|prompts| {
        let mut prompts = prompts.borrow_mut();
        let prompt = prompts.get_mut(&key).ok_or_else(|| "The dialog is closed.".to_owned())?;
        let name = text.trim();
        prompt.request.value = text.to_owned();
        match check(name, prompt.request.max, &prompt.validate) {
            Ok(()) => {
                prompt.result = Some(name.to_owned());
                prompt.request.error.clear();
                Ok(())
            }
            Err(error) => {
                prompt.request.error.clone_from(&error);
                Err(error)
            }
        }
    });
    if outcome.is_ok() {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(Some(parent), WM_FINISH, WPARAM(FINISH_STORED), LPARAM(0));
        }
    }
    outcome
}

fn check(name: &str, max: usize, validate: &Validator) -> Result<(), String> {
    if name.chars().count() > max {
        return Err(format!("Use at most {max} characters."));
    }
    validate_plain(name)?;
    validate(name)
}

/// Cancel or Escape from the page.
pub(crate) fn html_cancel(parent: HWND) {
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(Some(parent), WM_FINISH, WPARAM(0), LPARAM(0));
    }
}

/// The page failed (or never became ready): show the native controls if the dialog is still open.
pub(crate) fn html_failed(parent: HWND) {
    let pending = HTML_PROMPTS.with(|prompts| {
        prompts.borrow_mut().get_mut(&(parent.0 as isize)).map(|p| !std::mem::replace(&mut p.shown, true))
    });
    if pending == Some(true) {
        super::html_dialog::close(parent);
        if let Some(state) = dialog(parent) {
            show_native(parent, state.edit);
        }
    } else if pending == Some(false) {
        // The page was showing and died: fall back to the native controls in place.
        super::html_dialog::close(parent);
        if let Some(state) = dialog(parent) {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(state.edit, windows::Win32::UI::WindowsAndMessaging::SW_SHOW);
                let _ = windows::Win32::Graphics::Gdi::SetWindowRgn(parent, None, true);
            }
            show_native(parent, state.edit);
        }
    }
}
// --- end popup surfaces ---

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

fn colorref(colour: theme::Rgba) -> windows::Win32::Foundation::COLORREF {
    windows::Win32::Foundation::COLORREF(
        u32::from(colour.r) | (u32::from(colour.g) << 8) | (u32::from(colour.b) << 16),
    )
}

fn register_class() -> Option<()> {
    REGISTERED.with(|registered| {
        if registered.get() {
            return Some(());
        }
        let instance = unsafe { HINSTANCE(GetModuleHandleW(None).ok()?.0) };
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance,
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW).ok()? },
            lpszClassName: CLASS,
            ..Default::default()
        };
        if unsafe { RegisterClassW(&class) } == 0 {
            return None;
        }
        registered.set(true);
        Some(())
    })
}

fn dialog(hwnd: HWND) -> Option<&'static mut Dialog> {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Dialog;
    unsafe { pointer.as_mut() }
}

unsafe extern "system" fn proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match message {
            WM_NCCREATE => {
                let create = &*(lparam.0 as *const CREATESTRUCTW);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            0x0001 => {
                if let Some(state) = dialog(hwnd) {
                    state.edit = match CreateWindowExW(
                        Default::default(),
                        w!("EDIT"),
                        PCWSTR::null(),
                        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                        scale(24, hwnd),
                        scale(54, hwnd),
                        scale(372, hwnd),
                        scale(30, hwnd),
                        Some(hwnd),
                        None,
                        Some(HINSTANCE(GetModuleHandleW(None).unwrap_or_default().0)),
                        None,
                    ) {
                        Ok(edit) => edit,
                        Err(_) => {
                            state.done = true;
                            return LRESULT(0);
                        }
                    };
                    let _ = SetWindowSubclass(
                        state.edit,
                        Some(edit_proc),
                        EDIT_SUBCLASS,
                        hwnd.0 as usize,
                    );
                }
                LRESULT(0)
            }
            WM_CTLCOLOREDIT => {
                if let Some(state) = dialog(hwnd) {
                    SetBkColor(
                        windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut c_void),
                        colorref(theme::palette(glass::appearance(), true).omnibox_fill),
                    );
                    SetTextColor(
                        windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut c_void),
                        colorref(theme::palette(glass::appearance(), true).omnibox_text),
                    );
                    LRESULT(state.brush.0 as isize)
                } else {
                    LRESULT(0)
                }
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let x = (lparam.0 as u16) as i32;
                let y = ((lparam.0 >> 16) as u16) as i32;
                if let Some(state) = dialog(hwnd) {
                    if inside(scaled_rect(SAVE, hwnd), x, y) {
                        finish(state, true);
                        let _ = DestroyWindow(hwnd);
                    } else if inside(scaled_rect(CANCEL, hwnd), x, y) {
                        finish(state, false);
                        let _ = DestroyWindow(hwnd);
                    }
                }
                LRESULT(0)
            }
            WM_FINISH => {
                if let Some(state) = dialog(hwnd) {
                    if wparam.0 == FINISH_STORED {
                        // --- popup surfaces (lane PP) ---
                        state.result = HTML_PROMPTS.with(|prompts| {
                            prompts.borrow_mut().get_mut(&(hwnd.0 as isize)).and_then(|p| p.result.take())
                        });
                        state.done = true;
                        // --- end popup surfaces ---
                    } else {
                        finish(state, wparam.0 != 0);
                    }
                }
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            windows::Win32::UI::WindowsAndMessaging::WM_TIMER if wparam.0 == HTML_READY_TIMER => {
                let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(hwnd), HTML_READY_TIMER);
                html_failed(hwnd);
                LRESULT(0)
            }
            windows::Win32::UI::WindowsAndMessaging::WM_SIZE => {
                super::html_dialog::resize(hwnd);
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            WM_NCDESTROY => {
                // --- popup surfaces (lane PP) ---
                super::html_dialog::close(hwnd);
                HTML_PROMPTS.with(|prompts| prompts.borrow_mut().remove(&(hwnd.0 as isize)));
                // --- end popup surfaces ---
                if let Some(state) = dialog(hwnd) {
                    state.done = true;
                }
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

fn finish(state: &mut Dialog, save: bool) {
    state.result = save
        .then(|| {
            let count = unsafe { GetWindowTextLengthW(state.edit).max(0) as usize };
            let mut value = vec![0u16; count + 1];
            let written = unsafe { GetWindowTextW(state.edit, &mut value).max(0) as usize };
            String::from_utf16_lossy(&value[..written])
                .trim()
                .to_owned()
        })
        .filter(|name| !name.is_empty());
    state.done = true;
}

unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    parent: usize,
) -> LRESULT {
    if message == WM_KEYDOWN && (wparam.0 == 0x0D || wparam.0 == 0x1B) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                Some(HWND(parent as *mut c_void)),
                WM_FINISH,
                WPARAM(usize::from(wparam.0 == 0x0D)),
                LPARAM(0),
            );
        }
        return LRESULT(0);
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

fn inside(rect: RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let palette = theme::palette(glass::appearance(), true);
        let brush = CreateSolidBrush(colorref(palette.window));
        FillRect(dc, &client, brush);
        let _ = DeleteObject(brush.into());
        smooth::fill_round_rect(
            dc,
            RECT {
                left: 0,
                top: 0,
                right: client.right - 1,
                bottom: client.bottom - 1,
            },
            scale(12, hwnd),
            palette.window,
        );
        smooth::stroke_round_rect(
            dc,
            RECT {
                left: 0,
                top: 0,
                right: client.right - 1,
                bottom: client.bottom - 1,
            },
            scale(12, hwnd),
            palette.hairline,
            1,
        );
        SetBkMode(dc, TRANSPARENT);
        let (caption, save_label) = dialog(hwnd).map_or_else(
            || ("Rename profile".to_owned(), "Save".to_owned()),
            |state| (state.caption.clone(), state.save_label.clone()),
        );
        text(
            dc,
            RECT {
                left: scale(24, hwnd),
                top: scale(12, hwnd),
                right: scale(396, hwnd),
                bottom: scale(44, hwnd),
            },
            &caption,
            palette.tab_text_active,
        );
        fill_button(dc, CANCEL, "Cancel", palette, hwnd);
        fill_button(dc, SAVE, &save_label, palette, hwnd);
        let _ = EndPaint(hwnd, &ps);
    }
}

fn fill_button(
    dc: windows::Win32::Graphics::Gdi::HDC,
    bounds: RECT,
    label: &str,
    palette: &theme::Palette,
    hwnd: HWND,
) {
    unsafe {
        let bounds = RECT {
            left: scale(bounds.left, hwnd),
            top: scale(bounds.top, hwnd),
            right: scale(bounds.right, hwnd),
            bottom: scale(bounds.bottom, hwnd),
        };
        smooth::fill_round_rect(dc, bounds, scale(6, hwnd), palette.tab_hover);
        text(dc, bounds, label, palette.tab_text_active);
    }
}

fn text(
    dc: windows::Win32::Graphics::Gdi::HDC,
    mut bounds: RECT,
    value: &str,
    colour: theme::Rgba,
) {
    let mut wide = value.encode_utf16().collect::<Vec<_>>();
    if wide.is_empty() {
        return;
    }
    unsafe {
        SetTextColor(dc, colorref(colour));
        let _ = DrawTextW(
            dc,
            &mut wide,
            &mut bounds,
            windows::Win32::Graphics::Gdi::DT_CENTER
                | windows::Win32::Graphics::Gdi::DT_VCENTER
                | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
        );
    }
}

fn scale(value: i32, hwnd: HWND) -> i32 {
    unsafe { value * GetDpiForWindow(hwnd).max(96) as i32 / 96 }
}

fn scaled_rect(bounds: RECT, hwnd: HWND) -> RECT {
    RECT {
        left: scale(bounds.left, hwnd),
        top: scale(bounds.top, hwnd),
        right: scale(bounds.right, hwnd),
        bottom: scale(bounds.bottom, hwnd),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_trimmed_capped_and_checked_by_the_caller_rules() {
        let plain: Validator = Box::new(validate_plain);
        assert!(check("Work", 256, &plain).is_ok());
        assert_eq!(check("", 256, &plain), Err("Enter a name.".to_owned()));
        assert!(check("a\u{7}b", 256, &plain).is_err());
        assert!(check(&"x".repeat(257), 256, &plain).is_err());
        let profile: Validator = Box::new(|name| crate::profile::validate_name(name).map_err(|e| e.to_string()));
        assert!(check("Work_2", 256, &profile).is_ok());
        assert_eq!(
            check("My work", 256, &profile),
            Err("Profile names use 1 to 32 letters, digits, '-' or '_'.".to_owned())
        );
    }
}
