//! Opt-in real-App integration lane, never the `Session`/`Static` presentation experiment.
//!
//! Required integration seams (all `cfg(test)`, absent from release binaries):
//! - `engine::mod`: `pub(crate) mod app_context_menu_probe;` and a forwarding
//!   `set_menu_probe_observer(Rc<dyn Fn(MenuProbeEvent)>)` into the production coordinator.
//! - `EngineAdapter::probe_native_context_handles(TabId) -> Option<NativeContextProbeHandles>`
//!   and clone-only `probe_extension_receipt_handles()`, both defaulting to `None`.
//!   Implementations only clone handles/closures under App/Core borrows. All native reads,
//!   script calls, fences and nested pumping happen after those borrows have ended.
//! - `win32::set_app_menu_probe_hooks(Box<dyn FnOnce(AppRef)>, Box<dyn FnMut(AppRef)>)`:
//!   invoke ready once after real window/startup navigation; invoke tick on its exact-window
//!   test timer. Take callbacks out before invoking, catch unwinds before any extern boundary,
//!   and drop every App borrow first. The test below additionally catches ready's unwind.
//! - Production coordinator emits `BeforeShow` from the owning *current* engine request and
//!   `Completed` only after its real deferral completes. The fixture command is a test-only
//!   diagnostic selected from the unique enabled extension-created engine leaf, not a dispatch
//!   authorization. No test-created menu, manual event registration or ID setter exists here.
//!
//! Run ONLY the exact ignored test, in its own process, with an interactive unlocked desktop:
//! `BOWSER_APP_MENU_PROBE_CONSENT=probe-custom-context-menu` (PowerShell environment syntax)
//! `cargo test --lib --features html-chrome-default,experimental-engine-context-menu engine::app_context_menu_probe::live_probe_real_app_context_menu -- --exact --ignored --nocapture --test-threads=1`
//!
//! This intentionally narrow warm-worker lane covers cancel + one fixture selection. Copy/Paste,
//! host commands, no-activeTab/cross-origin permission matrix, worker restart, multi-profile and
//! in-modal navigation/close/shutdown races remain NOT RUN. No marketplace compatibility claim.

pub(crate) mod reload;

use std::{
    cell::{Cell, RefCell},
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    process::{Command, Stdio},
    rc::Rc,
    sync::{Arc, atomic::{AtomicBool, Ordering}},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use webview2_com::{CoTaskMemPWSTR, ExecuteScriptCompletedHandler, Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2Controller, ICoreWebView2Environment,
    ICoreWebView2_13, ICoreWebView2Environment7, COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC,
}};
use windows::{core::{Interface, PWSTR}, Win32::{
    Foundation::{HWND, RECT, WPARAM, LPARAM},
    System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED},
    UI::WindowsAndMessaging::{
        GetClassNameW, GetClientRect, GetForegroundWindow,
        IsWindow, PostMessageW, SetForegroundWindow, WM_CLOSE,
    },
}};
use crate::{app::{App, AppRef}, extensions::{self, json::{self, Value}}, performance::{LaunchOptions, PerfLog}, tabs::TabId};

/// Clone-only return value. The proof/fence closures must use `try_borrow` and fail closed;
/// constructing this value must not call any COM method or run user/test code.
#[derive(Clone)]
pub(crate) struct NativeContextProbeHandles {
    pub(crate) view: ICoreWebView2,
    pub(crate) controller: ICoreWebView2Controller,
    pub(crate) environment: ICoreWebView2Environment,
    pub(crate) proof: Rc<dyn Fn() -> Option<(u64, u64, u64)>>,
    pub(crate) document_fence: Rc<dyn Fn() -> bool>,
}
pub(crate) struct NativeReceiptHandles {
    pub(crate) view:ICoreWebView2,pub(crate) controller:ICoreWebView2Controller,pub(crate) environment:ICoreWebView2Environment,pub(crate) owner:HWND,
    pub(crate) proof:Rc<dyn Fn()->Option<(u64,u64)>>,
}
struct Receipt {handles:NativeReceiptHandles,expected:url::Url,captured:(u64,u64)}
impl std::ops::Deref for Receipt {type Target=ICoreWebView2;fn deref(&self)->&ICoreWebView2{&self.handles.view}}
impl Receipt {
    fn current(&self){
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId,IsWindowVisible};
        assert_eq!((self.handles.proof)(),Some(self.captured),"exact hidden receipt/document changed");
        let mut pid=0;let mut name=[0u16;64];let mut parent=HWND::default();let mut source=PWSTR::null();
        unsafe{
            assert_eq!(GetWindowThreadProcessId(self.handles.owner,Some(&mut pid)),windows::Win32::System::Threading::GetCurrentThreadId());assert_eq!(pid,std::process::id());
            assert!(IsWindow(Some(self.handles.owner)).as_bool());assert!(!IsWindowVisible(self.handles.owner).as_bool(),"data-only receipt must remain hidden");
            let n=GetClassNameW(self.handles.owner,&mut name);assert_eq!(String::from_utf16_lossy(&name[..n as usize]),"BowserExtensionPage");
            self.handles.controller.ParentWindow(&mut parent).unwrap();assert_eq!(parent,self.handles.owner);
            let view=self.handles.controller.CoreWebView2().unwrap();assert_eq!(view.cast::<windows::core::IUnknown>().unwrap().as_raw(),self.handles.view.cast::<windows::core::IUnknown>().unwrap().as_raw());
            let environment=self.handles.view.cast::<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_2>().unwrap().Environment().unwrap();
            assert_eq!(environment.cast::<windows::core::IUnknown>().unwrap().as_raw(),self.handles.environment.cast::<windows::core::IUnknown>().unwrap().as_raw());
            let settings=self.handles.view.Settings().unwrap();let mut messages=windows::core::BOOL::default();let mut objects=messages;
            settings.IsWebMessageEnabled(&mut messages).unwrap();settings.AreHostObjectsAllowed(&mut objects).unwrap();assert!(!messages.as_bool()&&!objects.as_bool());
            self.handles.view.Source(&mut source).unwrap();
        }
        assert!(!source.is_null());let allocation=CoTaskMemPWSTR::from(source);
        let length=(0..=2048).find(|index|unsafe{*source.0.add(*index)==0}).expect("bounded native receipt URI unavailable");
        let actual=String::from_utf16(unsafe{std::slice::from_raw_parts(source.0,length)}).unwrap();drop(allocation);
        assert_eq!(actual,self.expected.as_str());ordinary_profile(self);
        assert_eq!((self.handles.proof)(),Some(self.captured),"hidden receipt changed across native identity reads");
    }
}

