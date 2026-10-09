use std::{cell::RefCell, rc::Rc};

use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            COLOR_WINDOW, CreateFontIndirectW, DeleteObject, GetSysColorBrush, HFONT, HGDIOBJ,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{EM_SETCUEBANNER, EM_SETMARGINS},
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::{
                GetKeyState, SetFocus, VK_LEFT, VK_MENU, VK_RETURN, VK_RIGHT,
            },
            Shell::{DefSubclassProc, SetWindowSubclass},
            WindowsAndMessaging::{
                CREATESTRUCTW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
                DispatchMessageW, EC_LEFTMARGIN, EC_RIGHTMARGIN, EN_CHANGE, ES_AUTOHSCROLL,
                GWLP_USERDATA, GetClientRect, GetMessageW, GetWindowTextLengthW, GetWindowTextW,
                HMENU, LoadCursorW, MSG, MoveWindow, NONCLIENTMETRICSW, PostQuitMessage,
                RegisterClassW, SIZE_MINIMIZED, SPI_GETNONCLIENTMETRICS, SW_SHOW,
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SendMessageW, SetWindowLongPtrW, ShowWindow,
                SystemParametersInfoW, TranslateMessage, WINDOW_STYLE, WM_CLOSE, WM_COMMAND,
                WM_CREATE, WM_DESTROY, WM_NCCREATE, WM_NCDESTROY, WM_SETFOCUS, WM_SETFONT, WM_SIZE,
                WNDCLASSW, WS_BORDER, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
            },
        },
    },
    core::w,
};

use super::{INPUT_ID, WM_HISTORY_BACK, WM_HISTORY_FORWARD, WM_SUBMIT};
use crate::app::{App, AppRef, layout_client};

const CLASS_NAME: windows::core::PCWSTR = w!("BowserNativeWindow");

pub fn dpi_scale(hwnd: HWND) -> u32 {
    (unsafe { GetDpiForWindow(hwnd) }.max(96) / 96).max(1)
}

