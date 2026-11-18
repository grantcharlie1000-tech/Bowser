//! Drives the download registry with a COM operation double, without a runtime or live profile.
use super::*;
use windows::core::{Ref, implement};

#[derive(Default)]
struct OperationState {
    state: Cell<COREWEBVIEW2_DOWNLOAD_STATE>,
    reason: Cell<COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON>,
    resumable: Cell<bool>,
    unreadable: Cell<bool>,
    unreadable_reason: Cell<bool>,
    unreadable_resume: Cell<bool>,
    on_state: RefCell<Option<Box<dyn FnOnce()>>>,
    cancelled: Cell<bool>,
}

#[implement(ICoreWebView2DownloadOperation)]
struct Operation { state: Rc<OperationState> }

fn unsupported<T>() -> Result<T> { Err(windows::Win32::Foundation::E_NOTIMPL.into()) }

#[allow(non_snake_case)]
impl ICoreWebView2DownloadOperation_Impl for Operation_Impl {
    fn add_BytesReceivedChanged(&self, _: Ref<ICoreWebView2BytesReceivedChangedEventHandler>, _: *mut i64) -> Result<()> { unsupported() }
    fn remove_BytesReceivedChanged(&self, _: i64) -> Result<()> { unsupported() }
    fn add_EstimatedEndTimeChanged(&self, _: Ref<ICoreWebView2EstimatedEndTimeChangedEventHandler>, _: *mut i64) -> Result<()> { unsupported() }
    fn remove_EstimatedEndTimeChanged(&self, _: i64) -> Result<()> { unsupported() }
    fn add_StateChanged(&self, _: Ref<ICoreWebView2StateChangedEventHandler>, _: *mut i64) -> Result<()> { unsupported() }
    fn remove_StateChanged(&self, _: i64) -> Result<()> { unsupported() }
    fn Uri(&self, _: *mut PWSTR) -> Result<()> { unsupported() }
    fn ContentDisposition(&self, _: *mut PWSTR) -> Result<()> { unsupported() }
    fn MimeType(&self, _: *mut PWSTR) -> Result<()> { unsupported() }
    fn TotalBytesToReceive(&self, total: *mut i64) -> Result<()> { unsafe { *total = 64; } Ok(()) }
    fn BytesReceived(&self, got: *mut i64) -> Result<()> { unsafe { *got = 16; } Ok(()) }
    fn EstimatedEndTime(&self, _: *mut PWSTR) -> Result<()> { unsupported() }
    fn ResultFilePath(&self, _: *mut PWSTR) -> Result<()> { unsupported() }
    fn State(&self, state: *mut COREWEBVIEW2_DOWNLOAD_STATE) -> Result<()> {
        let callback = self.state.on_state.borrow_mut().take();
        if let Some(callback) = callback { callback(); }
        if self.state.unreadable.get() { return Err(windows::Win32::Foundation::E_FAIL.into()); }
        unsafe { *state = self.state.state.get(); } Ok(())
    }
    fn InterruptReason(&self, reason: *mut COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON) -> Result<()> {
        if self.state.unreadable_reason.get() { return Err(windows::Win32::Foundation::E_FAIL.into()); }
        unsafe { *reason = self.state.reason.get(); } Ok(())
    }
    fn Cancel(&self) -> Result<()> { self.state.cancelled.set(true); Ok(()) }
    fn Pause(&self) -> Result<()> {
        self.state.state.set(COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED);
        self.state.reason.set(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_PAUSED);
        self.state.resumable.set(true); Ok(())
    }
    fn Resume(&self) -> Result<()> {
        self.state.state.set(COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS);
        self.state.reason.set(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NONE); Ok(())
    }
    fn CanResume(&self, resumable: *mut BOOL) -> Result<()> {
        if self.state.unreadable_resume.get() { return Err(windows::Win32::Foundation::E_FAIL.into()); }
        unsafe { *resumable = self.state.resumable.get().into(); } Ok(())
    }
}

pub(crate) struct Download {
    pub(crate) key: u64,
    state: Rc<OperationState>,
    pub(crate) events: Rc<RefCell<Vec<DownloadEvent>>>,
}

impl Download {
    pub(crate) fn new(owner: u64) -> Self {
        let state = Rc::new(OperationState::default());
        state.state.set(COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS);
        let op = Operation { state: state.clone() }.into();
        let key = NEXT_KEY.with(|next| { let key = next.get(); next.set(key + 1); key });
        let events = Rc::new(RefCell::new(Vec::new()));
        let observed = events.clone();
        OPS.with(|ops| ops.borrow_mut().insert(key, Tracked { op, owner, event: Rc::new(move |event| observed.borrow_mut().push(event)), webview: None, review_armed: false }));
        Self { key, state, events }
    }
    pub(crate) fn cancelled(&self) -> bool { self.state.cancelled.get() }
}

impl Drop for Download { fn drop(&mut self) { release(&[self.key]); } }

#[test]
fn download_recovery_paused_and_resumable_operations_keep_their_owner_awake() {
    let owner = tab_owner(12345);
    let download = Download::new(owner);
    assert!(owner_downloading(owner));
    assert!(pause(download.key));
    assert!(owner_downloading(owner), "a paused operation still needs its controller for Resume");
    assert!(resume(download.key));
    download.state.state.set(COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED);
    download.state.reason.set(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_DISCONNECTED);
    download.state.resumable.set(true);
    assert!(owner_downloading(owner), "a resumable failure must survive sleeping-tab policy");
    assert!(!owner_downloading(tab_owner(12346)), "ownership must remain exact");
    assert!(!download.cancelled());
}

#[test]
fn download_recovery_completed_and_terminal_operations_release_sleep_protection() {
    let owner = tab_owner(12345);
    let download = Download::new(owner);
    download.state.state.set(COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED);
    assert!(!owner_downloading(owner));
    download.state.state.set(COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED);
    download.state.reason.set(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_CANCELED);
    assert!(!owner_downloading(owner));
}

#[test]
fn download_recovery_an_unreadable_live_operation_fails_closed_for_sleep() {
    let owner = tab_owner(12345);
    let download = Download::new(owner);
    download.state.unreadable.set(true);
    assert!(owner_downloading(owner), "unreadable COM state cannot authorize destroying a download");
}

#[test]
fn download_recovery_unknown_interrupted_and_resume_states_fail_closed() {
    let owner = tab_owner(12345);
    let download = Download::new(owner);
    download.state.state.set(COREWEBVIEW2_DOWNLOAD_STATE(99));
    assert!(owner_downloading(owner));
    download.state.state.set(COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED);
    download.state.unreadable_reason.set(true);
    assert!(owner_downloading(owner));
    download.state.unreadable_reason.set(false);
    download.state.reason.set(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_DISCONNECTED);
    download.state.unreadable_resume.set(true);
    assert!(owner_downloading(owner));
    download.state.unreadable_resume.set(false);
    assert!(!owner_downloading(owner), "a known non-resumable terminal operation can release the tab");
}

#[test]
fn download_recovery_native_state_queries_can_reenter_registry_commands() {
    let owner = tab_owner(12345);
    let download = Download::new(owner);
    let key = download.key;
    *download.state.on_state.borrow_mut() = Some(Box::new(move || release(&[key])));
    assert!(owner_downloading(owner), "the snapshot conservatively retains the observed operation");
    assert!(download.cancelled());
    assert!(!owner_downloading(owner), "the next check observes completed registry removal");
}