/// Read-only diagnostic emitted at the actual production menu coordinator boundaries.
#[derive(Clone, Copy, Debug)]
pub(crate) enum MenuProbeEvent {
    BeforeShow { request: usize, tab: TabId, view_identity: usize,
        proof: (u64, u64, u64), fixture_command: i32 },
    Completed { request: usize, selected: Option<i32>, setter_calls: u32,
        setter_result:Option<Result<(),i32>>, complete_calls: u32, complete_error: Option<i32>, refused: bool, cleanup_ok: bool },
    Deregistered { native_error: Option<i32> },
}

const TEST_NAME: &str = "engine::app_context_menu_probe::live_probe_real_app_context_menu";
const FIXTURE_ID: &str = "ciocoifocbggchcecjgibbaeojegkpen";
const FIXTURE_CONSENT: &str = "probe-custom-context-menu";
const TARGET_PAGE: &str = r#"<!doctype html><meta charset="utf-8"><title>Disposable real App native menu fixture</title>
<style>body{margin:24px}textarea{width:320px;height:140px}</style>
<label for="app-context-target">Disposable native menu source</label><br>
<textarea id="app-context-target">Disposable real App native menu text</textarea>
<script>document.addEventListener('contextmenu',e=>{
 const t=document.getElementById('app-context-target');
 if(e.isTrusted&&e.button===2&&e.target===t){
  const r=document.documentElement;
  r.setAttribute('data-app-context-trusted',String(Number(r.getAttribute('data-app-context-trusted')||0)+1));
 }
});</script>"#;

#[derive(Default)]
struct LoopbackReads {accepted:std::sync::atomic::AtomicUsize,parsed:std::sync::atomic::AtomicUsize,served:std::sync::atomic::AtomicUsize}
fn app_loopback_target(socket:&mut std::net::TcpStream)->Option<String>{
    // Winsock accepts may inherit nonblocking mode. Keep the original one-second,
    // 2048-byte bound, but wait for complete headers before serving the document.
    socket.set_nonblocking(false).ok()?;let deadline=Instant::now()+Duration::from_secs(1);
    let mut bytes=[0u8;2048];let mut used=0;
    loop {
        socket.set_read_timeout(Some(deadline.checked_duration_since(Instant::now())?)).ok()?;
        let count=socket.read(&mut bytes[used..]).ok()?;if count==0{return None}used+=count;
        if used>=bytes.len()||Instant::now()>=deadline{return None}
        if bytes[..used].windows(4).any(|part|part==b"\r\n\r\n"){break}
    }
    std::str::from_utf8(&bytes[..used]).ok()?.lines().next()?.strip_prefix("GET ")?.split_whitespace().next().map(str::to_owned)
}
struct Loopback { url: url::Url, stop: Arc<AtomicBool>, reads:Arc<LoopbackReads>, worker: Option<std::thread::JoinHandle<()>> }
impl Loopback {
    fn start() -> Self {
        let listener=TcpListener::bind("127.0.0.1:0").unwrap();
        let url=url::Url::parse(&format!("http://127.0.0.1:{}/app-native-menu",listener.local_addr().unwrap().port())).unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop=Arc::new(AtomicBool::new(false)); let stopping=stop.clone();let reads=Arc::new(LoopbackReads::default());let observed=reads.clone();
        let worker=std::thread::spawn(move || while !stopping.load(Ordering::Relaxed) {
            let Ok((mut socket,_))=listener.accept() else {std::thread::sleep(Duration::from_millis(10));continue};
            observed.accepted.fetch_add(1,Ordering::Relaxed);
            let Some(path)=app_loopback_target(&mut socket) else{continue};observed.parsed.fetch_add(1,Ordering::Relaxed);
            let (status,body)=if path=="/app-native-menu" {observed.served.fetch_add(1,Ordering::Relaxed);("200 OK",TARGET_PAGE)} else {("404 Not Found","")};
            let _=write!(socket,"HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",body.len());
        });
        Self {url,stop,reads,worker:Some(worker)}
    }
}
impl Drop for Loopback {fn drop(&mut self) {self.stop.store(true,Ordering::Relaxed);if let Some(worker)=self.worker.take(){let _=worker.join();}}}

