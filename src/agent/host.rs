//! Hidden WebView2 host for `--agent`. The window thread is the STA thread and never blocks on a
//! socket. A background thread accepts loopback connections and posts the work here.

use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    io::Write,
    net::{SocketAddr, TcpListener},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Mutex, mpsc},
    time::{Duration, Instant},
};

use webview2_com::{
    CoTaskMemPWSTR, CoreWebView2EnvironmentOptions, CreateCoreWebView2ControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler, DownloadStartingEventHandler,
    ExecuteScriptCompletedHandler, ExecuteScriptWithResultCompletedHandler,
    NavigationCompletedEventHandler, NewWindowRequestedEventHandler,
    PermissionRequestedEventHandler, ScriptDialogOpeningEventHandler, SourceChangedEventHandler,
    Microsoft::Web::WebView2::Win32::*,
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, MSG,
            PostMessageW, PostQuitMessage, RegisterClassW, SetTimer, TranslateMessage, WM_APP,
            WM_TIMER, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
        },
    },
    core::{Interface, PCWSTR, PWSTR, w},
};

use super::{
    Command, Op, MAX_SESSIONS, action_response, back_script, bounded_response, check_session_name,
    click_script, decode_script_result, err_json, eval_script, fill_script, listening_line,
    navigate_script, ok_stop, ok_url, parse_command, peer_is_loopback, press_script, read_line,
    session_dir, shape_page_json, snapshot_script, text_response, text_script,
    webview_override_from_process,
};

const WM_AGENT: u32 = WM_APP + 0x41;
const JOB_BUDGET: Duration = Duration::from_secs(90);

struct Job {
    command: Command,
    reply: mpsc::Sender<String>,
    enqueued: Instant,
}

enum After {
    Snapshot,
    Text { markdown: bool },
    /// Click, fill, and press. Only `{ok:true}` from the page script succeeds.
    Action,
    /// Raw `script`. The decoded value is returned, including a real `null`.
    Script,
    /// `location.assign` / `history.back` have been asked. Finish when `Source` matches, not on
    /// the script callback: the page going away often makes that callback fail.
    Nav,
}

struct Pending {
    id: u64,
    session: String,
    reply: mpsc::Sender<String>,
    deadline: Instant,
    after: After,
}

struct Session {
    _environment: ICoreWebView2Environment,
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
}

struct Host {
    hwnd: HWND,
    root: PathBuf,
    sessions: HashMap<String, Session>,
    pending: Option<Pending>,
    creating: bool,
    create_generation: u64,
    creating_deadline: Option<Instant>,
    waiting: Option<Job>,
    pending_env: Option<ICoreWebView2Environment>,
    active: Option<String>,
    next_id: u64,
    stopping: bool,
    quit_at: Option<Instant>,
    /// URL `navigate` is waiting to land on. Empty while a `back` is in flight.
    nav_expect: Option<String>,
    /// Address before `history.back()`, so a stale navigation event is not treated as success.
    nav_before: Option<String>,
}

static QUEUE: Mutex<VecDeque<Job>> = Mutex::new(VecDeque::new());

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
}

pub fn serve(data_dir: &Path, addr: SocketAddr) -> Result<(), String> {
    if let Some(name) = webview_override_from_process() {
        return Err(format!(
            "Bowser cannot start while {name} is set. This variable can override the agent session profile or browser security settings. Remove it and try again."
        ));
    }
    if !addr.ip().is_loopback() {
        return Err(format!("Agent interface only binds to loopback, not {addr}"));
    }
    std::fs::create_dir_all(data_dir).map_err(|error| format!("Cannot create --data-dir: {error}"))?;
    let hwnd = create_hidden_window()?;
    let listener = TcpListener::bind(addr).map_err(|error| {
        format!("Cannot listen on {addr}: {error}. The agent interface stays on loopback.")
    })?;
    let bound = listener
        .local_addr()
        .map_err(|error| format!("Cannot read the agent address: {error}"))?;
    HOST.with(|slot| {
        *slot.borrow_mut() = Some(Host {
            hwnd,
            root: data_dir.to_path_buf(),
            sessions: HashMap::new(),
            pending: None,
            creating: false,
            create_generation: 0,
            creating_deadline: None,
            waiting: None,
            pending_env: None,
            active: None,
            next_id: 0,
            stopping: false,
            quit_at: None,
            nav_expect: None,
            nav_before: None,
        });
    });
    if unsafe { SetTimer(Some(hwnd), 1, 500, None) } == 0 {
        return Err("Cannot arm the agent timer".into());
    }
    let hwnd_bits = hwnd.0 as isize;
    std::thread::spawn(move || accept_loop(listener, hwnd_bits));
    let line = listening_line(bound);
    let _ = writeln!(std::io::stdout(), "{line}");
    let _ = std::io::stdout().flush();
    let mut message = MSG::default();
    loop {
        let got = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if got.0 <= 0 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        pump_jobs();
    }
    shutdown();
    Ok(())
}

