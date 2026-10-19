//! What the main window's moves and resizes ask of the engine (src/win32/window.rs WM_MOVE and
//! WM_SIZE), driven with a scripted engine and no window.

use super::*;
use std::cell::Cell;

/// An engine whose resize fails and which counts parent-moved notices.
struct WindowEngine {
    moved: Rc<Cell<usize>>,
}

impl EngineAdapter for WindowEngine {
    fn navigate(&mut self, _url: &url::Url) -> windows::core::Result<()> {
        Ok(())
    }
    fn navigate_tab(&mut self, _tab: TabId, _url: &url::Url) -> windows::core::Result<()> {
        Ok(())
    }
    fn can_go_back(&self) -> bool {
        false
    }
    fn go_back(&mut self) -> windows::core::Result<()> {
        Ok(())
    }
    fn can_go_forward(&self) -> bool {
        false
    }
    fn go_forward(&mut self) -> windows::core::Result<()> {
        Ok(())
    }
    fn resize(&mut self, _bounds: windows::Win32::Foundation::RECT) -> windows::core::Result<()> {
        Err(windows::core::Error::from(windows::Win32::Foundation::E_FAIL))
    }
    fn parent_window_moved(&mut self) {
        self.moved.set(self.moved.get() + 1);
    }
    fn set_memory_usage_target(&mut self, _target: MemoryUsageTarget) -> windows::core::Result<MemoryTargetApply> {
        Ok(MemoryTargetApply::Applied)
    }
    fn close(&mut self) -> windows::core::Result<()> {
        Ok(())
    }
}

fn fixture() -> (AppRef, Rc<Cell<usize>>) {
    let moved = Rc::new(Cell::new(0));
    let app = app_with_pending(windows::Win32::Foundation::RECT { left: 0, top: 0, right: 800, bottom: 600 });
    app.borrow_mut().engine = Some(Box::new(WindowEngine { moved: moved.clone() }));
    (app, moved)
}

#[test]
fn a_failed_resize_is_reported_with_the_app_free_for_the_dialog_s_nested_resizes() {
    let (app, _) = fixture();
    let bounds = windows::Win32::Foundation::RECT { left: 0, top: 80, right: 800, bottom: 600 };
    let reported = Cell::new(false);
    App::resize_current_then(&app, bounds, |_| {
        // The error dialog's modal loop delivers WM_SIZE, whose handler borrows the app again.
        assert!(app.try_borrow_mut().is_ok(), "the app is still borrowed while the error is shown");
        App::resize_current_then(&app, bounds, |_| {});
        reported.set(true);
    });
    assert!(reported.get());
    assert!(app.borrow().engine.is_some(), "the engine is put back");
}

#[test]
fn a_window_move_tells_the_engine_unless_the_app_is_busy() {
    let (app, moved) = fixture();
    App::parent_window_moved(&app);
    assert_eq!(moved.get(), 1);
    {
        let _busy = app.borrow();
        App::parent_window_moved(&app);
    }
    assert_eq!(moved.get(), 1, "a move inside an app call is skipped instead of panicking");
    assert!(app.borrow().engine.is_some());
}