fn message_font(hwnd: HWND) -> HFONT {
    let mut metrics = NONCLIENTMETRICSW {
        cbSize: size_of::<NONCLIENTMETRICSW>() as u32,
        ..Default::default()
    };
    unsafe {
        if SystemParametersInfoW(
            SPI_GETNONCLIENTMETRICS,
            metrics.cbSize,
            Some((&mut metrics as *mut NONCLIENTMETRICSW).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_err()
        {
            return HFONT::default();
        }
        let mut logfont = metrics.lfMessageFont;
        logfont.lfHeight = logfont.lfHeight * GetDpiForWindow(hwnd).max(96) as i32 / 96;
        CreateFontIndirectW(&logfont)
    }
}

pub fn run(app: App) -> windows::core::Result<()> {
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None)?.0);
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hCursor: LoadCursorW(None, windows::Win32::UI::WindowsAndMessaging::IDC_ARROW)?,
            hbrBackground: GetSysColorBrush(COLOR_WINDOW),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(windows::core::Error::from_thread());
        }
        let app = Rc::new(RefCell::new(app));
        let hwnd = CreateWindowExW(
            Default::default(),
            CLASS_NAME,
            w!("Bowser 0.0.1"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1000,
            720,
            None,
            None,
            Some(instance),
            Some((&app as *const AppRef).cast()),
        )?;
        app.borrow_mut().hwnd = hwnd;
        let _ = ShowWindow(hwnd, SW_SHOW);
        app.borrow().log.record("window_visible").ok();
        if !app.borrow().edit.is_invalid() {
            app.borrow().log.record("input_ready").ok();
        }
        let benchmark_url = { app.borrow_mut().benchmark_url.take() };
        if let Some(url) = benchmark_url {
            App::submit_benchmark_url(&app, url);
        }

        let mut message = MSG::default();
        loop {
            match GetMessageW(&mut message, None, 0, 0).0 {
                -1 => return Err(windows::core::Error::from_thread()),
                0 => break,
                _ => {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
        }
        Ok(())
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            let source = create.lpCreateParams as *const Rc<RefCell<App>>;
            if !source.is_null() {
                let data = clone_for_window(&*source);
                let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, data as isize);
                (*(*data)).borrow_mut().hwnd = hwnd;
            }
        }
        let raw = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA)
            as *const Rc<RefCell<App>>;
        let app = raw.as_ref();
        match message {
            WM_CREATE => {
                if let Some(app) = app {
                    let instance = HINSTANCE(GetModuleHandleW(None).unwrap_or_default().0);
                    let edit = match CreateWindowExW(
                        Default::default(),
                        w!("EDIT"),
                        w!(""),
                        WS_CHILD | WS_VISIBLE | WS_BORDER | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                        0,
                        0,
                        1,
                        36,
                        Some(hwnd),
                        Some(HMENU(INPUT_ID as usize as *mut std::ffi::c_void)),
                        Some(instance),
                        None,
                    ) {
                        Ok(edit) => edit,
                        Err(error) => {
                            super::show_error_message(&format!(
                                "Bowser could not create its address input: {error}"
                            ));
                            return LRESULT(-1);
                        }
                    };
                    let font = message_font(hwnd);
                    SendMessageW(
                        edit,
                        WM_SETFONT,
                        Some(WPARAM(font.0 as usize)),
                        Some(LPARAM(1)),
                    );
                    let margin = 6 * dpi_scale(hwnd) as isize;
                    SendMessageW(
                        edit,
                        EM_SETMARGINS,
                        Some(WPARAM((EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize)),
                        Some(LPARAM(margin | (margin << 16))),
                    );
                    SendMessageW(
                        edit,
                        EM_SETCUEBANNER,
                        Some(WPARAM(1)),
                        Some(LPARAM(w!("Search or enter address").0 as isize)),
                    );
                    if !SetWindowSubclass(edit, Some(edit_subclass), 1, hwnd.0 as usize).as_bool() {
                        super::show_error_message(
                            "Bowser could not install keyboard handling for its address input.",
                        );
                        return LRESULT(-1);
                    }
                    let mut state = app.borrow_mut();
                    state.edit = edit;
                    state.input_font = font;
                    drop(state);
                    let _ = SetFocus(Some(edit));
                }
                LRESULT(0)
            }
            WM_SIZE => {
                if let Some(app) = app {
                    let is_minimized = wparam.0 as u32 == SIZE_MINIMIZED;
                    let mut rect = RECT::default();
                    let _ = GetClientRect(hwnd, &mut rect);
                    let layout = layout_client(
                        rect.right - rect.left,
                        rect.bottom - rect.top,
                        dpi_scale(hwnd),
                    );
                    let state = app.borrow();
                    let edit = state.edit;
                    drop(state);
                    if !is_minimized {
                        let _ = MoveWindow(
                            edit,
                            layout.input_x,
                            layout.input_y,
                            layout.input_width,
                            layout.input_height,
                            true,
                        );
                    }
                    App::window_size_changed(
                        app,
                        is_minimized,
                        RECT {
                            left: layout.viewport_x,
                            top: layout.viewport_y,
                            right: layout.viewport_x + layout.viewport_width,
                            bottom: layout.viewport_y + layout.viewport_height,
                        },
                    );
                }
                LRESULT(0)
            }
            WM_COMMAND
                if (wparam.0 & 0xffff) as u16 == INPUT_ID
                    && (wparam.0 >> 16) as u32 == EN_CHANGE =>
            {
                if let Some(app) = app {
                    let edit = app.borrow().edit;
                    let length = GetWindowTextLengthW(edit).max(0) as usize;
                    let mut buffer = vec![0u16; length + 1];
                    let copied = GetWindowTextW(edit, &mut buffer) as usize;
                    let text = String::from_utf16_lossy(&buffer[..copied]);
                    App::prewarm(app, &text);
                }
                LRESULT(0)
            }
            WM_COMMAND if (wparam.0 & 0xffff) as u16 == INPUT_ID => LRESULT(0),
            WM_SUBMIT => {
                if let Some(app) = app {
                    let edit = app.borrow().edit;
                    let length = GetWindowTextLengthW(edit).max(0) as usize;
                    let mut buffer = vec![0u16; length + 1];
                    let copied = GetWindowTextW(edit, &mut buffer) as usize;
                    let text = String::from_utf16_lossy(&buffer[..copied]);
                    App::submit(app, &text);
                }
                LRESULT(0)
            }
            WM_HISTORY_BACK => {
                if let Some(app) = app {
                    App::history(app, false);
                }
                LRESULT(0)
            }
            WM_HISTORY_FORWARD => {
                if let Some(app) = app {
                    App::history(app, true);
                }
                LRESULT(0)
            }
            WM_SETFOCUS => {
                if let Some(app) = app {
                    let _ = SetFocus(Some(app.borrow().edit));
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                if let Some(app) = app {
                    let (mut engine, log) = {
                        let mut state = app.borrow_mut();
                        state.closing = true;
                        (state.engine.take(), state.log.clone())
                    };
                    if let Some(engine) = engine.as_mut() {
                        let _ = engine.close();
                    }
                    let _ = log.record("shutdown");
                }
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                if let Some(app) = app {
                    let mut state = app.borrow_mut();
                    if !state.input_font.is_invalid() {
                        let _ = DeleteObject(HGDIOBJ(state.input_font.0));
                    }
                    if state.initializing {
                        state.loop_shutdown_pending = true;
                    } else {
                        PostQuitMessage(0);
                    }
                } else {
                    PostQuitMessage(0);
                }
                LRESULT(0)
            }
            WM_NCDESTROY => {
                let data =
                    windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA)
                        as *mut Rc<RefCell<App>>;
                let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                if !data.is_null() {
                    drop(Box::from_raw(data));
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

fn clone_for_window<T: Clone>(source: &T) -> *mut T {
    Box::into_raw(Box::new(source.clone()))
}

unsafe extern "system" fn edit_subclass(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    parent: usize,
) -> LRESULT {
    unsafe {
        let alt = GetKeyState(VK_MENU.0 as i32) < 0;
        if message == windows::Win32::UI::WindowsAndMessaging::WM_KEYDOWN
            && wparam.0 == VK_RETURN.0 as usize
        {
            super::post(HWND(parent as *mut _), WM_SUBMIT, WPARAM(0), LPARAM(0));
            return LRESULT(0);
        }
        if message == windows::Win32::UI::WindowsAndMessaging::WM_SYSKEYDOWN && alt {
            if wparam.0 == VK_LEFT.0 as usize {
                super::post(
                    HWND(parent as *mut _),
                    WM_HISTORY_BACK,
                    WPARAM(0),
                    LPARAM(0),
                );
                return LRESULT(0);
            }
            if wparam.0 == VK_RIGHT.0 as usize {
                super::post(
                    HWND(parent as *mut _),
                    WM_HISTORY_FORWARD,
                    WPARAM(0),
                    LPARAM(0),
                );
                return LRESULT(0);
            }
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn window_userdata_owns_a_clone_independent_of_createwindow_caller() {
        let source = Rc::new(Cell::new(4));
        let owned = clone_for_window(&source);
        unsafe {
            (*owned).set(8);
        }
        assert_eq!(source.get(), 8);
        unsafe {
            drop(Box::from_raw(owned));
        }
        source.set(12);
        assert_eq!(source.get(), 12);
    }
}
