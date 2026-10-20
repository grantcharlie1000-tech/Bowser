//! Fixed test-only dispatch telemetry. It is never a readiness or dispatch authority.
//! Each pump owns its record; TLS only lends that record to a lexical native dispatch.
use std::{cell::Cell, rc::{Rc, Weak}};

const MAX_HANDLER_DEPTH: u8 = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MessageKind { NotDispatched, NonTimer, TimerProc, ThreadTimer, WindowTimer }
impl MessageKind {
    pub(crate) fn classify(is_timer: bool, has_timerproc: bool, has_window: bool) -> Self {
        if !is_timer { Self::NonTimer }
        else if has_timerproc { Self::TimerProc }
        else if !has_window { Self::ThreadTimer }
        else { Self::WindowTimer }
    }
    fn label(self) -> &'static str {
        match self {
            Self::NotDispatched => "not-dispatched", Self::NonTimer => "non-timer",
            Self::TimerProc => "timerproc", Self::ThreadTimer => "thread-timer",
            Self::WindowTimer => "unclassified-window-timer",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Handler {
    Unobserved, Unavailable, ImportOffer, ProbeWatchdog, ContextMenu, PopupPrewarm,
    TabLink, Thumbnail, TabTick, ThemeSchedule, AutoHide, Downloads, Power, IdleTrim,
    SidebarResize, PageIdle, SidebarScroll, IslandHover, CardHover, SessionSave,
    PrivacyStats, BlockedTitle, Animation, SidePanel, PanelReload, PanelIdle,
    TranslateWatch, ReaderProbe, DevtoolsDock, Progress, SplitBounds, CaretBlink,
    ClipboardClear, RailTooltip, NewtabTooltip, MainOtherTimer,
}
impl Handler {
    fn label(self) -> &'static str {
        match self {
            Self::Unobserved => "unobserved", Self::Unavailable => "unavailable",
            Self::ImportOffer => "import-offer", Self::ProbeWatchdog => "probe-watchdog",
            Self::ContextMenu => "context-menu", Self::PopupPrewarm => "popup-prewarm",
            Self::TabLink => "tab-link", Self::Thumbnail => "thumbnail", Self::TabTick => "tab-tick",
            Self::ThemeSchedule => "theme-schedule", Self::AutoHide => "auto-hide",
            Self::Downloads => "downloads", Self::Power => "power", Self::IdleTrim => "idle-trim",
            Self::SidebarResize => "sidebar-resize", Self::PageIdle => "page-idle",
            Self::SidebarScroll => "sidebar-scroll", Self::IslandHover => "island-hover",
            Self::CardHover => "card-hover", Self::SessionSave => "session-save",
            Self::PrivacyStats => "privacy-stats", Self::BlockedTitle => "blocked-title",
            Self::Animation => "animation", Self::SidePanel => "side-panel",
            Self::PanelReload => "panel-reload", Self::PanelIdle => "panel-idle",
            Self::TranslateWatch => "translate-watch", Self::ReaderProbe => "reader-probe",
            Self::DevtoolsDock => "devtools-dock", Self::Progress => "progress",
            Self::SplitBounds => "split-bounds", Self::CaretBlink => "caret-blink",
            Self::ClipboardClear => "clipboard-clear", Self::RailTooltip => "rail-tooltip",
            Self::NewtabTooltip => "newtab-tooltip", Self::MainOtherTimer => "main-other-timer",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowKind { Unobserved, MainWindowProcedure, Unavailable }
impl WindowKind {
    fn label(self) -> &'static str {
        match self {
            Self::Unobserved => "unobserved", Self::MainWindowProcedure => "main-window-procedure",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Clone, Copy)]
struct Snapshot {
    kind: MessageKind, window: WindowKind, handler: Handler,
    active: bool, entered: bool, exited: bool, depth: u8, unavailable: bool,
}
impl Snapshot {
    fn new(kind: MessageKind) -> Self {
        Self { kind, window: WindowKind::Unobserved, handler: Handler::Unobserved,
            active: false, entered: false, exited: false, depth: 0, unavailable: false }
    }
}

pub(crate) struct DispatchRecord { snapshot: Cell<Snapshot> }
impl DispatchRecord {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self { snapshot: Cell::new(Snapshot::new(MessageKind::NotDispatched)) })
    }
    fn unavailable(&self) {
        let mut value = self.snapshot.get();
        value.unavailable = true; value.window = WindowKind::Unavailable; value.handler = Handler::Unavailable;
        self.snapshot.set(value);
    }
    pub(crate) fn diagnostic_json(&self) -> String {
        let value = self.snapshot.get();
        format!(r#"{{"kind":"{}","window":"{}","handler":"{}","entered":{},"exited":{},"depth":{},"unavailable":{}}}"#,
            value.kind.label(), value.window.label(), value.handler.label(),
            value.entered, value.exited, value.depth, value.unavailable)
    }
}

/// Equality-only, already-dequeued identity. This is deliberately not Debug/Display or
/// part of DispatchRecord. It dies with its lexical binding and is never output.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct DispatchIdentity { window: usize, timer: usize, callback: isize }
impl DispatchIdentity {
    pub(crate) fn new(window: usize, timer: usize, callback: isize) -> Self {
        Self { window, timer, callback }
    }
}
struct DispatchBinding { record: Rc<DispatchRecord>, identity: DispatchIdentity }
thread_local! { static ACTIVE: Cell<Option<Rc<DispatchBinding>>> = const { Cell::new(None) }; }

fn current() -> Option<Rc<DispatchBinding>> {
    ACTIVE.try_with(|slot| {
        let value = slot.take(); let copy = value.clone(); slot.set(value); copy
    }).ok().flatten()
}
fn is_current(binding: &Rc<DispatchBinding>) -> bool {
    current().is_some_and(|active| Rc::ptr_eq(&active, binding))
}

/// No TLS/RefCell/App guard spans DispatchMessageW. An unobserved nested pump masks
/// its parent, so it cannot lend another call's evidence a new handler label.
pub(crate) struct DispatchScope {
    previous: Option<Weak<DispatchBinding>>, bound: Option<Rc<DispatchBinding>>,
}
impl DispatchScope {
    pub(crate) fn enter(record: Option<&Rc<DispatchRecord>>, kind: MessageKind, identity: DispatchIdentity) -> Self {
        let bound = record.and_then(|record| {
            if record.snapshot.get().active { record.unavailable(); None }
            else {
                let mut value = Snapshot::new(kind); value.active = true; record.snapshot.set(value);
                Some(Rc::new(DispatchBinding { record: record.clone(), identity }))
            }
        });
        match ACTIVE.try_with(|slot| slot.replace(bound.clone())) {
            Ok(previous) => Self { previous: previous.as_ref().map(Rc::downgrade), bound },
            Err(_) => {
                if let Some(binding) = &bound { binding.record.unavailable(); }
                Self { previous: None, bound }
            }
        }
    }
}
impl Drop for DispatchScope {
    fn drop(&mut self) {
        let active = current();
        let matches = match (&self.bound, &active) {
            (Some(bound), Some(active)) => Rc::ptr_eq(bound, active),
            (None, None) => true,
            _ => false,
        };
        if !matches {
            if let Some(active) = &active { active.record.unavailable(); }
            if let Some(bound) = &self.bound { bound.record.unavailable(); }
        }
        if let Some(binding) = &self.bound {
            let record = &binding.record;
            if record.snapshot.get().depth != 0 { record.unavailable(); }
            let mut value = record.snapshot.get(); value.active = false; record.snapshot.set(value);
        }
        let _ = ACTIVE.try_with(|slot| slot.set(self.previous.take().and_then(|previous| previous.upgrade())));
    }
}

// The handler may be retained by a synthetic mismatch test. It must not keep the
// equality-only identity alive after the owning DispatchScope/TLS binding ends.
pub(crate) struct HandlerScope {
    record: Option<Rc<DispatchRecord>>, binding: Weak<DispatchBinding>, previous_depth: u8,
}
impl HandlerScope {
    fn inactive() -> Self { Self { record: None, binding: Weak::new(), previous_depth: 0 } }
}
pub(crate) fn enter_main_timer(identity: DispatchIdentity, handler: Handler) -> HandlerScope {
    let Some(binding) = current() else { return HandlerScope::inactive(); };
    let record = &binding.record;
    let mut value = record.snapshot.get();
    if value.unavailable { return HandlerScope::inactive(); }
    if !value.active || value.kind != MessageKind::WindowTimer || value.depth >= MAX_HANDLER_DEPTH
        || (value.depth == 0 && (value.entered || binding.identity != identity)) {
        record.unavailable(); return HandlerScope::inactive();
    }
    let previous_depth = value.depth;
    if previous_depth == 0 && !value.unavailable {
        value.window = WindowKind::MainWindowProcedure; value.handler = handler; value.entered = true;
    }
    value.depth += 1; record.snapshot.set(value);
    HandlerScope { record: Some(record.clone()), binding: Rc::downgrade(&binding), previous_depth }
}
impl Drop for HandlerScope {
    fn drop(&mut self) {
        let Some(record) = &self.record else { return; };
        let current_binding = self.binding.upgrade().is_some_and(|binding| is_current(&binding));
        if !current_binding || !record.snapshot.get().active
            || record.snapshot.get().depth != self.previous_depth + 1 { record.unavailable(); }
        let mut value = record.snapshot.get(); value.depth = self.previous_depth;
        if self.previous_depth == 0 { value.exited = true; }
        record.snapshot.set(value);
    }
}

#[cfg(test)]
fn identity() -> DispatchIdentity { DispatchIdentity::new(17, 23, 0) }

#[test]
fn synthetic_dispatch_trace_nested_watchdog_preserves_outer_handler_and_exit() {
    let record = DispatchRecord::new();
    let dispatch = DispatchScope::enter(Some(&record), MessageKind::WindowTimer, identity());
    let outer = enter_main_timer(identity(), Handler::ImportOffer);
    assert_eq!(record.snapshot.get().handler, Handler::ImportOffer);
    assert_eq!(record.snapshot.get().window, WindowKind::MainWindowProcedure);
    { let _watchdog = enter_main_timer(DispatchIdentity::new(17, 999, 0), Handler::ProbeWatchdog);
        assert_eq!(record.snapshot.get().handler, Handler::ImportOffer);
        assert_eq!(record.snapshot.get().depth, 2);
        assert!(!record.snapshot.get().exited); }
    assert_eq!(record.snapshot.get().depth, 1);
    drop(outer); drop(dispatch);
    assert_eq!(record.snapshot.get().handler, Handler::ImportOffer);
    assert_eq!(record.snapshot.get().depth, 0);
    assert!(record.snapshot.get().exited);
    assert!(!record.snapshot.get().unavailable);
}

#[test]
fn synthetic_dispatch_trace_independent_pumps_and_unobserved_calls_are_isolated() {
    let first = DispatchRecord::new(); let second = DispatchRecord::new();
    let first_dispatch = DispatchScope::enter(Some(&first), MessageKind::WindowTimer, identity());
    let first_handler = enter_main_timer(identity(), Handler::ImportOffer);
    { let _dispatch = DispatchScope::enter(Some(&second), MessageKind::WindowTimer, identity());
        let _handler = enter_main_timer(identity(), Handler::ProbeWatchdog);
        assert_eq!(second.snapshot.get().handler, Handler::ProbeWatchdog);
        assert_eq!(first.snapshot.get().handler, Handler::ImportOffer); }
    let second_saved = second.diagnostic_json();
    { let _unobserved = DispatchScope::enter(None, MessageKind::WindowTimer, identity());
        let _handler = enter_main_timer(identity(), Handler::ContextMenu);
        assert_eq!(first.snapshot.get().depth, 1); }
    drop(first_handler); drop(first_dispatch);
    { let _later = DispatchScope::enter(Some(&first), MessageKind::WindowTimer, identity());
        let _handler = enter_main_timer(identity(), Handler::TabLink); }
    assert_eq!(first.snapshot.get().handler, Handler::TabLink);
    assert_eq!(second.diagnostic_json(), second_saved);
    assert!(current().is_none());
}

#[test]
fn synthetic_dispatch_trace_raii_unwind_restores_previous_binding() {
    let first = DispatchRecord::new(); let second = DispatchRecord::new();
    let dispatch = DispatchScope::enter(Some(&first), MessageKind::WindowTimer, identity());
    let first_binding = Rc::downgrade(&current().unwrap());
    let handler = enter_main_timer(identity(), Handler::ImportOffer);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _dispatch = DispatchScope::enter(Some(&second), MessageKind::WindowTimer, identity());
        let _handler = enter_main_timer(identity(), Handler::Thumbnail);
        panic!("synthetic unwind, no native call");
    }));
    assert!(result.is_err()); assert!(is_current(&first_binding.upgrade().unwrap()));
    assert_eq!(second.snapshot.get().depth, 0); assert!(second.snapshot.get().exited);
    assert_eq!(first.snapshot.get().handler, Handler::ImportOffer);
    drop(handler); drop(dispatch); assert!(current().is_none());
}

