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

fn main() -> ExitCode {
    let options = match LaunchOptions::parse() {
        Ok(options) => options,
        Err(error) => {
            show_error(&error);
            return ExitCode::FAILURE;
        }
    };
    let benchmark = options.benchmark_log.is_some();
    if bowser::updater::startup(benchmark) == bowser::updater::StartupAction::Relaunch
        && bowser::updater::relaunch()
    {
        return ExitCode::SUCCESS;
    }
    bowser::updater::spawn_background(benchmark);
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