fn create_hidden_window() -> Result<HWND, String> {
    let class_name: Vec<u16> = "BowserAgentHost\0".encode_utf16().collect();
    let instance = unsafe { GetModuleHandleW(None) }.map_err(|error| error.to_string())?;
    let class = WNDCLASSW {
        lpfnWndProc: Some(wnd_proc),
        hInstance: instance.into(),
        lpszClassName: PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };
    let atom = unsafe { RegisterClassW(&class) };
    if atom == 0 {
        return Err("Cannot register the agent window class".into());
    }
    unsafe {
        CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            PCWSTR(class_name.as_ptr()),
            w!("Bowser agent"),
            WS_POPUP,
            -32000,
            -32000,
            800,
            600,
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
    .map_err(|error| format!("Cannot create the agent window: {error}"))
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_TIMER {
        check_timeout();
        return LRESULT(0);
    }
    if message == WM_AGENT {
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn accept_loop(listener: TcpListener, hwnd_bits: isize) {
    let hwnd = HWND(hwnd_bits as *mut std::ffi::c_void);
    for incoming in listener.incoming() {
        let Ok(mut stream) = incoming else {
            continue;
        };
        let allowed = stream.peer_addr().ok().is_some_and(peer_is_loopback);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
        if !allowed {
            write_reply(&mut stream, &err_json("refused non-loopback peer"));
            continue;
        }
        let request = match read_line(&mut stream, 1_000_000) {
            Ok(line) if !line.trim().is_empty() => line,
            Ok(_) => {
                write_reply(&mut stream, &err_json("empty agent command"));
                continue;
            }
            Err(error) => {
                write_reply(&mut stream, &err_json(&error));
                continue;
            }
        };
        let command = match parse_command(&request) {
            Ok(command) => command,
            Err(error) => {
                write_reply(&mut stream, &err_json(&error));
                continue;
            }
        };
        let (tx, rx) = mpsc::channel();
        if let Ok(mut queue) = QUEUE.lock() {
            queue.push_back(Job {
                command,
                reply: tx,
                enqueued: Instant::now(),
            });
        }
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_AGENT, WPARAM(0), LPARAM(0));
        }
        match rx.recv_timeout(JOB_BUDGET + Duration::from_secs(5)) {
            Ok(body) => write_reply(&mut stream, &body),
            Err(_) => write_reply(&mut stream, &err_json("timed out")),
        }
    }
}

fn write_reply(stream: &mut impl Write, body: &str) {
    let body = bounded_response(body);
    let _ = writeln!(stream, "{body}");
}

fn pump_jobs() {
    check_timeout();
    let stopping = HOST.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|host| host.stopping)
    });
    if stopping {
        while let Some(job) = pop_job() {
            let _ = job.reply.send(err_json("agent is stopping"));
        }
        return;
    }
    let busy = HOST.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|host| host.pending.is_some() || host.creating)
    });
    if busy {
        return;
    }
    while let Some(job) = pop_job() {
        if job.enqueued.elapsed() > JOB_BUDGET {
            let _ = job.reply.send(err_json("timed out"));
            continue;
        }
        dispatch(job);
        break;
    }
}

fn pop_job() -> Option<Job> {
    QUEUE.lock().ok().and_then(|mut queue| queue.pop_front())
}