#[test]
fn app_loopback_waits_for_late_fragmented_headers(){
    let server=Loopback::start();let address=format!("127.0.0.1:{}",server.url.port().unwrap());
    let mut socket=std::net::TcpStream::connect(address).unwrap();socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    // Force accept to precede the first payload, then split the request target itself.
    std::thread::sleep(Duration::from_millis(50));
    socket.write_all(b"GET /app-native-").expect("accepted socket must wait for fixture bytes");
    std::thread::sleep(Duration::from_millis(20));
    socket.write_all(b"menu HTTP/1.1\r\nHost: 127.0.0.1\r\n").expect("partial headers must not cause premature response/close");
    std::thread::sleep(Duration::from_millis(20));socket.write_all(b"\r\n").unwrap();
    let mut response=String::new();socket.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"),"complete fixture request was lost: {response}");
    assert!(response.ends_with(TARGET_PAGE));
}
#[test]
fn app_loopback_refuses_oversize_incomplete_headers(){
    let server=Loopback::start();let mut socket=std::net::TcpStream::connect(format!("127.0.0.1:{}",server.url.port().unwrap())).unwrap();
    socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let mut bytes=b"GET /app-native-menu HTTP/1.1\r\nX: ".to_vec();bytes.extend(std::iter::repeat_n(b'a',2048));
    let _=socket.write_all(&bytes);let mut response=String::new();let _=socket.read_to_string(&mut response);
    assert!(!response.contains(TARGET_PAGE),"incomplete/oversize request must never receive the controlled page");
}

fn pump_until(timeout: Duration, ready: impl FnMut() -> bool) -> bool {
    super::native_probe_pump::until(timeout,ready)
}

fn field<'a>(v: &'a Value,k: &str)->Option<&'a Value>{let Value::Object(o)=v else{return None};json::get(o,k)}
fn text<'a>(v: &'a Value,k: &str)->&'a str{let Some(Value::String(s))=field(v,k) else{panic!("missing string {k}")};s}
fn number(v: &Value,k: &str)->i64{let Some(Value::Number(s))=field(v,k) else{panic!("missing number {k}")};s.parse().unwrap()}
fn array<'a>(v: &'a Value,k: &str)->&'a [Value]{let Some(Value::Array(a))=field(v,k) else{panic!("missing array {k}")};a}
fn assert_bool(v: &Value,k: &str,b: bool){assert_eq!(field(v,k),Some(&Value::Bool(b)),"{k}: {v:?}");}

