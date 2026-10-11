//! Small themed profile-name editor using the same native EDIT control behavior as island rename.

use std::{cell::Cell, ffi::c_void};

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
    register_class()?;
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
    let width = scale(420, owner);
    let height = scale(160, owner);
    let x = (owner_rect.left + owner_rect.right - width) / 2;
    let y = (owner_rect.top + owner_rect.bottom - height) / 2;
    let instance = unsafe { HINSTANCE(GetModuleHandleW(None).ok()?.0) };
    let hwnd = unsafe {
        match CreateWindowExW(
            WS_EX_TOOLWINDOW,
            CLASS,
            PCWSTR(wide(title).as_ptr()),
            WS_POPUP | WS_VISIBLE,
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
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(state.edit));
    }
    while !state.done {
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
                    finish(state, wparam.0 != 0);
                }
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_NCDESTROY => {
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
        fill_button(dc, CANCEL, "Cancel", &palette, hwnd);
        fill_button(dc, SAVE, &save_label, &palette, hwnd);
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
