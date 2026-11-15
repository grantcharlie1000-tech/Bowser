//! Exercises the real reconciliation function without a WebView2 runtime or profile.
//! Allocation counts establish host work only, not task time or process RAM.

use super::*;
use std::alloc::{GlobalAlloc, Layout, System};

thread_local! {
    static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) };
}

struct CountedAllocator;

fn record_allocation() {
    let _ = ALLOCATIONS.try_with(|count| {
        if let Some(value) = count.get() { count.set(Some(value + 1)); }
    });
}

unsafe impl GlobalAlloc for CountedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_allocation();
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountedAllocator = CountedAllocator;

fn allocation_count<T>(run: impl FnOnce() -> T) -> (T, usize) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) { ALLOCATIONS.with(|count| count.set(None)); }
    }
    ALLOCATIONS.with(|count| assert!(count.replace(Some(0)).is_none()));
    let reset = Reset;
    let result = run();
    let count = ALLOCATIONS.with(|count| count.get().unwrap());
    drop(reset);
    (result, count)
}

#[derive(Debug, PartialEq)]
struct Call {
    add: bool,
    uri: String,
    context: COREWEBVIEW2_WEB_RESOURCE_CONTEXT,
}

#[derive(Default)]
struct RegistrationProbe {
    calls: RefCell<Vec<Call>>,
    fail_add_after: Cell<Option<usize>>,
    block: Rc<TabBlockState>,
}

impl RegistrationProbe {
    fn record(&self, add: bool, uri: &PCWSTR, context: COREWEBVIEW2_WEB_RESOURCE_CONTEXT) -> Result<()> {
        assert!(self.block.request_filters.try_borrow_mut().is_ok(), "registration held a RefCell borrow across COM");
        self.calls.borrow_mut().push(Call { add, uri: unsafe { uri.to_string()? }, context });
        if add && let Some(remaining) = self.fail_add_after.get() {
            if remaining == 0 { return Err(Error::from_hresult(windows::Win32::Foundation::E_FAIL)); }
            self.fail_add_after.set(Some(remaining - 1));
        }
        Ok(())
    }
}

// This implements the complete older interface. Unsupported operations return E_NOTIMPL;
// QueryInterface for _22 fails, exercising the real legacy registration branch.
#[implement(ICoreWebView2)]
struct FilterWebView(Rc<RegistrationProbe>);

macro_rules! unsupported {
    ($($name:ident($($arg:ident: $ty:ty),*) -> $result:ty;)*) => {$(
        fn $name(&self, $($arg: $ty),*) -> $result {
            $(let _ = $arg;)*
            Err(Error::from_hresult(windows::Win32::Foundation::E_NOTIMPL))
        }
    )*};
}