fn check_timeout() {
    let quit = HOST.with(|slot| {
        slot.borrow().as_ref().and_then(|host| host.quit_at).is_some_and(|at| Instant::now() >= at)
    });
    if quit {
        unsafe { PostQuitMessage(0) };
        return;
    }
    let expired_create = HOST.with(|slot| {
        slot.borrow().as_ref().is_some_and(|host| {
            host.creating
                && host
                    .creating_deadline
                    .is_some_and(|deadline| Instant::now() >= deadline)
        })
    });
    if expired_create {
        fail_create("timed out creating the session");
        return;
    }
    if let Some(url) = navigation_landed() {
        finish_current(ok_url(&url));
        return;
    }
    let expired = HOST.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|host| host.pending.as_ref())
            .map(|pending| (pending.id, Instant::now() >= pending.deadline))
    });
    if let Some((id, true)) = expired {
        finish_if(id, err_json("timed out"));
    }
}

fn navigation_landed() -> Option<String> {
    let (expect, before) = HOST.with(|slot| {
        let host = slot.borrow();
        let host = host.as_ref()?;
        if !matches!(host.pending.as_ref().map(|pending| &pending.after), Some(After::Nav)) {
            return None;
        }
        Some((host.nav_expect.clone(), host.nav_before.clone()))
    })?;
    let url = current_url();
    if let Some(expect) = expect {
        return same_document(&url, &expect).then_some(url);
    }
    let before = before?;
    if !url.is_empty() && !url.starts_with("about:") && url != before {
        Some(url)
    } else {
        None
    }
}

fn same_document(current: &str, expected: &str) -> bool {
    let norm = |value: &str| value.trim().trim_end_matches('/').to_ascii_lowercase();
    let current = norm(current);
    let expected = norm(expected);
    !expected.is_empty() && (current == expected || current.starts_with(&format!("{expected}#")))
}

fn dispatch(job: Job) {
    if matches!(job.command.op, Op::Stop) {
        request_stop(job.reply);
        return;
    }
    if let Err(error) = check_session_name(&job.command.session) {
        let _ = job.reply.send(err_json(&error));
        return;
    }
    let root = HOST.with(|slot| slot.borrow().as_ref().map(|host| host.root.clone()));
    let Some(root) = root else {
        let _ = job.reply.send(err_json("agent host is not running"));
        return;
    };
    let dir = match session_dir(&root, &job.command.session) {
        Ok(dir) => dir,
        Err(error) => {
            let _ = job.reply.send(err_json(&error));
            return;
        }
    };
    let exists = HOST.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|host| host.sessions.contains_key(&job.command.session))
    });
    if exists {
        run_command(job);
    } else {
        let full = HOST.with(|slot| {
            slot.borrow()
                .as_ref()
                .is_some_and(|host| host.sessions.len() >= MAX_SESSIONS)
        });
        if full {
            let _ = job.reply.send(err_json("too many agent sessions"));
            return;
        }
        begin_session(dir, job);
    }
}

fn request_stop(reply: mpsc::Sender<String>) {
    let _ = reply.send(ok_stop());
    HOST.with(|slot| {
        if let Some(host) = slot.borrow_mut().as_mut() {
            host.stopping = true;
            host.quit_at = Some(Instant::now() + Duration::from_millis(250));
        }
    });
}

fn begin_session(dir: PathBuf, job: Job) {
    if let Err(error) = std::fs::create_dir_all(&dir) {
        let _ = job.reply.send(err_json(&format!("Cannot create the session folder: {error}")));
        return;
    }
    let generation = HOST.with(|slot| {
        slot.borrow_mut().as_mut().map(|host| {
            host.creating = true;
            host.waiting = Some(job);
            host.create_generation = host.create_generation.wrapping_add(1);
            host.creating_deadline = Some(Instant::now() + JOB_BUDGET);
            host.create_generation
        })
    });
    let Some(generation) = generation else {
        return;
    };
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let options = CoreWebView2EnvironmentOptions::default();
    let options2: ICoreWebView2EnvironmentOptions2 = options.into();
    if let Err(error) = unsafe { options2.SetExclusiveUserDataFolderAccess(true) } {
        fail_create(&format!("Cannot configure the session profile: {error}"));
        return;
    }
    let Ok(options1) = options2.cast::<ICoreWebView2EnvironmentOptions>() else {
        fail_create("Cannot configure the session profile");
        return;
    };
    let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |status, environment| {
            on_environment(generation, status, environment);
            Ok(())
        },
    ));
    let requested = unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            PCWSTR(wide.as_ptr()),
            &options1,
            &handler,
        )
    };
    if let Err(error) = requested {
        fail_create(&format!("WebView2 did not start: {error}"));
    }
}