/// No App/engine borrow survives the trait getter. Its implementation must be clone-only.
fn handles(app: &AppRef,tab: TabId)->Option<NativeContextProbeHandles>{
    let app=app.try_borrow().ok()?;
    app.engine.as_ref()?.probe_native_context_handles(tab)
}
fn receipt_handles(app: &AppRef)->Option<NativeReceiptHandles>{
    let app=app.try_borrow().ok()?;
    app.engine.as_ref()?.probe_extension_receipt_handles()
}
fn open_receipt_view(app:&AppRef,id:&str,folder:&Path)->Receipt{
    let outcome=Rc::new(RefCell::new(None));let completed=outcome.clone();
    App::probe_open_extension_receipt(app,id,folder,Box::new(move|result|*completed.borrow_mut()=Some(result)));
    assert!(pump_until(Duration::from_secs(30),||outcome.borrow().is_some()),"hidden native fixture receipt did not open");
    outcome.borrow_mut().take().unwrap().expect("real loaded-extension receipt creation failed");
    let handles=receipt_handles(app).expect("completed native receipt has no exact controller/view");
    assert!(pump_until(Duration::from_secs(20),||(handles.proof)().is_some()&&script(&handles.view,"document.documentElement.getAttribute('data-ready')")==Value::String("yes".into())),"successful hidden receipt document did not settle");
    let captured=(handles.proof)().unwrap();let expected=extensions::extension_page_url(folder,extensions::PageKind::Popup,FIXTURE_ID).unwrap();
    let receipt=Receipt{handles,expected,captured};receipt.current();
    println!("APP_MENU_RECEIPT receipt_only=true hidden_native_host={} exact_controller={} exact_view={} successful_navigation={} epoch={} ordinary_profile=true fixed_native_document=true web_messages=false host_objects=false",receipt.handles.owner.0 as usize,receipt.handles.controller.as_raw() as usize,receipt.handles.view.as_raw() as usize,captured.0,captured.1);
    receipt
}
fn script(view: &ICoreWebView2,source: &str)->Value {
    let slot=Rc::new(RefCell::new(None));let sink=slot.clone();
    let handler=ExecuteScriptCompletedHandler::create(Box::new(move |status,result| {
        *sink.borrow_mut()=Some(status.is_ok().then(||result.to_string()));Ok(())
    }));
    let source=CoTaskMemPWSTR::from(source);
    unsafe {view.ExecuteScript(*source.as_ref().as_pcwstr(),&handler).unwrap()};
    assert!(pump_until(Duration::from_secs(10),||slot.borrow().is_some()),"exact COM script completion timed out");
    let result=slot.borrow_mut().take().flatten().expect("exact COM script failed");
    assert!(result.len()<=64*1024,"fixture receipt exceeded bound");json::parse(&result).expect("native JSON result")
}
fn worker(view: &Receipt,kind: &str)->Value {
    view.current();
    assert!(["native-command-state","native-command-preflight"].contains(&kind));
    let request=format!("(()=>{{document.documentElement.removeAttribute('data-app-menu-receipt');chrome.runtime.sendMessage({{type:{}}},answer=>document.documentElement.setAttribute('data-app-menu-receipt',JSON.stringify(answer)));return true}})()",crate::chrome_host::protocol::json_string(kind));
    assert_eq!(script(view,&request),Value::Bool(true));
    let mut answer=None;
    assert!(pump_until(Duration::from_secs(10),||{
        let value=script(view,"JSON.parse(document.documentElement.getAttribute('data-app-menu-receipt')||'null')");
        if matches!(value,Value::Object(_)){answer=Some(value);true}else{false}
    }),"genuine fixture worker receipt absent");view.current();answer.unwrap()
}
fn marker(view: &ICoreWebView2,role: &str)->Value {
    assert!(["primary","negative"].contains(&role));
    script(view,&format!("document.documentElement.getAttribute('data-bowser-native-command-{role}')"))
}
fn document(view: &ICoreWebView2,page: &url::Url)->Value {
    let value=script(view,"({url:location.href,timeOrigin:performance.timeOrigin,ready:document.readyState})");
    assert_eq!(text(&value,"url"),page.as_str());assert_eq!(text(&value,"ready"),"complete");value
}
fn current(h: &NativeContextProbeHandles,proof: (u64,u64,u64)) {
    assert!((h.document_fence)(),"source COM/document fence refused");assert_eq!((h.proof)(),Some(proof));
}
fn record_foreground_refusal(out:&Path,owner:HWND,observed:HWND,browser:u32){
    // Failure-only metadata for the exact observed HWND. Do not inspect class/text of a
    // foreign process, walk its UI, activate anything, or change the original refusal.
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId,GetParent,GetAncestor,GA_ROOT,GA_ROOTOWNER};
    let mut pid=0;let thread=unsafe{GetWindowThreadProcessId(observed,Some(&mut pid))};
    let known=pid!=0&&(pid==std::process::id()||pid==browser);let mut name=[0u16;64];
    let (class,parent,root,root_owner)=if known&&unsafe{GetForegroundWindow()}==observed {
        let n=unsafe{GetClassNameW(observed,&mut name)};
        let class=if n>0&&n<63 {crate::chrome_host::protocol::json_string(&String::from_utf16_lossy(&name[..n as usize]))}else{"null".into()};
        unsafe{(class,GetParent(observed).unwrap_or_default(),GetAncestor(observed,GA_ROOT),GetAncestor(observed,GA_ROOTOWNER))}
    }else{("null".into(),HWND::default(),HWND::default(),HWND::default())};
    let mut after=0;let after_thread=unsafe{GetWindowThreadProcessId(observed,Some(&mut after))};
    let same=pid!=0&&thread!=0&&!observed.is_invalid()&&pid==after&&thread==after_thread&&unsafe{GetForegroundWindow()}==observed;
    let (parent,root,root_owner)=if same{(parent,root,root_owner)}else{(HWND::default(),HWND::default(),HWND::default())};
    let text=format!(r#"{{"scope":"failure-only-exact-observed-foreground-not-authority","owner":{},"observed":{},"pid":{},"thread":{},"known_test_or_browser_process":{},"identity_same_after_read":{},"class":{},"parent":{},"root":{},"root_owner":{},"helper_launched":false,"activation_retry":false}}"#,
        owner.0 as isize,observed.0 as isize,pid,thread,known,same,if same{class}else{"null".into()},parent.0 as isize,root.0 as isize,root_owner.0 as isize);
    if text.len()<=2048 {let _=std::fs::write(out.join("foreground-refusal.json"),text);}
}
fn isolated_environment(app: &AppRef,h: &NativeContextProbeHandles) {
    let expected=app.borrow().user_data_dir.clone();
    let environment=h.environment.cast::<ICoreWebView2Environment7>().unwrap();
    let mut path=PWSTR::null();unsafe{environment.UserDataFolder(&mut path).unwrap()};
    assert!(!path.is_null());let actual=CoTaskMemPWSTR::from(path).to_string();
    let actual=std::fs::canonicalize(actual).unwrap();let expected=std::fs::canonicalize(expected).unwrap();
    assert!(actual.to_string_lossy().eq_ignore_ascii_case(&expected.to_string_lossy()),"actual COM environment must use the disposable App profile");
}
fn ordinary_profile(view: &ICoreWebView2) {
    let profile=unsafe{view.cast::<ICoreWebView2_13>().unwrap().Profile().unwrap()};
    let mut name=PWSTR::null();let mut private=windows::core::BOOL::default();
    unsafe {profile.ProfileName(&mut name).unwrap();profile.IsInPrivateModeEnabled(&mut private).unwrap()};
    assert!(!name.is_null());let name=unsafe{CoTaskMemPWSTR::from(name).to_string()};
    assert!(name.is_empty(),"this lane requires the ordinary unnamed native profile, not chrome UI");assert!(!private.as_bool());
}
fn authority(root: &Path,bowser_id: &str) {
    let records=extensions::read_authority_records(&root.join("extensions.txt")).unwrap();
    assert_eq!(records.len(),1,"only the explicitly consented fixture may be loaded");
    assert_eq!(records[0].id,bowser_id);assert!(records[0].enabled);
    let mapping=std::fs::read_to_string(root.join("extension_ids.txt")).unwrap();
    assert_eq!(mapping.lines().collect::<Vec<_>>(),vec![format!("{bowser_id}\t{FIXTURE_ID}")]);
}

thread_local! {
    static DEREGISTRATIONS: RefCell<Vec<Option<i32>>> = const { RefCell::new(Vec::new()) };
}
fn install_observer(observer: Rc<dyn Fn(MenuProbeEvent)>) {
    super::set_menu_probe_observer(Rc::new(move |event| {
        if let MenuProbeEvent::Deregistered { native_error }=event {
            DEREGISTRATIONS.with(|events|events.borrow_mut().push(native_error));
        }
        observer(event);
    }));
}
struct ObserverScope;
impl Drop for ObserverScope {fn drop(&mut self){install_observer(Rc::new(|_|{}));}}

