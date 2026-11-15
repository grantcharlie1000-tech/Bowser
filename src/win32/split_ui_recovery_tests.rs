//! Real, never-shown native children; callbacks and cleanup use the production split host path.
use super::*;
use std::cell::Cell;
use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, GWL_STYLE, WS_POPUP, WS_VISIBLE};

struct Fixture { owner: HWND }
impl Fixture {
    fn new() -> Self {
        let owner = unsafe { CreateWindowExW(Default::default(), w!("STATIC"), w!(""),
            WS_POPUP, -32000, -32000, 800, 600, None, None, None, None) }.unwrap();
        Self { owner }
    }
    fn install(&self, shown: Shown) -> (HWND, Lease) {
        let container = unsafe { CreateWindowExW(Default::default(), w!("STATIC"), w!(""),
            WS_CHILD | WS_VISIBLE, 10, 60, 700, 500, Some(self.owner), None, None, None) }.unwrap();
        let rt = Runtime { container: Some(container), ready: true, requested: true, shown: Some(shown), ..Default::default() };
        let lease = rt.owner.begin();
        RUNTIMES.with(|all| { assert!(all.borrow_mut().insert(self.owner.0 as isize, rt).is_none()); });
        (container, lease)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        close(self.owner);
        unsafe { let _ = DestroyWindow(self.owner); }
    }
}

fn visible_style(hwnd: HWND) -> bool {
    unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & WS_VISIBLE.0 != 0 }
}

#[test]
fn failed_start_private_picker_and_drop_containers_are_hidden_then_destroyed() {
    let _profile = crate::profile::TestProfileGuard::isolated();
    let rect = RECT { left: 10, top: 60, right: 710, bottom: 560 };
    for shown in [Shown::Start { tab: TabId(0), rect }, Shown::Private { tab: TabId(0), rect },
        Shown::Picker { tab: TabId(0), pane: Pane::Left, side: DropZone::Left, rect },
        Shown::Drop { zone: DropZone::Right, rect }]
    {
        let f = Fixture::new();
        let (container, lease) = f.install(shown);
        let (command, failure, _) = callbacks(f.owner, &lease);
        assert!(visible_style(container));
        let app = crate::app::test_app();
        crate::app::test_show_page(&app, TabId(0), "https://example.test/recovered");
        { let _borrow = app.borrow_mut(); failure(); }
        assert!(!visible_style(container), "failure must hide the stale native child immediately");
        command(Command::Ready);
        assert!(!private_page_shown(f.owner));
        sync(f.owner, &app);
        assert!(!unsafe { IsWindow(Some(container)) }.as_bool(), "the not-ready path must still dispose the failed container");
    }
}

#[test]
fn late_ready_failure_accelerator_and_creation_error_cannot_touch_a_replacement() {
    let f = Fixture::new();
    let rect = RECT { left: 10, top: 60, right: 710, bottom: 560 };
    let (_, old) = f.install(Shown::Start { tab: TabId(0), rect });
    let (command, failure, accelerator) = callbacks(f.owner, &old);
    close(f.owner);
    let (replacement, current) = f.install(Shown::Private { tab: TabId(0), rect });
    with(f.owner, |rt| rt.ready = false);
    command(Command::Ready);
    failure();
    host_created(f.owner, &old, Err(windows::core::Error::from_hresult(windows::core::HRESULT(0x80004005u32 as i32))));
    assert!(!private_page_shown(f.owner));
    assert!(visible_style(replacement));
    assert!(!accelerator(Accel { vk: 0x73, ctrl: false, shift: false, alt: true,
        key_down: true, is_system: true, repeat: false }));
    callbacks(f.owner, &current).0(Command::Ready);
    assert!(private_page_shown(f.owner), "the replacement's own readiness must still work");
}