fn still_creating(generation: u64) -> bool {
    HOST.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|host| host.creating && host.create_generation == generation)
    })
}

fn on_environment(
    generation: u64,
    status: windows::core::Result<()>,
    environment: Option<ICoreWebView2Environment>,
) {
    if !still_creating(generation) {
        return;
    }
    let environment = match (status, environment) {
        (Ok(()), Some(environment)) => environment,
        (Err(error), _) => {
            fail_create(&format!("WebView2 did not start: {error}"));
            return;
        }
        _ => {
            fail_create("WebView2 did not start");
            return;
        }
    };
    let hwnd = HOST.with(|slot| slot.borrow().as_ref().map(|host| host.hwnd));
    let Some(hwnd) = hwnd else {
        fail_create("agent window is gone");
        return;
    };
    HOST.with(|slot| {
        if let Some(host) = slot.borrow_mut().as_mut() {
            host.pending_env = Some(environment.clone());
        }
    });
    let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
        move |status, controller| {
            on_controller(generation, status, controller);
            Ok(())
        },
    ));
    if let Err(error) = unsafe { environment.CreateCoreWebView2Controller(hwnd, &handler) } {
        fail_create(&format!("WebView2 did not create a page: {error}"));
    }
}

fn on_controller(
    generation: u64,
    status: windows::core::Result<()>,
    controller: Option<ICoreWebView2Controller>,
) {
    if !still_creating(generation) {
        if let Some(controller) = controller {
            let _ = unsafe { controller.Close() };
        }
        return;
    }
    let controller = match (status, controller) {
        (Ok(()), Some(controller)) => controller,
        (Err(error), _) => {
            fail_create(&format!("WebView2 did not create a page: {error}"));
            return;
        }
        _ => {
            fail_create("WebView2 did not create a page");
            return;
        }
    };
    let webview = match unsafe { controller.CoreWebView2() } {
        Ok(webview) => webview,
        Err(error) => {
            fail_create(&format!("WebView2 did not create a page: {error}"));
            return;
        }
    };
    if let Err(error) = configure(&controller, &webview) {
        fail_create(&error);
        return;
    }
    let (name, environment) = HOST.with(|slot| {
        let host = slot.borrow();
        let name = host
            .as_ref()
            .and_then(|host| host.waiting.as_ref())
            .map(|job| job.command.session.clone())
            .unwrap_or_default();
        let environment = host.as_ref().and_then(|host| host.pending_env.clone());
        (name, environment)
    });
    let Some(environment) = environment else {
        fail_create("WebView2 environment was dropped");
        return;
    };
    let mut token = 0;
    let nav_session = name.clone();
    let hooked = unsafe {
        webview.add_NavigationCompleted(
            &NavigationCompletedEventHandler::create(Box::new(move |_sender, _args| {
                on_navigation(&nav_session);
                Ok(())
            })),
            &mut token,
        )
    };
    if let Err(error) = hooked {
        fail_create(&format!("WebView2 did not report navigation: {error}"));
        return;
    }
    let source_session = name.clone();
    let sourced = unsafe {
        webview.add_SourceChanged(
            &SourceChangedEventHandler::create(Box::new(move |_sender, _args| {
                on_navigation(&source_session);
                Ok(())
            })),
            &mut token,
        )
    };
    if let Err(error) = sourced {
        fail_create(&format!("WebView2 did not report the address: {error}"));
        return;
    }
    HOST.with(|slot| {
        if let Some(host) = slot.borrow_mut().as_mut() {
            host.pending_env = None;
            host.sessions.insert(
                name,
                Session {
                    _environment: environment,
                    controller,
                    webview,
                },
            );
        }
    });
    let job = HOST.with(|slot| {
        slot.borrow_mut().as_mut().and_then(|host| {
            host.creating = false;
            host.creating_deadline = None;
            host.waiting.take()
        })
    });
    if let Some(job) = job {
        run_command(job);
    }
}