#[allow(non_snake_case)]
impl ICoreWebView2_Impl for FilterWebView_Impl {
    fn AddWebResourceRequestedFilter(&self, uri: &PCWSTR, context: COREWEBVIEW2_WEB_RESOURCE_CONTEXT) -> Result<()> {
        self.0.record(true, uri, context)
    }
    fn RemoveWebResourceRequestedFilter(&self, uri: &PCWSTR, context: COREWEBVIEW2_WEB_RESOURCE_CONTEXT) -> Result<()> {
        self.0.record(false, uri, context)
    }
    unsupported! {
        Settings() -> windows::core::Result<ICoreWebView2Settings>;
        Source(uri: *mut windows::core::PWSTR) -> windows::core::Result<()>;
        Navigate(uri: &windows::core::PCWSTR) -> windows::core::Result<()>;
        NavigateToString(htmlcontent: &windows::core::PCWSTR) -> windows::core::Result<()>;
        add_NavigationStarting( eventhandler: windows::core::Ref<ICoreWebView2NavigationStartingEventHandler>, token: *mut i64) -> windows::core::Result<()>;
        remove_NavigationStarting(token: i64) -> windows::core::Result<()>;
        add_ContentLoading( eventhandler: windows::core::Ref<ICoreWebView2ContentLoadingEventHandler>, token: *mut i64) -> windows::core::Result<()>;
        remove_ContentLoading(token: i64) -> windows::core::Result<()>;
        add_SourceChanged( eventhandler: windows::core::Ref<ICoreWebView2SourceChangedEventHandler>, token: *mut i64) -> windows::core::Result<()>;
        remove_SourceChanged(token: i64) -> windows::core::Result<()>;
        add_HistoryChanged( eventhandler: windows::core::Ref<ICoreWebView2HistoryChangedEventHandler>, token: *mut i64) -> windows::core::Result<()>;
        remove_HistoryChanged(token: i64) -> windows::core::Result<()>;
        add_NavigationCompleted( eventhandler: windows::core::Ref<ICoreWebView2NavigationCompletedEventHandler>, token: *mut i64) -> windows::core::Result<()>;
        remove_NavigationCompleted(token: i64) -> windows::core::Result<()>;
        add_FrameNavigationStarting( eventhandler: windows::core::Ref<ICoreWebView2NavigationStartingEventHandler>, token: *mut i64) -> windows::core::Result<()>;
        remove_FrameNavigationStarting(token: i64) -> windows::core::Result<()>;
        add_FrameNavigationCompleted( eventhandler: windows::core::Ref<ICoreWebView2NavigationCompletedEventHandler>, token: *mut i64) -> windows::core::Result<()>;
        remove_FrameNavigationCompleted(token: i64) -> windows::core::Result<()>;
        add_ScriptDialogOpening( eventhandler: windows::core::Ref<ICoreWebView2ScriptDialogOpeningEventHandler>, token: *mut i64) -> windows::core::Result<()>;
        remove_ScriptDialogOpening(token: i64) -> windows::core::Result<()>;
        add_PermissionRequested( eventhandler: windows::core::Ref<ICoreWebView2PermissionRequestedEventHandler>, token: *mut i64) -> windows::core::Result<()>;
        remove_PermissionRequested(token: i64) -> windows::core::Result<()>;
        add_ProcessFailed( eventhandler: windows::core::Ref<ICoreWebView2ProcessFailedEventHandler>, token: *mut i64) -> windows::core::Result<()>;
        remove_ProcessFailed(token: i64) -> windows::core::Result<()>;
        AddScriptToExecuteOnDocumentCreated( javascript: &windows::core::PCWSTR, handler: windows::core::Ref< ICoreWebView2AddScriptToExecuteOnDocumentCreatedCompletedHandler>) -> windows::core::Result<()>;
        RemoveScriptToExecuteOnDocumentCreated( id: &windows::core::PCWSTR) -> windows::core::Result<()>;
        ExecuteScript( javascript: &windows::core::PCWSTR, handler: windows::core::Ref<ICoreWebView2ExecuteScriptCompletedHandler>) -> windows::core::Result<()>;
        CapturePreview( imageformat: COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT, imagestream: windows::core::Ref<windows::Win32::System::Com::IStream>, handler: windows::core::Ref<ICoreWebView2CapturePreviewCompletedHandler>) -> windows::core::Result<()>;
        Reload() -> windows::core::Result<()>;
        PostWebMessageAsJson( webmessageasjson: &windows::core::PCWSTR) -> windows::core::Result<()>;
        PostWebMessageAsString( webmessageasstring: &windows::core::PCWSTR) -> windows::core::Result<()>;
        add_WebMessageReceived( handler: windows::core::Ref<ICoreWebView2WebMessageReceivedEventHandler>, token: *mut i64) -> windows::core::Result<()>;
        remove_WebMessageReceived(token: i64) -> windows::core::Result<()>;
        CallDevToolsProtocolMethod( methodname: &windows::core::PCWSTR, parametersasjson: &windows::core::PCWSTR, handler: windows::core::Ref<ICoreWebView2CallDevToolsProtocolMethodCompletedHandler>) -> windows::core::Result<()>;
        BrowserProcessId(value: *mut u32) -> windows::core::Result<()>;
        CanGoBack(cangoback: *mut windows::core::BOOL) -> windows::core::Result<()>;
        CanGoForward(cangoforward: *mut windows::core::BOOL) -> windows::core::Result<()>;
        GoBack() -> windows::core::Result<()>;
        GoForward() -> windows::core::Result<()>;
        GetDevToolsProtocolEventReceiver( eventname: &windows::core::PCWSTR) -> windows::core::Result<ICoreWebView2DevToolsProtocolEventReceiver>;
        Stop() -> windows::core::Result<()>;
        add_NewWindowRequested( eventhandler: windows::core::Ref<ICoreWebView2NewWindowRequestedEventHandler>, token: *mut i64) -> windows::core::Result<()>;
        remove_NewWindowRequested(token: i64) -> windows::core::Result<()>;
        add_DocumentTitleChanged( eventhandler: windows::core::Ref<ICoreWebView2DocumentTitleChangedEventHandler>, token: *mut i64) -> windows::core::Result<()>;
        remove_DocumentTitleChanged(token: i64) -> windows::core::Result<()>;
        DocumentTitle(title: *mut windows::core::PWSTR) -> windows::core::Result<()>;
        AddHostObjectToScript( name: &windows::core::PCWSTR, object: *mut windows::Win32::System::Variant::VARIANT) -> windows::core::Result<()>;
        RemoveHostObjectFromScript(name: &windows::core::PCWSTR) -> windows::core::Result<()>;
        OpenDevToolsWindow() -> windows::core::Result<()>;
        add_ContainsFullScreenElementChanged( eventhandler: windows::core::Ref<ICoreWebView2ContainsFullScreenElementChangedEventHandler>, token: *mut i64) -> windows::core::Result<()>;
        remove_ContainsFullScreenElementChanged(token: i64) -> windows::core::Result<()>;
        ContainsFullScreenElement( containsfullscreenelement: *mut windows::core::BOOL) -> windows::core::Result<()>;
        add_WebResourceRequested( eventhandler: windows::core::Ref<ICoreWebView2WebResourceRequestedEventHandler>, token: *mut i64) -> windows::core::Result<()>;
        remove_WebResourceRequested(token: i64) -> windows::core::Result<()>;
        add_WindowCloseRequested( eventhandler: windows::core::Ref<ICoreWebView2WindowCloseRequestedEventHandler>, token: *mut i64) -> windows::core::Result<()>;
        remove_WindowCloseRequested(token: i64) -> windows::core::Result<()>;
    }
}

