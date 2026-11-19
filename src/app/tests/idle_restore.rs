//! Foreground memory priority changes only at a visibility boundary.
use super::*;

#[test]
fn a_loaded_window_can_remain_idle_without_trimming_or_polling() {
    let app = test_app();
    let calls = Rc::new(RefCell::new(Vec::new()));
    {
        let mut state = app.borrow_mut();
        state.page_loaded = true;
        state.engine = Some(recording_engine(calls.clone(), []));
        state.effective_memory_target = Some(MemoryUsageTarget::Normal);
    }
    // Background maintenance may still be dispatched, for example after a tab closes.
    for _ in 0..2 {
        assert_eq!(
            App::background_maintenance_fired(&app),
            IdleTimerAction::Kill
        );
        assert_eq!(
            app.borrow().desired_memory_target,
            MemoryUsageTarget::Normal
        );
    }
    assert!(calls.borrow().is_empty());
}

#[test]
fn minimizing_and_restoring_changes_priority_without_waiting_for_input() {
    let app = test_app();
    let calls = Rc::new(RefCell::new(Vec::new()));
    app.borrow_mut().engine = Some(recording_engine(calls.clone(), []));

    App::window_size_changed(&app, true, Default::default());
    assert_eq!(
        app.borrow().effective_memory_target,
        Some(MemoryUsageTarget::Low)
    );
    assert_eq!(
        App::background_maintenance_fired(&app),
        IdleTimerAction::Kill
    );
    App::window_size_changed(&app, false, rect(900, 650));
    assert_eq!(
        app.borrow().effective_memory_target,
        Some(MemoryUsageTarget::Normal)
    );
    assert_eq!(
        *calls.borrow(),
        ["target:Low", "resize:900x650", "target:Normal"]
    );
}

#[test]
fn reentrant_restore_reconciles_after_the_engine_can_be_called() {
    let app = test_app();
    let calls = Rc::new(RefCell::new(Vec::new()));
    app.borrow_mut().engine = Some(recording_engine(calls.clone(), []));
    App::window_size_changed(&app, true, Default::default());
    calls.borrow_mut().clear();

    App::call_engine(&app, |_engine| {
        App::window_size_changed(&app, false, rect(900, 650))
    });
    assert!(calls.borrow().is_empty());
    App::process_engine_notices(&app);
    assert_eq!(*calls.borrow(), ["resize:900x650", "target:Normal"]);
    assert_eq!(
        app.borrow().effective_memory_target,
        Some(MemoryUsageTarget::Normal)
    );
}
