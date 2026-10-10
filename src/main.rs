#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{process::ExitCode, rc::Rc};

use bowser::{
    app::App,
    performance::{LaunchOptions, PerfLog},
};
use windows::{
    Win32::{
        System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
        UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW},
    },
    core::w,
};

// TEMPORARY G3h crash diagnosis: logs the first-chance access violation with a backtrace.
// REMOVE before finishing.
#[repr(C)]
struct DiagExceptionRecord {
    code: u32,
    flags: u32,
    nested: *const std::ffi::c_void,
    address: *const std::ffi::c_void,
    parameter_count: u32,
    information: [usize; 15],
}

#[repr(C)]
struct DiagExceptionPointers {
    record: *const DiagExceptionRecord,
    context: *const std::ffi::c_void,
}

unsafe extern "system" {
    fn AddVectoredExceptionHandler(
        first: u32,
        handler: unsafe extern "system" fn(*mut DiagExceptionPointers) -> i32,
    ) -> *mut std::ffi::c_void;
}

unsafe extern "system" fn diag_exception(info: *mut DiagExceptionPointers) -> i32 {
    unsafe {
        let record = &*(*info).record;
        if record.code == 0xc000_0005 || record.code == 0xc000_041d {
            use std::io::Write;
            let path = std::env::temp_dir().join("bowser-panic.txt");
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(
                    file,
                    "EXCEPTION code={:#x} address={:p} access={:?} target={:#x}\nBACKTRACE:\n{}",
                    record.code,
                    record.address,
                    record.information[0],
                    record.information[1],
                    std::backtrace::Backtrace::force_capture()
                );
            }
        }
    }
    0
}

fn main() -> ExitCode {
    unsafe {
        AddVectoredExceptionHandler(1, diag_exception);
    }
    std::panic::set_hook(Box::new(|info| {
        use std::io::Write;
        let path = std::env::temp_dir().join("bowser-panic.txt");
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(
                file,
                "PANIC: {info}\nBACKTRACE:\n{}",
                std::backtrace::Backtrace::force_capture()
            );
        }
    }));
    let options = match LaunchOptions::parse() {
        Ok(options) => options,
        Err(error) => {
            show_error(&error);
            return ExitCode::FAILURE;
        }
    };
    let log = match PerfLog::new(options.benchmark_log.as_deref()) {
        Ok(log) => Rc::new(log),
        Err(error) => {
            show_error(&format!("Unable to create benchmark log: {error}"));
            return ExitCode::FAILURE;
        }
    };
    let _ = log.record("process_start");
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok();
    if let Err(error) = initialized {
        show_error(&format!(
            "Bowser could not initialize its Windows UI thread: {error}"
        ));
        return ExitCode::FAILURE;
    }
    let result = App::run(options, log.clone());
    unsafe {
        CoUninitialize();
    }
    if let Some(error) = log.failure() {
        show_error(&format!(
            "Bowser could not write its benchmark log: {error}"
        ));
        return ExitCode::FAILURE;
    }
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            show_error(&format!("Bowser could not start: {error}"));
            ExitCode::FAILURE
        }
    }
}

fn show_error(message: &str) {
    let text: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let _ = MessageBoxW(
            Some(windows::Win32::Foundation::HWND::default()),
            windows::core::PCWSTR(text.as_ptr()),
            w!("Bowser"),
            MB_OK | MB_ICONERROR,
        );
    }
}