fn webview(probe: &Rc<RegistrationProbe>) -> ICoreWebView2 {
    FilterWebView(probe.clone()).into()
}

fn strings(uris: &[&str]) -> Vec<String> {
    uris.iter().map(|uri| (*uri).to_owned()).collect()
}

#[test]
fn no_change_reconciliation_does_not_copy_registered_uris() {
    let probe = Rc::new(RegistrationProbe::default());
    let webview = webview(&probe);
    let filters: Vec<String> = (0..64).map(|index| format!("*://*host{index}.test/????*" )).collect();
    sync_request_filters(&webview, &probe.block, 3, filters.clone()).unwrap();
    probe.calls.borrow_mut().clear();
    let (result, allocations) = allocation_count(|| sync_request_filters(&webview, &probe.block, 4, filters));
    assert_eq!(result.unwrap(), 0);
    assert!(probe.calls.borrow().is_empty(), "unchanged filters must not touch the COM registrations");
    assert_eq!(probe.block.request_filters.borrow().as_ref().unwrap().0, 4);
    eprintln!("no-change reconciliation allocation calls: {allocations}");
    // Converting the supplied URI vector to tuples can allocate; copying each of the 64
    // retained URI strings cannot fit this bound. Fixture construction is outside the count.
    assert!(allocations <= 2, "no-change reconciliation copied the registered plan: {allocations} allocations");
}

#[test]
fn reconciliation_removes_and_adds_only_changed_uris_across_all_eight_contexts() {
    let probe = Rc::new(RegistrationProbe::default());
    let webview = webview(&probe);
    assert_eq!(sync_request_filters(&webview, &probe.block, 1, strings(&["*old*", "*keep*"])).unwrap(), 16);
    probe.calls.borrow_mut().clear();
    assert_eq!(sync_request_filters(&webview, &probe.block, 2, strings(&["*keep*", "*new*"])).unwrap(), 8);
    let calls = probe.calls.borrow();
    let expected_contexts = [
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_EVENT_SOURCE,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_OTHER,
    ];
    assert_eq!(calls.len(), 16);
    for (call, context) in calls[..8].iter().zip(expected_contexts) {
        assert!(!call.add);
        assert_eq!(call.uri, "*old*");
        assert_eq!(call.context, context);
    }
    for (call, context) in calls[8..].iter().zip(expected_contexts) {
        assert!(call.add);
        assert_eq!(call.uri, "*new*");
        assert_eq!(call.context, context);
    }
    let stored = probe.block.request_filters.borrow();
    assert_eq!(stored.as_ref().unwrap(), &(2, vec![
        ("*keep*".to_owned(), COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL),
        ("*new*".to_owned(), COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL),
    ]));
}

#[test]
fn partial_add_failure_does_not_publish_a_successful_generation() {
    let probe = Rc::new(RegistrationProbe::default());
    let webview = webview(&probe);
    sync_request_filters(&webview, &probe.block, 7, strings(&["*old*"])).unwrap();
    probe.calls.borrow_mut().clear();
    probe.fail_add_after.set(Some(3));
    assert!(sync_request_filters(&webview, &probe.block, 8, strings(&["*new*"])).is_err());
    assert_eq!(probe.calls.borrow().len(), 12, "eight removals, three additions, then the failed addition");
    assert_eq!(probe.block.request_filters.borrow().as_ref().unwrap(), &(7, vec![
        ("*old*".to_owned(), COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL),
    ]));
    probe.fail_add_after.set(None);
    probe.calls.borrow_mut().clear();
    assert_eq!(sync_request_filters(&webview, &probe.block, 8, strings(&["*new*"])).unwrap(), 8);
    assert_eq!(probe.block.request_filters.borrow().as_ref().unwrap().0, 8);
    assert_eq!(probe.calls.borrow().len(), 16, "retry still reconciles against the previous snapshot");
}