#[test]
fn queued_old_private_command_is_discarded_and_current_command_still_works() {
    let _profile = crate::profile::TestProfileGuard::isolated();
    let app = crate::app::test_app();
    let private = App::new_private_tab(&app);
    let f = Fixture::new();
    let shown = Shown::Private { tab: private, rect: RECT { left: 0, top: 0, right: 700, bottom: 500 } };
    let (_, old) = f.install(shown);
    let command = callbacks(f.owner, &old).0;
    command(Command::Split(SplitCommand::ClosePrivate));
    close(f.owner);
    let (_, current) = f.install(shown);
    command(Command::Split(SplitCommand::ClosePrivate));
    run_queued(f.owner, &app);
    assert_eq!(App::private_tab_ids(&app), vec![private]);
    callbacks(f.owner, &current).0(Command::Split(SplitCommand::ClosePrivate));
    run_queued(f.owner, &app);
    assert!(App::private_tab_ids(&app).is_empty());
}

#[test]
fn shared_loss_with_only_blank_tabs_disposes_start_container_through_the_window_hook() {
    let _profile = crate::profile::TestProfileGuard::isolated();
    let app = crate::app::test_app();
    App::new_tab(&app);
    assert_eq!(App::tab_count(&app), 2);
    assert!(App::tab_ids(&app).into_iter().all(|tab| App::tab_is_blank(&app, tab) && !App::tab_has_controller(&app, tab)));
    let f = Fixture::new();
    let (container, lease) = f.install(Shown::Start {
        tab: App::active_tab_id(&app), rect: RECT { left: 0, top: 60, right: 800, bottom: 600 },
    });
    let (command, failure, _) = callbacks(f.owner, &lease);
    // This is the parent integration boundary. No engine or WebView2 is created by this fixture.
    super::super::window::recover_native_chrome(f.owner, &app);
    command(Command::Ready);
    failure();
    assert!(!unsafe { IsWindow(Some(container)) }.as_bool());
    assert!(!private_page_shown(f.owner));
    assert_eq!(App::tab_count(&app), 2);
    assert!(App::tab_ids(&app).into_iter().all(|tab| App::content_failure_message(&app, tab).is_none()));
}

struct DestroyProbe { lease: Lease, observed: Rc<Cell<bool>>, command: Rc<dyn Fn(Command)> }

#[test]
fn page_drag_end_supersedes_an_older_tab_strip_move_in_the_shared_queue() {
    let _profile = crate::profile::TestProfileGuard::isolated();
    let app = crate::app::test_app();
    let f = Fixture::new();
    let (_, lease) = f.install(Shown::Start { tab: TabId(0), rect: RECT::default() });
    let id = wire_id(App::active_tab_id(&app));
    queue_from_tabs(f.owner, SplitCommand::DragOver { id, x: 40, y: 60 });
    callbacks(f.owner, &lease).0(Command::Split(SplitCommand::DragEnd { id }));
    run_queued(f.owner, &app);
    assert!(with(f.owner, |rt| rt.drag.is_none()).unwrap(),
        "an older move must not recreate drag state after the page ends the drag");
    queue_from_tabs(f.owner, SplitCommand::DragOver { id, x: 40, y: 60 });
    run_queued(f.owner, &app);
    assert!(with(f.owner, |rt| rt.drag.is_some()).unwrap(), "a fresh drag must still start");
}

unsafe extern "system" fn destroying(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, id: usize, data: usize) -> LRESULT {
    use windows::Win32::{UI::Shell::{DefSubclassProc, RemoveWindowSubclass}, UI::WindowsAndMessaging::WM_NCDESTROY};
    if msg == WM_NCDESTROY {
        let probe = unsafe { Box::from_raw(data as *mut DestroyProbe) };
        probe.observed.set(!probe.lease.current());
        (probe.command)(Command::Ready);
        unsafe { let _ = RemoveWindowSubclass(hwnd, Some(destroying), id); }
    }
    unsafe { DefSubclassProc(hwnd, msg, wp, lp) }
}

#[test]
fn native_child_destruction_reenters_only_after_its_owner_is_revoked() {
    let f = Fixture::new();
    let (container, lease) = f.install(Shown::Start { tab: TabId(0), rect: RECT::default() });
    let observed = Rc::new(Cell::new(false));
    let data = Box::into_raw(Box::new(DestroyProbe { command: callbacks(f.owner, &lease).0, lease, observed: observed.clone() }));
    assert!(unsafe { windows::Win32::UI::Shell::SetWindowSubclass(container, Some(destroying), 1, data as usize) }.as_bool());
    close(f.owner);
    assert!(observed.get());
    assert!(!unsafe { IsWindow(Some(container)) }.as_bool());
}
