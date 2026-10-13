#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    backtrace::Backtrace,
    fs::{self, OpenOptions},
    io::Write,
    panic::{AssertUnwindSafe, PanicHookInfo},
    process::ExitCode,
    rc::Rc,
};

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
    install_panic_hook();
    let options = match LaunchOptions::parse() {
        Ok(options) => options,
        Err(error) => {
            show_error(&error);
            return ExitCode::FAILURE;
        }
    };
    // The single authoritative app-data root. Installed before anything touches persistent state;
    // benchmark launches always have one (never the real %LOCALAPPDATA%\Bowser).
    if let Some(dir) = &options.data_dir {
        if let Err(error) = bowser::profile::set_data_root(dir.clone()) {
            show_error(&format!("Cannot create the data directory: {error}"));
            return ExitCode::FAILURE;
        }
    } else if options.benchmark_log.is_some() {
        show_error("Benchmark mode has no data directory; refusing to use the real profile.");
        return ExitCode::FAILURE;
    }
    let options_ephemeral = options.data_dir_ephemeral;
    let benchmark = options.benchmark_log.is_some();
    bowser::profile::set_dev_launch(options.dev);
    if bowser::updater::startup(benchmark, options.dev) == bowser::updater::StartupAction::Relaunch
        && bowser::updater::relaunch()
    {
        return ExitCode::SUCCESS;
    }
    bowser::updater::spawn_background(benchmark, options.dev);
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
    if result.is_ok() {
        bowser::updater::note_clean_exit();
    }
    unsafe {
        CoUninitialize();
    }
    if options_ephemeral {
        if let Some(dir) = bowser::profile::data_root_override() {
            let _ = fs::remove_dir_all(dir);
        }
    } else if result.is_ok() && bowser::updater::restart_requested() {
        // "Restart to update", once this process has let go of COM and its profile. The new
        // process installs the staged build as it starts.
        let _ = bowser::updater::relaunch_for_restart();
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

const MAX_CRASH_LOG_BYTES: u64 = 256 * 1024;

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| append_crash_log(info)));
        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| previous(info)));
    }));
}

fn append_crash_log(info: &PanicHookInfo<'_>) {
    let Some(directory) = bowser::profile::app_data_dir() else {
        return;
    };
    if fs::create_dir_all(&directory).is_err() {
        return;
    }

    let message = if let Some(message) = info.payload().downcast_ref::<&str>() {
        *message
    } else if let Some(message) = info.payload().downcast_ref::<String>() {
        message.as_str()
    } else {
        "<non-string panic payload>"
    };
    let location = info
        .location()
        .map(|location| {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        })
        .unwrap_or_else(|| "<unknown>".to_owned());
    let mut entry = format!(
        "Panic: {message}\nLocation: {location}\nBacktrace:\n{}\n",
        Backtrace::force_capture()
    );
    if entry.len() > MAX_CRASH_LOG_BYTES as usize {
        let mut end = MAX_CRASH_LOG_BYTES as usize;
        while !entry.is_char_boundary(end) {
            end -= 1;
        }
        entry.truncate(end);
    }
    let path = directory.join("crash.log");
    let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(path)
    else {
        return;
    };
    let current_len = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    if current_len.saturating_add(entry.len() as u64) > MAX_CRASH_LOG_BYTES
        && file.set_len(0).is_err()
    {
        return;
    }
    let _ = file.write_all(entry.as_bytes());
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
