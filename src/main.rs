#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    backtrace::Backtrace,
    fs::{self, OpenOptions},
    io::Write,
    panic::PanicHookInfo,
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
            if agent_argv() {
                return agent_fail(&error);
            }
            show_error(&error);
            return ExitCode::FAILURE;
        }
    };
    if options.agent_call.is_some() {
        return agent_client(&options);
    }
    if bowser::agent::should_listen(options.agent) {
        return agent_server(options);
    }
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
    let log = match PerfLog::new(options.diagnostic_log.as_deref().or(options.benchmark_log.as_deref())) {
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
    let interaction_diagnostic=options.diagnostic_interaction;
    let result = App::run(options, log.clone());
    if interaction_diagnostic {let _=log.record_detail("interaction_app_borrow_drops","all",0,0,App::diagnostic_interaction_dropped());}
    if result.is_ok() {
        bowser::updater::note_clean_exit();
    }
    // WM_DESTROY normally does this; a GetMessage/run error can return without that message.
    // Idempotent teardown must release retained native profile subscriptions before COM uninit.
    bowser::win32::panel_webapp::shutdown();
    unsafe {
        CoUninitialize();
    }
    // A retained sidebar controller may retain App callbacks. Its final diagnostic evidence must
    // not depend on the last Rc<PerfLog> being dropped; ordinary/startup-only paths are inert.
    let _=log.checkpoint_sidebar_if_observed();
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

fn agent_client(options: &LaunchOptions) -> ExitCode {
    let parts = bowser::agent::CallParts {
        op: options.agent_call.clone().unwrap_or_default(),
        session: options.agent_session.clone(),
        url: options.agent_url.clone(),
        refer: options.agent_ref.clone(),
        selector: options.agent_selector.clone(),
        text: options.agent_text.clone(),
        key: options.agent_key.clone(),
        expression: options.agent_expression.clone(),
        format: options.agent_format.clone(),
    };
    let command = match bowser::agent::command_from_parts(&parts) {
        Ok(command) => command,
        Err(error) => return agent_fail(&error),
    };
    let port = options.agent_port.unwrap_or(bowser::agent::DEFAULT_PORT);
    let addr = match bowser::agent::parse_loopback_bind("127.0.0.1", port) {
        Ok(addr) => addr,
        Err(error) => return agent_fail(&error),
    };
    match bowser::agent::client_exchange(addr, &command) {
        Ok(body) => {
            println!("{body}");
            if bowser::agent::response_ok(&body) {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => agent_fail(&error),
    }
}

fn agent_fail(message: &str) -> ExitCode {
    eprintln!("{}", bowser::agent::err_json(message));
    ExitCode::FAILURE
}

fn agent_argv() -> bool {
    std::env::args().skip(1).any(|arg| {
        arg == "--agent" || arg == "--agent-call" || arg.starts_with("--agent-call=")
    })
}

fn agent_server(options: LaunchOptions) -> ExitCode {
    let Some(dir) = options.data_dir.clone() else {
        return agent_fail("--agent requires --data-dir");
    };
    if let Err(error) = bowser::profile::set_data_root(dir.clone()) {
        return agent_fail(&format!("Cannot create the data directory: {error}"));
    }
    let host = options.agent_bind_host.as_deref().unwrap_or("127.0.0.1");
    let port = options.agent_port.unwrap_or(bowser::agent::DEFAULT_PORT);
    let addr = match bowser::agent::parse_loopback_bind(host, port) {
        Ok(addr) => addr,
        Err(error) => return agent_fail(&error),
    };
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok();
    if let Err(error) = initialized {
        return agent_fail(&format!(
            "Bowser could not initialize its Windows UI thread: {error}"
        ));
    }
    let result = bowser::agent::serve(&dir, addr);
    unsafe {
        CoUninitialize();
    }
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => agent_fail(&error),
    }
}

const MAX_CRASH_LOG_BYTES: u64 = 256 * 1024;

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    // Release builds use `panic = "abort"`: the hook runs on the panicking thread, to completion,
    // and only then does the process abort. The log is written here, straight to the file, with
    // nothing relying on unwinding (a panic inside the hook aborts in either mode).
    std::panic::set_hook(Box::new(move |info| {
        append_crash_log(info);
        previous(info);
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
