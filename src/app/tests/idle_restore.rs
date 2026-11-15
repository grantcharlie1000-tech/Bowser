//! Idle trim must hand a Low page back to Normal promptly once the user types or scrolls again.
use super::*;

#[test]
fn low_polls_cheaply_and_normal_does_not() {
    let trimmed = idle_memory_decision(false, true, 3_000, MemoryUsageTarget::Normal);
    assert_eq!(trimmed.target, MemoryUsageTarget::Low);
    assert_eq!(trimmed.timer, IdleTimerAction::Arm(IDLE_RESTORE_POLL_MS));
    assert!(IDLE_RESTORE_POLL_MS <= 250);

    let still_idle = idle_memory_decision(false, true, 9_000, MemoryUsageTarget::Low);
    assert_eq!(still_idle.target, MemoryUsageTarget::Low);
    assert_eq!(still_idle.timer, IdleTimerAction::Arm(IDLE_RESTORE_POLL_MS));

    // Normal never polls at the restore cadence.
    let counting = idle_memory_decision(false, true, 1_000, MemoryUsageTarget::Normal);
    assert_eq!(counting.timer, IdleTimerAction::Arm(1_000));
}

#[test]
fn newer_input_restores_normal_and_rearms_the_idle_timer() {
    let restored = idle_memory_decision(false, true, 120, MemoryUsageTarget::Low);
    assert_eq!(
        restored,
        IdleMemoryDecision { target: MemoryUsageTarget::Normal, timer: IdleTimerAction::Arm(3_000) }
    );
}

#[test]
fn timer_handler_transitions_low_then_normal_on_input() {
    let app = test_app();
    {
        let mut state = app.borrow_mut();
        state.page_loaded = true;
        state.minimized = false;
        state.desired_memory_target = MemoryUsageTarget::Normal;
    }
    assert_eq!(App::idle_timer_fired(&app, 3_500, true), IdleTimerAction::Arm(IDLE_RESTORE_POLL_MS));
    assert_eq!(app.borrow().desired_memory_target, MemoryUsageTarget::Low);
    // Still idle: stays Low and keeps polling.
    assert_eq!(App::idle_timer_fired(&app, 3_750, true), IdleTimerAction::Arm(IDLE_RESTORE_POLL_MS));
    assert_eq!(app.borrow().desired_memory_target, MemoryUsageTarget::Low);
    // Input arrived: Normal, idle timer re-armed, poll stops being the cadence.
    assert_eq!(App::idle_timer_fired(&app, 80, true), IdleTimerAction::Arm(3_000));
    assert_eq!(app.borrow().desired_memory_target, MemoryUsageTarget::Normal);
}

#[test]
fn background_minimized_or_unloaded_pages_never_poll() {
    let app = test_app();
    {
        let mut state = app.borrow_mut();
        state.page_loaded = true;
        state.desired_memory_target = MemoryUsageTarget::Low;
    }
    // Another application is foreground: input says nothing about this window.
    assert_eq!(App::idle_timer_fired(&app, 0, false), IdleTimerAction::Kill);
    assert_eq!(app.borrow().desired_memory_target, MemoryUsageTarget::Low);
    app.borrow_mut().minimized = true;
    assert_eq!(App::idle_timer_fired(&app, 0, true), IdleTimerAction::Kill);
    {
        let mut state = app.borrow_mut();
        state.minimized = false;
        state.page_loaded = false; // closed or stale page: nothing to restore
    }
    assert_eq!(App::idle_timer_fired(&app, 100, true), IdleTimerAction::Kill);
    assert_eq!(app.borrow().desired_memory_target, MemoryUsageTarget::Normal);
}