#[test]
fn synthetic_dispatch_trace_message_classes_and_unknown_main_timer_stay_explicit() {
    let classes = [(false, false, false, MessageKind::NonTimer),
        (false, true, true, MessageKind::NonTimer), (true, true, true, MessageKind::TimerProc),
        (true, true, false, MessageKind::TimerProc), (true, false, false, MessageKind::ThreadTimer),
        (true, false, true, MessageKind::WindowTimer)];
    for (timer, proc, window, expected) in classes {
        let record = DispatchRecord::new(); let kind = MessageKind::classify(timer, proc, window);
        assert_eq!(kind, expected);
        { let _dispatch = DispatchScope::enter(Some(&record), kind, identity()); }
        assert_eq!(record.snapshot.get().kind, expected);
        assert_eq!(record.snapshot.get().handler, Handler::Unobserved);
    }
    let record = DispatchRecord::new();
    { let _dispatch = DispatchScope::enter(Some(&record), MessageKind::WindowTimer, identity());
        let _handler = enter_main_timer(identity(), Handler::MainOtherTimer); }
    assert_eq!(record.snapshot.get().handler, Handler::MainOtherTimer);
    assert!(record.snapshot.get().exited);
}

#[test]
fn synthetic_dispatch_trace_depth_cap_and_nested_same_record_are_unavailable() {
    let record = DispatchRecord::new();
    { let _dispatch = DispatchScope::enter(Some(&record), MessageKind::WindowTimer, identity());
        let guards: Vec<_> = (0..=MAX_HANDLER_DEPTH).map(|_| enter_main_timer(identity(), Handler::ImportOffer)).collect();
        assert!(record.snapshot.get().unavailable); assert_eq!(record.snapshot.get().depth, MAX_HANDLER_DEPTH);
        for guard in guards.into_iter().rev() { drop(guard); }
        assert_eq!(record.snapshot.get().depth, 0); assert!(record.snapshot.get().exited); }
    { let _outer = DispatchScope::enter(Some(&record), MessageKind::WindowTimer, identity());
        let _handler = enter_main_timer(identity(), Handler::ImportOffer);
        { let _nested = DispatchScope::enter(Some(&record), MessageKind::WindowTimer, identity());
            let _watchdog = enter_main_timer(DispatchIdentity::new(17, 999, 0), Handler::ProbeWatchdog); }
        assert!(record.snapshot.get().unavailable);
        assert_eq!(record.snapshot.get().handler, Handler::Unavailable);
        assert_eq!(record.snapshot.get().depth, 1); }
    assert_eq!(record.snapshot.get().depth, 0); assert!(record.snapshot.get().exited);
}