fn configure(controller: &ICoreWebView2Controller, webview: &ICoreWebView2) -> Result<(), String> {
    unsafe {
        controller
            .SetBounds(RECT {
                left: 0,
                top: 0,
                right: 800,
                bottom: 600,
            })
            .map_err(|error| error.to_string())?;
        controller.SetIsVisible(true).map_err(|error| error.to_string())?;
        let settings = webview.Settings().map_err(|error| error.to_string())?;
        settings
            .SetIsWebMessageEnabled(false)
            .map_err(|error| error.to_string())?;
        settings
            .SetAreHostObjectsAllowed(false)
            .map_err(|error| error.to_string())?;
        settings
            .SetAreDefaultContextMenusEnabled(false)
            .map_err(|error| error.to_string())?;
        settings
            .SetIsStatusBarEnabled(false)
            .map_err(|error| error.to_string())?;
        settings
            .SetAreDevToolsEnabled(false)
            .map_err(|error| error.to_string())?;
        settings
            .SetAreDefaultScriptDialogsEnabled(false)
            .map_err(|error| error.to_string())?;
        let mut token = 0;
        #[cfg(test)] let window_probe=crate::extension_window_probe::Observer::prepare("isolated-agent-host",webview);
        #[cfg(test)] let request_probe=window_probe.clone();
        webview
            .add_NewWindowRequested(
                &NewWindowRequestedEventHandler::create(Box::new(move |_sender, args| {
                    if let Some(args) = args {
                    #[cfg(test)] let request_id=request_probe.enter(_sender.as_ref(),&args);
                        let _ = args.SetHandled(true);
                    #[cfg(test)] request_probe.finish(request_id,&args,"existing-policy-unchanged",false);
                    }
                    Ok(())
                })),
                &mut token,
            )
            .map_err(|error| error.to_string())?;
        #[cfg(test)] window_probe.installed();
        webview
            .add_PermissionRequested(
                &PermissionRequestedEventHandler::create(Box::new(|_sender, args| {
                    if let Some(args) = args {
                        let _ = args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY);
                    }
                    Ok(())
                })),
                &mut token,
            )
            .map_err(|error| error.to_string())?;
        webview
            .add_ScriptDialogOpening(
                &ScriptDialogOpeningEventHandler::create(Box::new(|_sender, args| {
                    if let Some(args) = args {
                        let mut kind = COREWEBVIEW2_SCRIPT_DIALOG_KIND::default();
                        if args.Kind(&mut kind).is_ok()
                            && kind == COREWEBVIEW2_SCRIPT_DIALOG_KIND_BEFOREUNLOAD
                        {
                            let _ = args.Accept();
                        }
                    }
                    Ok(())
                })),
                &mut token,
            )
            .map_err(|error| error.to_string())?;
        let webview4: ICoreWebView2_4 = webview.cast().map_err(|error| error.to_string())?;
        webview4
            .add_DownloadStarting(
                &DownloadStartingEventHandler::create(Box::new(|_sender, args| {
                    if let Some(args) = args {
                        let _ = args.SetCancel(true);
                        let _ = args.SetHandled(true);
                    }
                    Ok(())
                })),
                &mut token,
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn fail_create(message: &str) {
    let job = HOST.with(|slot| {
        slot.borrow_mut().as_mut().and_then(|host| {
            host.creating = false;
            host.creating_deadline = None;
            host.pending_env = None;
            host.waiting.take()
        })
    });
    if let Some(job) = job {
        let _ = job.reply.send(err_json(message));
    }
}

fn run_command(job: Job) {
    let Job {
        command,
        reply,
        enqueued: _,
    } = job;
    let session_name = command.session.clone();
    HOST.with(|slot| {
        if let Some(host) = slot.borrow_mut().as_mut() {
            host.active = Some(session_name.clone());
        }
    });
    let webview = HOST.with(|slot| {
        slot.borrow().as_ref().and_then(|host| {
            host.sessions
                .get(&session_name)
                .map(|session| session.webview.clone())
        })
    });
    let Some(webview) = webview else {
        let _ = reply.send(err_json("session is not open"));
        return;
    };
    match command.op {
        Op::Stop => request_stop(reply),
        Op::Back => {
            let mut can = windows::core::BOOL::default();
            if unsafe { webview.CanGoBack(&mut can) }.is_err() || !can.as_bool() {
                let _ = reply.send(err_json("no previous page"));
                return;
            }
            let before = read_source(&webview);
            HOST.with(|slot| {
                if let Some(host) = slot.borrow_mut().as_mut() {
                    host.nav_expect = None;
                    host.nav_before = Some(before);
                }
            });
            let id = arm(reply, session_name, After::Nav);
            execute(&webview, back_script(), true, id);
        }
        Op::Navigate(url) => {
            HOST.with(|slot| {
                if let Some(host) = slot.borrow_mut().as_mut() {
                    host.nav_expect = Some(url.clone());
                    host.nav_before = None;
                }
            });
            let id = arm(reply, session_name, After::Nav);
            execute(&webview, &navigate_script(&url), true, id);
        }
        Op::Snapshot => {
            let id = arm(reply, session_name, After::Snapshot);
            execute(&webview, snapshot_script(), false, id);
        }
        Op::Text { markdown } => {
            let id = arm(reply, session_name, After::Text { markdown });
            execute(&webview, text_script(), false, id);
        }
        Op::Click { refer, selector } => {
            let id = arm(reply, session_name, After::Action);
            execute(
                &webview,
                &click_script(refer.as_deref(), selector.as_deref()),
                false,
                id,
            );
        }
        Op::Fill { refer, text } => {
            let id = arm(reply, session_name, After::Action);
            execute(&webview, &fill_script(&refer, &text), false, id);
        }
        Op::Press { key } => {
            let id = arm(reply, session_name, After::Action);
            execute(&webview, &press_script(&key), false, id);
        }
        Op::Script(expression) => {
            let id = arm(reply, session_name, After::Script);
            execute(&webview, &eval_script(&expression), false, id);
        }
    }
}

fn arm(reply: mpsc::Sender<String>, session: String, after: After) -> u64 {
    HOST.with(|slot| {
        let mut host_slot = slot.borrow_mut();
        let Some(host) = host_slot.as_mut() else {
            return 0;
        };
        host.next_id = host.next_id.wrapping_add(1);
        let id = host.next_id;
        host.pending = Some(Pending {
            id,
            session,
            reply,
            deadline: Instant::now() + JOB_BUDGET,
            after,
        });
        id
    })
}

type ScriptCompletedCallback = Box<dyn FnOnce(Result<String, String>)>;

fn execute(webview: &ICoreWebView2, script: &str, navigation: bool, id: u64) {
    let slot: Rc<RefCell<Option<ScriptCompletedCallback>>> =
        Rc::new(RefCell::new(Some(Box::new(move |result| {
            on_script(id, result, navigation);
        }))));
    let wide = CoTaskMemPWSTR::from(script);
    if let Ok(webview21) = webview.cast::<ICoreWebView2_21>() {
        let completion = slot.clone();
        let handler = ExecuteScriptWithResultCompletedHandler::create(Box::new(
            move |status, result| {
                let parsed = script_with_result(status, result);
                if let Some(done) = completion.borrow_mut().take() {
                    done(parsed);
                }
                Ok(())
            },
        ));
        let called = unsafe {
            webview21.ExecuteScriptWithResult(*wide.as_ref().as_pcwstr(), &handler)
        };
        if called.is_ok() {
            return;
        }
        if let Some(done) = slot.borrow_mut().take() {
            done(Err("ExecuteScriptWithResult was rejected".into()));
        }
        return;
    }
    let completion = slot.clone();
    let handler = ExecuteScriptCompletedHandler::create(Box::new(move |status, value| {
        let result = if status.is_ok() {
            Ok(value.to_string())
        } else {
            Err(format!("ExecuteScript failed: {status:?}"))
        };
        if let Some(done) = completion.borrow_mut().take() {
            done(result);
        }
        Ok(())
    }));
    let called = unsafe { webview.ExecuteScript(*wide.as_ref().as_pcwstr(), &handler) };
    if called.is_err()
        && let Some(done) = slot.borrow_mut().take()
    {
        done(Err("ExecuteScript was rejected".into()));
    }
}

fn script_with_result(
    status: windows::core::Result<()>,
    result: Option<ICoreWebView2ExecuteScriptResult>,
) -> Result<String, String> {
    if let Err(error) = status {
        return Err(format!("ExecuteScriptWithResult failed: {error}"));
    }
    let Some(result) = result else {
        return Err("script returned no result".into());
    };
    let mut succeeded = windows::core::BOOL::default();
    if unsafe { result.Succeeded(&mut succeeded) }.is_err() {
        return Err("script result was unreadable".into());
    }
    if !succeeded.as_bool() {
        let message = unsafe { result.Exception() }
            .ok()
            .and_then(|exception| {
                let mut message = PWSTR::null();
                unsafe { exception.Message(&mut message) }.ok()?;
                let text = CoTaskMemPWSTR::from(message).to_string();
                let text = text.chars().take(200).collect::<String>();
                if text.is_empty() { None } else { Some(text) }
            })
            .unwrap_or_else(|| "script threw".to_owned());
        return Err(message);
    }
    let mut json = PWSTR::null();
    if unsafe { result.ResultAsJson(&mut json) }.is_err() {
        return Err("script result was unreadable".into());
    }
    Ok(CoTaskMemPWSTR::from(json).to_string())
}

fn on_script(id: u64, result: Result<String, String>, navigation: bool) {
    if navigation {
        if let Err(error) = result {
            if error.contains("was rejected") || error.contains("failed:") {
                finish_if(id, err_json(&error));
            }
        }
        return;
    }
    let kind = HOST.with(|slot| {
        slot.borrow().as_ref().and_then(|host| {
            let pending = host.pending.as_ref()?;
            if pending.id != id {
                return None;
            }
            Some(match pending.after {
                After::Snapshot => 0,
                After::Text { markdown: false } => 1,
                After::Text { markdown: true } => 2,
                After::Action => 3,
                After::Script => 4,
                After::Nav => 5,
            })
        })
    });
    let Some(kind) = kind else {
        return;
    };
    let body = match result {
        Err(error) => err_json(&error),
        Ok(raw) => match decode_script_result(&raw) {
            Err(error) => err_json(&error),
            Ok(decoded) => match kind {
                0 => shape_page_json(&decoded).unwrap_or_else(|error| err_json(&error)),
                1 => text_response(&decoded, false).unwrap_or_else(|error| err_json(&error)),
                2 => text_response(&decoded, true).unwrap_or_else(|error| err_json(&error)),
                3 => action_response(&decoded, true),
                4 => action_response(&decoded, false),
                _ => err_json("no agent command was waiting"),
            },
        },
    };
    finish_if(id, body);
}

fn on_navigation(session: &str) {
    let id = HOST.with(|slot| {
        slot.borrow().as_ref().and_then(|host| {
            host.pending.as_ref().and_then(|pending| {
                (pending.session == session && matches!(pending.after, After::Nav)).then_some(pending.id)
            })
        })
    });
    let Some(id) = id else {
        return;
    };
    if let Some(url) = navigation_landed() {
        finish_if(id, ok_url(&url));
    }
}

fn current_url() -> String {
    let webview = HOST.with(|slot| {
        let host = slot.borrow();
        let host = host.as_ref()?;
        let name = host
            .pending
            .as_ref()
            .map(|pending| pending.session.clone())
            .or_else(|| host.active.clone())?;
        host.sessions.get(&name).map(|session| session.webview.clone())
    });
    webview.as_ref().map(read_source).unwrap_or_default()
}

fn read_source(webview: &ICoreWebView2) -> String {
    let mut source = PWSTR::null();
    if unsafe { webview.Source(&mut source) }.is_err() {
        return String::new();
    }
    CoTaskMemPWSTR::from(source).to_string()
}

fn finish_current(body: String) {
    let id = HOST.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|host| host.pending.as_ref().map(|pending| pending.id))
    });
    if let Some(id) = id {
        finish_if(id, body);
    }
}

fn finish_if(id: u64, body: String) {
    let reply = HOST.with(|slot| {
        let mut host_slot = slot.borrow_mut();
        let host = host_slot.as_mut()?;
        let matches = host.pending.as_ref().is_some_and(|pending| pending.id == id);
        if !matches {
            return None;
        }
        host.nav_expect = None;
        host.nav_before = None;
        host.pending.take().map(|pending| pending.reply)
    });
    if let Some(reply) = reply {
        let _ = reply.send(body);
    }
}

fn shutdown() {
    let host = HOST.with(|slot| slot.borrow_mut().take());
    if let Some(host) = host {
        for (_, session) in host.sessions {
            let _ = unsafe { session.controller.Close() };
        }
        unsafe {
            let _ = DestroyWindow(host.hwnd);
        }
    }
}