fn native_input(app: &AppRef,h: &NativeContextProbeHandles,tab: TabId,proof:(u64,u64,u64),mode: &str,out: &Path)->i32 {
    current(h,proof);ordinary_profile(&h.view);isolated_environment(app,h);
    let owner=app.borrow().hwnd;
    let mut class=[0;64];let n=unsafe{GetClassNameW(owner,&mut class)};
    assert_eq!(String::from_utf16_lossy(&class[..n as usize]),"BowserNativeWindow");
    assert!(unsafe{IsWindow(Some(owner))}.as_bool());
    // Complete DOM/receipt setup before the sole Rust activation, as in the Reload lane.
    assert_eq!(script(&h.view,"(()=>{const t=document.getElementById('app-context-target');t.focus();return document.activeElement===t})()"),Value::Bool(true));
    current(h,proof);
    let source=script(&h.view,"(()=>{const t=document.getElementById('app-context-target'),r=t.getBoundingClientRect(),x=(r.left+r.right)/2,y=(r.top+r.bottom)/2;return {left:r.left,top:r.top,right:r.right,bottom:r.bottom,width:innerWidth,height:innerHeight,ratio:devicePixelRatio,hit:document.elementFromPoint(x,y)===t,trusted:Number(document.documentElement.getAttribute('data-app-context-trusted')||0)}})()");
    assert_bool(&source,"hit",true);
    let mut bounds=RECT::default();let mut client=RECT::default();let mut parent=HWND::default();let mut zoom=0.0;let mut browser=0;
    unsafe{h.controller.Bounds(&mut bounds).unwrap();h.controller.ParentWindow(&mut parent).unwrap();h.controller.ZoomFactor(&mut zoom).unwrap();GetClientRect(owner,&mut client).unwrap();h.view.BrowserProcessId(&mut browser).unwrap()};
    assert_eq!(parent,owner);assert_eq!(zoom,1.0);assert!(browser>0);
    assert!(bounds.left>=0&&bounds.top>=0&&bounds.right<=client.right&&bounds.bottom<=client.bottom);
    let numeric=|key|{let Some(Value::Number(v))=field(&source,key) else{panic!("missing source geometry {key}")};v.parse::<f64>().unwrap()};
    let width=numeric("width");let height=numeric("height");let ratio=numeric("ratio");
    assert!(width>0.0&&height>0.0&&ratio>0.0);
    assert!(((bounds.right-bounds.left) as f64-width*ratio).abs()<=2.0&&((bounds.bottom-bounds.top) as f64-height*ratio).abs()<=2.0,"unverified CSS/controller/device-pixel transform");
    let x=|v:f64|bounds.left+(v*(bounds.right-bounds.left) as f64/width).round() as i32;
    let y=|v:f64|bounds.top+(v*(bounds.bottom-bounds.top) as f64/height).round() as i32;
    let (left,top,right,bottom)=(x(numeric("left")),y(numeric("top")),x(numeric("right")),y(numeric("bottom")));
    let (px,py)=((left+right)/2,(top+bottom)/2);
    std::fs::create_dir_all(out).unwrap();let request_file=out.join("current-request.json");assert!(!request_file.exists());
    let nonce=format!("{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos());
    let observed=Rc::new(RefCell::new(Vec::new()));let sink=observed.clone();let error=Rc::new(RefCell::new(None::<String>));let failure=error.clone();
    let path=request_file.clone();let expected=h.view.as_raw() as usize;let token=nonce.clone();let already=Rc::new(Cell::new(false));
    install_observer(Rc::new(move |event|{
        if let MenuProbeEvent::BeforeShow{request,tab:actual,view_identity,proof:actual_proof,fixture_command}=event.clone(){
            if already.replace(true)||request==0||actual!=tab||view_identity!=expected||actual_proof!=proof||fixture_command<=0 {
                *failure.borrow_mut()=Some("wrong/repeated real coordinator request".into());
            }else{
                // Write-once per helper invocation, using the actual current request item ID.
                let text=format!(r#"{{"schema":"bowser-real-app-menu-v1","nonce":"{token}","phase":"before-show","owner":{},"tab":"{}","request":"{request}","view_identity":"{view_identity}","proof0":"{}","proof1":"{}","proof2":"{}","fixture_command":{fixture_command}}}"#,owner.0 as isize,tab.0,proof.0,proof.1,proof.2);
                let pending=path.with_extension("pending");
                if let Err(e)=std::fs::OpenOptions::new().write(true).create_new(true).open(&pending).and_then(|mut f|{f.write_all(text.as_bytes())?;f.sync_all()?;drop(f);std::fs::rename(&pending,&path)}){*failure.borrow_mut()=Some(format!("request witness write failed: {e}"));}
            }
        }
        sink.borrow_mut().push(event);
    }));
    let _observer=ObserverScope;
    current(h,proof);
    unsafe{assert!(SetForegroundWindow(owner).as_bool(),"exact-owner activation failed; no retry or helper rescue");h.controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC).unwrap()};
    assert!(pump_until(Duration::from_secs(3),||unsafe{GetForegroundWindow()}==owner));
    assert_eq!(script(&h.view,"document.hasFocus()&&document.activeElement===document.getElementById('app-context-target')"),Value::Bool(true));
    current(h,proof);let foreground=unsafe{GetForegroundWindow()};if foreground!=owner {record_foreground_refusal(out,owner,foreground,browser);}assert_eq!(foreground,owner,"exact owner displaced after final script; no retry or helper launch");
    std::fs::write(out.join("source-conditioning.json"),format!(r#"{{"schema":"bowser-real-app-menu-conditioning-v1","setup_complete_before_activation":true,"activation_owner":"Rust-exact-original-App-owner","activation_attempts":1,"actual_activation_return":true,"owner":{},"foreground_after_final_script":{}}}"#,owner.0 as isize,foreground.0 as isize)).unwrap();
    assert_eq!(unsafe{GetForegroundWindow()},owner,"exact owner displaced before helper launch; no retry");
    let mut child=Command::new("powershell.exe").args(["-NoProfile","-File"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/scripts/probe-app-context-menu.ps1"))
        .args(["-OwnerPid",&std::process::id().to_string(),"-OwnerHandle",&(owner.0 as isize).to_string()])
        .arg("-OwnerExe").arg(std::env::current_exe().unwrap())
        .args(["-BrowserPid",&browser.to_string(),"-ClientX",&px.to_string(),"-ClientY",&py.to_string(),"-ViewportWidth",&client.right.to_string(),"-ViewportHeight",&client.bottom.to_string(),"-ControlLeft",&left.to_string(),"-ControlTop",&top.to_string(),"-ControlRight",&right.to_string(),"-ControlBottom",&bottom.to_string(),"-ProbeNonce",&nonce,"-SourceViewIdentity",&expected.to_string(),"-Proof0",&proof.0.to_string(),"-Proof1",&proof.1.to_string(),"-Proof2",&proof.2.to_string(),"-Mode",mode])
        .arg("-RequestReceipt").arg(&request_file).arg("-Out").arg(out)
        .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    if !pump_until(Duration::from_secs(25),||child.try_wait().is_ok_and(|s|s.is_some())){
        // Kill only our exact retained child handle. The helper's native-modal watchdog retains
        // its exact test-process handle; it never kills a browser or a name/PID-only target.
        let _=child.kill();let _=child.wait();panic!("real App input helper timed out; no acceptance");
    }
    let output=child.wait_with_output().unwrap();
    println!("APP_CONTEXT_INPUT mode={mode} exit={:?} stdout={} stderr={}",output.status.code(),String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(),"native App input helper failed; no acceptance");assert!(error.borrow().is_none(),"{:?}",error.borrow());
    current(h,proof);
    let request=observed.borrow().iter().find_map(|event|match event {MenuProbeEvent::BeforeShow {request,..}=>Some(*request),_=>None}).expect("real before-show request receipt missing");
    assert!(pump_until(Duration::from_secs(10),|| {
        if !(h.document_fence)() || (h.proof)()!=Some(proof) {return true;}
        observed.borrow().iter().any(|event|matches!(event,MenuProbeEvent::Completed {request:completed,..} if *completed==request))
    }),"actual asynchronous dispatch/deferral completion did not settle; helper exit is not completion");
    current(h,proof);
    let events=observed.borrow();assert_eq!(events.len(),2,"one real request and one settled real completion are mandatory");
    let MenuProbeEvent::BeforeShow{request,fixture_command,..}=events[0] else{panic!("before-show receipt missing")};
    let MenuProbeEvent::Completed{request:completed,selected,setter_calls,setter_result,complete_calls,complete_error,refused,cleanup_ok}=events[1] else{panic!("actual completion receipt missing")};
    assert_eq!(request,completed);assert_eq!(complete_calls,1);assert_eq!(complete_error,None);assert!(!refused);assert!(cleanup_ok,"real HMENU close must succeed explicitly, not via best-effort Drop");
    if mode=="Cancel" {assert_eq!(selected,None);assert_eq!(setter_calls,0);assert_eq!(setter_result,None)}
    else {assert_eq!(selected,Some(fixture_command));assert_eq!(setter_calls,1);assert_eq!(setter_result,Some(Ok(())),"one actual successful native setter is mandatory, not a later inferred effect")}
    drop(events);
    let after=script(&h.view,"Number(document.documentElement.getAttribute('data-app-context-trusted')||0)");
    assert_eq!(after,Value::Number((number(&source,"trusted")+1).to_string()),"physical right-click did not reach exact COM-owned source control");
    let native=json::parse(&std::fs::read_to_string(out.join("app-context-menu.json")).unwrap()).unwrap();
    assert_eq!(text(&native,"status"),if mode=="Cancel"{"observed-real-App-owned-popup-and-real-cancellation"}else{"observed-real-App-owned-popup-and-real-selection"});
    if mode=="Select"{assert_eq!(number(&native,"selected_command_id"),fixture_command as i64)}
    fixture_command
}

fn sequence(app: &AppRef,root: &Path,page: &url::Url,bowser_id: &str,folder: &Path,reads:&LoopbackReads) {
    let source_tab=App::active_tab_id(app);let mut primary=None;
    let source_ready=pump_until(Duration::from_secs(60),||{primary=handles(app,source_tab);primary.as_ref().is_some_and(|h|(h.proof)().is_some())});
    if !source_ready {
        // Original result remains a refusal. These are Rust App state classifications,
        // not another script/readiness attempt or a substitute for the native gate proof.
        let ids=App::tab_ids(app);let active=(!ids.is_empty()).then(||App::active_tab_id(app));
        let current_url=App::tab_committed_url(app,source_tab);
        let receipt=format!(r#"{{"scope":"initial-source-readiness-failure-not-authority","source_tab":"{}","source_present":{},"tab_count":{},"active_matches_source":{},"committed_url_matches_fixture":{},"last_handle_available":{},"loopback_accepted":{},"loopback_complete_gets":{},"fixture_response_attempts":{},"pump_calls":{}}}"#,
            source_tab.0,ids.contains(&source_tab),ids.len(),active==Some(source_tab),current_url.as_ref()==Some(page),primary.is_some(),reads.accepted.load(Ordering::Relaxed),reads.parsed.load(Ordering::Relaxed),reads.served.load(Ordering::Relaxed),super::native_probe_pump::diagnostic_json());
        if receipt.len()<=8192 {let _=std::fs::write(std::env::current_dir().unwrap().join("store-real-app-context-menu/initial-source-readiness.json"),receipt);}
    }
    assert!(source_ready,"real App controller/successful document did not settle");
    let primary=primary.unwrap();let primary_proof=(primary.proof)().unwrap();ordinary_profile(&primary.view);
    let before_doc=document(&primary.view,page);current(&primary,primary_proof);isolated_environment(app,&primary);
    assert!(pump_until(Duration::from_secs(30),||std::fs::read_to_string(root.join("extension_ids.txt")).is_ok_and(|s|s.lines().any(|line|line==format!("{bowser_id}\t{FIXTURE_ID}")))),"real native fixture startup did not persist its exact ID");
    authority(root,bowser_id);
    let other_tab=App::new_tab(app);App::submit(app,page.as_str());let mut negative=None;
    assert!(pump_until(Duration::from_secs(30),||{negative=handles(app,other_tab);negative.as_ref().is_some_and(|h|(h.proof)().is_some())}));
    let negative=negative.unwrap();let negative_doc=document(&negative.view,page);
    assert_ne!(primary.view.as_raw(),negative.view.as_raw(),"negative must be a different COM-owned page");
    assert!(App::select_tab(app,source_tab));current(&primary,primary_proof);
    let popup=open_receipt_view(app,bowser_id,folder);
    let mut ready=None;
    assert!(pump_until(Duration::from_secs(10),||{let r=worker(&popup,"native-command-state");let ok=field(&r,"creation").is_some_and(|v|field(v,"complete")==Some(&Value::Bool(true)));ready=Some(r);ok}));
    let ready=ready.unwrap();let worker_id=text(&ready,"workerInstance").to_owned();let creation=field(&ready,"creation").unwrap().clone();assert_bool(&creation,"ok",true);assert_eq!(field(&creation,"error"),Some(&Value::Null));
    let before=worker(&popup,"native-command-preflight");assert_eq!(text(&before,"workerInstance"),worker_id);assert!(array(&before,"events").is_empty());
    let denials=array(&before,"preflight");assert!(denials.len()>=2,"retain at least two native returned-ID denials; duplicate-view identity is not inferred from this count");
    for denial in denials {assert_bool(denial,"allowed",false);assert_bool(denial,"permissionDenied",true);}
    for view in [&primary.view,&negative.view] {for role in ["primary","negative"]{assert_eq!(marker(view,role),Value::Null);}}
    let out=std::env::current_dir().unwrap().join("store-real-app-context-menu");
    native_input(app,&primary,source_tab,primary_proof,"Cancel",&out.join("cancel"));
    // Fixed quiet window is explicitly bounded evidence, not proof of indefinite non-delivery.
    pump_until(Duration::from_secs(2),||false);
    let cancel=worker(&popup,"native-command-preflight");assert_eq!(text(&cancel,"workerInstance"),worker_id);assert_eq!(field(&cancel,"creation"),Some(&creation));assert!(array(&cancel,"events").is_empty());
    for denial in array(&cancel,"preflight"){assert_bool(denial,"allowed",false);assert_bool(denial,"permissionDenied",true);}
    for view in [&primary.view,&negative.view]{for role in ["primary","negative"]{assert_eq!(marker(view,role),Value::Null);}}
    assert_eq!(document(&primary.view,page),before_doc);assert_eq!(document(&negative.view,page),negative_doc);current(&primary,primary_proof);authority(root,bowser_id);
    let command=native_input(app,&primary,source_tab,primary_proof,"Select",&out.join("select"));
    let mut settled=None;
    assert!(pump_until(Duration::from_secs(10),||{let receipt=worker(&popup,"native-command-state");let events=array(&receipt,"events");let done=events.len()==1&&field(&events[0],"complete")==Some(&Value::Bool(true));settled=Some(receipt);done}),"genuine native onClicked/injection receipt absent, no functionality acceptance");
    let receipt=settled.unwrap();
    // Retain the actual settled native receipt even when a later grant/effect assertion fails.
    // This disposable profile contains only the controlled fixture and loopback test documents.
    std::fs::write(out.join("genuine-worker-receipt.txt"),format!("{receipt:?}\n")).unwrap();
    println!("APP_CONTEXT_MENU settled_native_worker_receipt={receipt:?}");
    assert_eq!(text(&receipt,"workerInstance"),worker_id);assert_eq!(field(&receipt,"creation"),Some(&creation));
    let event=&array(&receipt,"events")[0];assert_eq!(text(event,"eventKind"),"contextMenus.onClicked");assert!(field(event,"error").is_none());
    let info=field(event,"info").unwrap();assert_eq!(text(info,"menuItemId"),"bowser-native-context-probe");assert_bool(info,"editable",true);assert_eq!(number(info,"frameId"),0);
    let native_tab=field(event,"nativeTab").unwrap();let actual_tab_id=number(native_tab,"id");assert!(actual_tab_id>=0);
    assert_eq!(number(field(event,"currentNativeTab").expect("settled current real event tab receipt"),"id"),actual_tab_id);
    assert!(denials.iter().any(|d|number(d,"id")==actual_tab_id),"event-supplied real native ID must have been denied before the gesture");
    assert!(array(&cancel,"preflight").iter().any(|d|number(d,"id")==actual_tab_id
        && field(d,"allowed")==Some(&Value::Bool(false)) && field(d,"permissionDenied")==Some(&Value::Bool(true))),
        "fresh post-cancel denial must match the later genuine event-supplied owned native ID");
    let injection=field(event,"injection").unwrap();assert_bool(injection,"attempted",true);assert_bool(injection,"allowed",true);assert_eq!(number(injection,"id"),actual_tab_id);
    let token=text(event,"primaryToken");assert!(token.starts_with("native-context-primary-"));
    let results=array(injection,"nativeResults");assert_eq!(results.len(),1);assert_eq!(number(&results[0],"frameId"),0);assert_eq!(text(&results[0],"result"),token);
    // Native execution result + exact source COM marker bind event tab identity to this page.
    // URLs, tab order, labels and guessed native IDs are never used to choose an injection target.
    assert_eq!(marker(&primary.view,"primary"),Value::String(token.into()));assert_eq!(marker(&primary.view,"negative"),Value::Null);
    for role in ["primary","negative"]{assert_eq!(marker(&negative.view,role),Value::Null);}
    let other=array(event,"otherTabAttempts");assert!(!other.is_empty());for attempt in other {assert_bool(attempt,"allowed",false);assert_bool(attempt,"permissionDenied",true);assert_ne!(number(attempt,"id"),actual_tab_id);}
    assert_eq!(document(&primary.view,page),before_doc);assert_eq!(document(&negative.view,page),negative_doc);current(&primary,primary_proof);authority(root,bowser_id);
    std::fs::write(out.join("app-context-menu-summary.json"),format!(r#"{{"scope":"real-App-run-BowserNativeWindow-warm-worker-single-consented-fixture","cancel":"observed-no-event-no-grant-2000ms-window","extension_selection":"observed-real-native-onClicked-and-exact-source-COM-marker","selected_engine_command":{command},"copy_paste":"not-run","host_actions":"not-run","no_activeTab_cross_origin_matrix":"not-run","multi_profile":"not-run","worker_restart":"not-run","in_modal_navigation_close_shutdown_races":"not-run","store_package_compatibility":"not-claimed"}}"#)).unwrap();
    println!("APP_CONTEXT_MENU real_App_run=true actual_BowserNativeWindow=true cancellation_no_grant=true real_current_engine_selection=true real_worker_native_tab_and_exact_COM_marker=true scope=warm-worker-single-consented-fixture remaining_Copy_Paste_host_permissions_profiles_races=NOT_RUN evidence={}",out.display());
}

#[test]
#[ignore="requires explicitly consented disposable fixture, real App::run, interactive Windows and physical native-menu input"]
fn live_probe_real_app_context_menu() {
    let args=std::env::args().skip(1).collect::<Vec<_>>();
    assert_eq!(args.iter().map(String::as_str).collect::<Vec<_>>(),vec![TEST_NAME,"--exact","--ignored","--nocapture","--test-threads=1"],"must run alone: failure watchdog may terminate ONLY this exact retained single-test executable");
    assert_eq!(std::env::var("BOWSER_APP_MENU_PROBE_CONSENT").as_deref(),Ok(FIXTURE_CONSENT),"explicit consent to the exact retained fixture is required");
    assert!(crate::profile::data_root_override().is_none(),"never reuse an initialized process profile");
    assert!(std::env::var_os("BOWSER_DATA_DIR").is_none(),"do not inherit another profile override");
    let stamp=SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let root=std::env::temp_dir().join(format!("bowser-real-app-menu-{}-{stamp}",std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let evidence=std::env::current_dir().unwrap().join("store-real-app-context-menu");
    assert!(!evidence.exists(),"do not reuse a prior diagnostic run");std::fs::create_dir(&evidence).unwrap();
    let server=Loopback::start();
    let options=LaunchOptions::parse_from(vec!["--dev".into(),"--data-dir".into(),root.to_string_lossy().into_owned(),"--open-url".into(),server.url.to_string()],&Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/fixtures")).unwrap();
    assert!(options.dev&&!options.guest&&!options.agent&&options.benchmark_log.is_none());assert_eq!(options.data_dir.as_deref(),Some(root.as_path()));assert!(options.user_data_dir.is_none());
    // App::run does not do main.rs's process setup: mirror its data-root/dev/COM steps here.
    // Never override WEBVIEW2_* or touch updater/accounts/real installed Bowser profiles.
    crate::profile::set_data_root(root.clone()).unwrap();crate::profile::set_dev_launch(true);
    let record=extensions::install(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/extensions/probe-custom-context-menu"),&root.join("Extensions"),&root.join("extensions.txt")).unwrap();
    let folder=extensions::folder_for(&root.join("Extensions"),&record.id).unwrap();
    DEREGISTRATIONS.with(|events|events.borrow_mut().clear());install_observer(Rc::new(|_|{}));
    let outcome=Rc::new(RefCell::new(None));let sink=outcome.clone();let destination=root.clone();let page=server.url.clone();let reads=server.reads.clone();
    let started=Instant::now();let timed_out=Rc::new(Cell::new(false));let expired=timed_out.clone();
    crate::win32::set_app_menu_probe_hooks(Box::new(move |app|{
        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||sequence(&app,&destination,&page,&record.id,&folder,&reads)));
        *sink.borrow_mut()=Some(result.map_err(|_|"real App native menu sequence failed; inspect native helper diagnostics".to_owned()));
        let owner=app.borrow().hwnd;unsafe{let _=PostMessageW(Some(owner),WM_CLOSE,WPARAM(0),LPARAM(0));}
    }),Box::new(move |app|{
        if started.elapsed()>Duration::from_secs(180)&&!expired.replace(true){let owner=app.borrow().hwnd;unsafe{let _=PostMessageW(Some(owner),WM_CLOSE,WPARAM(0),LPARAM(0));}}
    }));
    unsafe{CoInitializeEx(None,COINIT_APARTMENTTHREADED).ok().unwrap()};
    let run=App::run(options,Rc::new(PerfLog::new(None).unwrap()));
    unsafe{CoUninitialize()};
    assert!(run.is_ok(),"actual App::run failed: {run:?}");assert!(!timed_out.get(),"test timer expired; no acceptance");
    let result=outcome.borrow_mut().take().expect("real App ready hook never ran");result.unwrap();
    let removed=DEREGISTRATIONS.with(|events|events.borrow().clone());
    assert!(removed.len()>=2,"actual source and negative page handler deregistration receipts are required");
    assert!(removed.iter().all(Option::is_none),"real native handler deregistration failed: {removed:?}");
    std::fs::write(std::env::current_dir().unwrap().join("store-real-app-context-menu/native-handler-teardown.json"),format!(r#"{{"status":"observed-native-removal-success","native_handler_receipts":{}}}"#,removed.len())).unwrap();
    println!("APP_CONTEXT_MENU actual_native_handler_teardown=true disposable_profile_retained_for_evidence={}",root.display());
}