#[test]
fn synthetic_dispatch_trace_mismatched_scope_is_unavailable_and_output_is_bounded() {
    let first = DispatchRecord::new(); let second = DispatchRecord::new();
    let dispatch = DispatchScope::enter(Some(&first), MessageKind::WindowTimer, identity());
    let handler = enter_main_timer(identity(), Handler::ImportOffer);
    let nested = DispatchScope::enter(Some(&second), MessageKind::NonTimer, identity());
    drop(handler);
    assert!(first.snapshot.get().unavailable);
    assert_eq!(first.snapshot.get().handler, Handler::Unavailable);
    assert_eq!(first.snapshot.get().depth, 0);
    drop(nested); drop(dispatch);
    assert!(current().is_none());
    let output = first.diagnostic_json();
    assert!(output.len() <= 256);
    assert!(output.contains(r#""handler":"unavailable""#));
    for raw in ["hwnd", "wparam", "lparam", "pointer", "class_name"] { assert!(!output.contains(raw)); }
}

#[test]
fn synthetic_dispatch_trace_nested_foreign_timer_cannot_become_main_window() {
    let record = DispatchRecord::new();
    { let _dispatch = DispatchScope::enter(Some(&record), MessageKind::WindowTimer,
        DispatchIdentity::new(41, 23, 0));
        // The foreign procedure opened a modal pump before any main procedure entry.
        let _nested_main = enter_main_timer(identity(), Handler::ImportOffer);
        assert_eq!(record.snapshot.get().kind, MessageKind::WindowTimer);
        assert!(record.snapshot.get().unavailable);
        assert_eq!(record.snapshot.get().window, WindowKind::Unavailable);
        assert_eq!(record.snapshot.get().handler, Handler::Unavailable);
        assert!(!record.snapshot.get().entered); }
    assert_eq!(record.snapshot.get().depth, 0);
    // Neither a matching timer ID on another HWND nor another callback can be proof.
    for mismatch in [DispatchIdentity::new(17, 24, 0), DispatchIdentity::new(17, 23, 1)] {
        let _dispatch = DispatchScope::enter(Some(&record), MessageKind::WindowTimer, mismatch);
        let _main = enter_main_timer(identity(), Handler::ImportOffer);
        assert!(record.snapshot.get().unavailable);
    }
}

#[test]
fn synthetic_dispatch_trace_identity_lives_only_in_lexical_binding() {
    let record = DispatchRecord::new();
    let token;
    { let _dispatch = DispatchScope::enter(Some(&record), MessageKind::WindowTimer, identity());
        token = Rc::downgrade(&current().unwrap());
        { let _masked = DispatchScope::enter(None, MessageKind::WindowTimer, identity());
            assert!(current().is_none()); assert!(token.upgrade().is_some()); }
        assert!(is_current(&token.upgrade().unwrap())); }
    assert!(current().is_none()); assert!(token.upgrade().is_none());
    let unwind_token = Cell::new(None);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _dispatch = DispatchScope::enter(Some(&record), MessageKind::WindowTimer, identity());
        unwind_token.set(Some(Rc::downgrade(&current().unwrap())));
        panic!("synthetic token lifetime unwind");
    }));
    assert!(unwind_token.take().unwrap().upgrade().is_none());
    assert!(current().is_none());
    // A retained handler cannot extend the raw token lifetime beyond DispatchScope.
    let dispatch = DispatchScope::enter(Some(&record), MessageKind::WindowTimer, identity());
    let retained_token = Rc::downgrade(&current().unwrap());
    let retained_handler = enter_main_timer(identity(), Handler::ImportOffer);
    drop(dispatch);
    assert!(current().is_none()); assert!(retained_token.upgrade().is_none());
    assert!(record.snapshot.get().unavailable);
    drop(retained_handler);
    assert_eq!(record.snapshot.get().depth, 0);
    assert!(record.snapshot.get().exited);
    // A nested dispatch only weakly remembers its predecessor. Dropping the
    // predecessor out of order must not retain or resurrect its raw identity.
    let outer = DispatchScope::enter(Some(&record), MessageKind::WindowTimer, identity());
    let outer_token = Rc::downgrade(&current().unwrap());
    let inner_record = DispatchRecord::new();
    let inner = DispatchScope::enter(Some(&inner_record), MessageKind::WindowTimer, identity());
    let inner_token = Rc::downgrade(&current().unwrap());
    drop(outer);
    assert!(outer_token.upgrade().is_none()); assert!(current().is_none());
    assert!(record.snapshot.get().unavailable); assert!(inner_record.snapshot.get().unavailable);
    drop(inner);
    assert!(inner_token.upgrade().is_none()); assert!(current().is_none());
    let serialized = record.diagnostic_json();
    for raw in ["identity", "window:17", "timer:23", "callback", "hwnd", "wparam", "lparam"] {
        assert!(!serialized.contains(raw));
    }
}
