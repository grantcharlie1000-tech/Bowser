//! Exact ignored single-process host regression. Pending Windows compile/native execution.
use std::{cell::{Cell,RefCell},io::{Read,Write},net::TcpListener,path::Path,process::{Command,Stdio},rc::Rc,
    sync::{Arc,Mutex,atomic::{AtomicBool,Ordering}},time::{Duration,Instant,SystemTime,UNIX_EPOCH}};
use webview2_com::{CoTaskMemPWSTR,ExecuteScriptCompletedHandler,Microsoft::Web::WebView2::Win32::{ICoreWebView2,ICoreWebView2_13,ICoreWebView2Environment7,COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC}};
use windows::{core::{Interface,PWSTR},Win32::{Foundation::{HWND,RECT,WPARAM,LPARAM},System::Com::{CoInitializeEx,CoUninitialize,COINIT_APARTMENTTHREADED},UI::WindowsAndMessaging::{GetClassNameW,GetClientRect,IsWindow,PostMessageW,WM_CLOSE}}};
use crate::{app::{App,AppRef},extensions::{self,json::{self,Value}},performance::{LaunchOptions,PerfLog},tabs::TabId};
use super::{HostProbeEvent,HostRole,NodeWitness,set_observer};
use super::super::app_context_menu_probe::{NativeContextProbeHandles,MenuProbeEvent};
const TEST_NAME:&str="engine::app_host_menu_probe::live::live_probe_real_app_host_context_menu";
const CONSENT:&str="disposable-app-host-menu-synthetic-clipboard";
fn quote(s:&str)->String {crate::chrome_host::protocol::json_string(s)}
fn pump_until(timeout: Duration, ready: impl FnMut() -> bool) -> bool {
    super::super::native_probe_pump::until(timeout,ready)
}

fn field<'a>(v: &'a Value,k: &str)->Option<&'a Value>{let Value::Object(o)=v else{return None};json::get(o,k)}
fn text<'a>(v: &'a Value,k: &str)->&'a str{let Some(Value::String(s))=field(v,k) else{panic!("missing string {k}")};s}
fn number(v: &Value,k: &str)->i64{let Some(Value::Number(s))=field(v,k) else{panic!("missing number {k}")};s.parse().unwrap()}
fn array<'a>(v: &'a Value,k: &str)->&'a [Value]{let Some(Value::Array(a))=field(v,k) else{panic!("missing array {k}")};a}
fn assert_bool(v: &Value,k: &str,b: bool){assert_eq!(field(v,k),Some(&Value::Bool(b)),"{k}: {v:?}");}

/// No App/engine borrow survives the trait getter. Its implementation must be clone-only.
fn handles(app: &AppRef,tab: TabId)->Option<NativeContextProbeHandles>{
    let app=app.try_borrow().ok()?;
    app.engine.as_ref()?.probe_owned_host_context_handles(tab)
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

fn current(h: &NativeContextProbeHandles,proof: (u64,u64,u64)) {
    assert!((h.document_fence)(),"source COM/document fence refused");assert_eq!((h.proof)(),Some(proof));
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

#[derive(Default)]
struct Receipts {paste:usize,note:usize,destination:usize}
// TCP reads may end anywhere in a request. Keep the existing size and one-second budget,
// but require complete headers before parsing the nonce or issuing an acknowledgment.
fn loopback_request_target(socket:&mut std::net::TcpStream)->Option<String> {
    // Winsock accept inherits the listener's nonblocking mode. Give this one accepted
    // connection a bounded blocking read; the listener itself must stay nonblocking.
    socket.set_nonblocking(false).ok()?;
    let deadline=Instant::now()+Duration::from_secs(1);
    let mut bytes=[0;8192];let mut used=0;
    loop {
        let remaining=deadline.checked_duration_since(Instant::now())?;
        socket.set_read_timeout(Some(remaining)).ok()?;
        let count=socket.read(&mut bytes[used..]).ok()?;
        if count==0 {return None;}
        used+=count;
        if used>=bytes.len()||Instant::now()>=deadline {return None;}
        if bytes[..used].windows(4).any(|part|part==b"\r\n\r\n") {break;}
    }
    std::str::from_utf8(&bytes[..used]).ok()?.lines().next()?.strip_prefix("GET ")?
        .split_whitespace().next().map(str::to_owned)
}
struct Loopback {url:url::Url,nonce:String,stop:Arc<AtomicBool>,receipts:Arc<Mutex<Receipts>>,worker:Option<std::thread::JoinHandle<()>>}
impl Loopback {
    fn start(nonce:String)->Self {
        let listener=TcpListener::bind("127.0.0.1:0").unwrap();listener.set_nonblocking(true).unwrap();
        let url=url::Url::parse(&format!("http://127.0.0.1:{}/source?nonce={nonce}",listener.local_addr().unwrap().port())).unwrap();
        let stop=Arc::new(AtomicBool::new(false));let stopping=stop.clone();let receipts=Arc::new(Mutex::new(Receipts::default()));let sink=receipts.clone();let token=nonce.clone();
        let source=include_str!("../../../benchmarks/fixtures/app-host-context-menu/index.html").replace("__NONCE__",&nonce);
        let worker=std::thread::spawn(move ||while !stopping.load(Ordering::Relaxed) {
            let Ok((mut socket,peer))=listener.accept() else{std::thread::sleep(Duration::from_millis(10));continue};
            if !peer.ip().is_loopback(){continue}
            let Some(target)=loopback_request_target(&mut socket) else{continue};
            let Ok(request)=url::Url::parse(&format!("http://127.0.0.1{target}")) else{continue};
            let query=request.query_pairs().collect::<std::collections::BTreeMap<_,_>>();
            let valid=query.get("nonce").is_some_and(|value|value.as_ref()==token);
            let (status,body)=if valid&&matches!(request.path(),"/source"|"/negative") {("200 OK",source.clone())}
                else if valid&&request.path()=="/destination" {sink.lock().unwrap().destination+=1;("200 OK",format!("<!doctype html><meta charset=utf-8><title>Disposable split destination</title><body data-destination-nonce={token}>known split destination</body>"))}
                else if valid&&request.path()=="/effect" {
                    let kind=query.get("kind").map(|v|v.as_ref()).unwrap_or("");let value=query.get("value").map(|v|v.as_ref()).unwrap_or("");
                    if ["paste","note"].contains(&kind)&&value==format!("{kind}-{token}") {let mut sink=sink.lock().unwrap();if kind=="paste"{sink.paste+=1}else{sink.note+=1};("200 OK",format!("{token}:{kind}"))}else{("400 Bad Request",String::new())}
                }else{("404 Not Found",String::new())};
            let _=write!(socket,"HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",body.len());
        });Self{url,nonce,stop,receipts,worker:Some(worker)}
    }
}
impl Drop for Loopback {fn drop(&mut self){self.stop.store(true,Ordering::Relaxed);if let Some(worker)=self.worker.take(){let _=worker.join();}}}

#[test]
fn loopback_effect_ack_waits_for_fragmented_request() {
    use std::net::TcpStream;
    let server=Loopback::start("fragmented-request".into());
    let mut socket=TcpStream::connect(("127.0.0.1",server.url.port().unwrap())).unwrap();
    socket.set_nodelay(true).unwrap();
    socket.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    socket.write_all(b"GET /effect?nonce=").unwrap();
    let mut first=[0];
    let pending=socket.read(&mut first).expect_err("incomplete request must not be answered or closed");
    assert!(matches!(pending.kind(),std::io::ErrorKind::WouldBlock|std::io::ErrorKind::TimedOut));
    socket.write_all(b"fragmented-request&kind=note&value=note-fragmented-request HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
    socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let mut response=String::new();socket.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"),"{response}");
    assert!(response.ends_with("fragmented-request:note"),"{response}");
    assert_eq!(server.receipts.lock().unwrap().note,1);
}

#[test]
fn loopback_rejects_oversize_headers_without_recording_an_effect() {
    use std::net::TcpStream;
    let server=Loopback::start("oversize".into());
    let mut socket=TcpStream::connect(("127.0.0.1",server.url.port().unwrap())).unwrap();
    socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let mut request=b"GET /effect?nonce=oversize&kind=note&value=note-oversize HTTP/1.1\r\nX: ".to_vec();
    request.resize(8192,b'x');socket.write_all(&request).unwrap();
    let mut response=Vec::new();let result=socket.read_to_end(&mut response);
    assert!(result.is_ok()||result.is_err_and(|error|error.kind()==std::io::ErrorKind::ConnectionReset));
    assert!(response.is_empty());assert_eq!(server.receipts.lock().unwrap().note,0);
}

fn atomic_receipt(path:&Path,text:&str)->std::io::Result<()> {
    assert!(text.len()<=1024*1024);let pending=path.with_extension("pending");
    let mut file=std::fs::OpenOptions::new().create_new(true).write(true).open(&pending)?;
    file.write_all(text.as_bytes())?;file.sync_all()?;drop(file);std::fs::rename(pending,path)
}
fn node_json(node:&NodeWitness)->String {
    let checked=node.checked.map(|v|v.to_string()).unwrap_or_else(||"null".into());
    let role=node.host_role.map(|role|quote(role.name())).unwrap_or_else(||"null".into());
    format!(r#"{{"path":[{}],"collection_identity":"{}","identity":"{}","kind":{},"command_id":{},"enabled":{},"checked":{},"current_name":{},"rendered_fingerprint":{},"rendered_units":{},"host_role":{},"children":[{}]}}"#,
        node.path.iter().map(usize::to_string).collect::<Vec<_>>().join(","),node.collection,node.identity,node.kind,node.command,node.enabled,checked,quote(&node.name),quote(&node.rendered_fingerprint),node.rendered_units,role,node.children.iter().map(node_json).collect::<Vec<_>>().join(","))
}
fn chosen(nodes:&[NodeWitness],mode:&str)->Option<i32> {
    fn visit<'a>(nodes:&'a [NodeWitness],mode:&str,parents:bool,found:&mut Vec<&'a NodeWitness>) {
        for node in nodes {let matches=match mode {
            "Copy"=>node.name=="copy"&&node.host_role.is_none(),"Paste"=>node.name=="paste"&&node.host_role.is_none(),
            "Note"=>node.host_role==Some(HostRole::InsertNoteChild),"Split"=>node.host_role==Some(HostRole::SplitLink),_=>false};
            if parents&&node.enabled&&matches&&(0..=2).contains(&node.kind){found.push(node)}visit(&node.children,mode,parents&&node.enabled,found);
        }
    }
    if mode=="Cancel"{return None}let mut found=Vec::new();visit(nodes,mode,true,&mut found);assert_eq!(found.len(),1,"one actual current native Name or inserted item role required");assert!(found[0].command>0);Some(found[0].command)
}
fn selected_path(nodes:&[NodeWitness],id:i32)->Option<Vec<usize>> {
    for node in nodes {if (0..=2).contains(&node.kind)&&node.command==id{return Some(node.path.clone())}
        if let Some(path)=selected_path(&node.children,id){return Some(path)}}None
}
struct ObserverScope;
impl Drop for ObserverScope {fn drop(&mut self){set_observer(None);}}

fn native_input(app:&AppRef,h:&NativeContextProbeHandles,tab:TabId,proof:(u64,u64,u64),mode:&str,control:&str,token:&str,out:&Path)->i32 {
    current(h,proof);assert_eq!(App::active_tab_id(app),tab);ordinary_profile(&h.view);isolated_environment(app,h);
    let owner=app.borrow().hwnd;let mut class=[0;64];let n=unsafe{GetClassNameW(owner,&mut class)};
    assert_eq!(String::from_utf16_lossy(&class[..n as usize]),"BowserNativeWindow");assert!(unsafe{IsWindow(Some(owner))}.as_bool());
    // MoveFocus is a normal focus preparation on the exact owned controller, not native-menu input.
    // The external helper is the sole caller of one initial SetForegroundWindow(owner).
    unsafe{h.controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC).unwrap()};
    let preparation=format!("(()=>{{const t=document.getElementById({});t.focus();{}return true}})()",quote(control),if mode=="Copy"{"t.select();"}else{""});
    assert_eq!(script(&h.view,&preparation),Value::Bool(true));
    let geometry=format!("(()=>{{const t=document.getElementById({}),r=t.getBoundingClientRect(),x=(r.left+r.right)/2,y=(r.top+r.bottom)/2;return {{left:r.left,top:r.top,right:r.right,bottom:r.bottom,width:innerWidth,height:innerHeight,ratio:devicePixelRatio,hit:document.elementFromPoint(x,y)===t,trusted:hostMenuSnapshot().trusted[t.id]}}}})()",quote(control));
    let source=script(&h.view,&geometry);assert_bool(&source,"hit",true);
    let mut bounds=RECT::default();let mut client=RECT::default();let mut parent=HWND::default();let mut zoom=0.0;let mut browser=0;
    unsafe{h.controller.Bounds(&mut bounds).unwrap();h.controller.ParentWindow(&mut parent).unwrap();h.controller.ZoomFactor(&mut zoom).unwrap();GetClientRect(owner,&mut client).unwrap();h.view.BrowserProcessId(&mut browser).unwrap()};
    assert_eq!(parent,owner);assert_eq!(zoom,1.0);assert!(browser>0);assert!(bounds.left>=0&&bounds.top>=0&&bounds.right<=client.right&&bounds.bottom<=client.bottom);
    let numeric=|key|{let Some(Value::Number(v))=field(&source,key)else{panic!("missing source geometry {key}")};v.parse::<f64>().unwrap()};
    let (width,height,ratio)=(numeric("width"),numeric("height"),numeric("ratio"));assert!(width>0.0&&height>0.0&&ratio>0.0);
    assert!(((bounds.right-bounds.left) as f64-width*ratio).abs()<=2.0&&((bounds.bottom-bounds.top) as f64-height*ratio).abs()<=2.0,"CSS/controller/device-pixel transform unverified");
    let x=|v:f64|bounds.left+(v*(bounds.right-bounds.left) as f64/width).round() as i32;let y=|v:f64|bounds.top+(v*(bounds.bottom-bounds.top) as f64/height).round() as i32;
    let (left,top,right,bottom)=(x(numeric("left")),y(numeric("top")),x(numeric("right")),y(numeric("bottom")));let (px,py)=((left+right)/2,(top+bottom)/2);
    std::fs::create_dir(out).unwrap();let request_file=out.join("current-request.json");let completed_file=out.join("completed-request.json");
    let nonce=format!("{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos());
    let events=Rc::new(RefCell::new(Vec::new()));let sink=events.clone();let expected_request=Rc::new(RefCell::new(None));let state=expected_request.clone();
    let errors=Rc::new(RefCell::new(Vec::<String>::new()));let failure=errors.clone();let path=request_file.clone();let completion=completed_file.clone();let expected=h.view.as_raw() as usize;let observed_nonce=nonce.clone();let mode_owned=mode.to_owned();
    set_observer(Some(Rc::new(move |event|{
        match &event {
            HostProbeEvent::BeforeShow{request,tab:actual,view,args,proof:actual_proof,nodes}=>{
                if state.borrow().is_some()||*request==0||*actual!=tab||*view!=expected||*args==0||*actual_proof!=proof {failure.borrow_mut().push("wrong/repeated exact coordinator request".into());return}
                let selected=chosen(nodes,&mode_owned);*state.borrow_mut()=Some((*request,selected));
                let path_json=selected.and_then(|id|selected_path(nodes,id)).map(|path|format!("[{}]",path.iter().map(usize::to_string).collect::<Vec<_>>().join(","))).unwrap_or_else(||"null".into());
                let text=format!(r#"{{"schema":"bowser-real-app-host-menu-v1","nonce":{},"phase":"before-show","owner":{},"tab":"{}","request":"{}","view_identity":"{}","args_identity":"{}","proof0":"{}","proof1":"{}","proof2":"{}","selected_path":{},"nodes":[{}]}}"#,quote(&observed_nonce),owner.0 as isize,tab.0,request,view,args,proof.0,proof.1,proof.2,path_json,nodes.iter().map(node_json).collect::<Vec<_>>().join(","));
                if let Err(error)=atomic_receipt(&path,&text){failure.borrow_mut().push(format!("full current request write failed: {error}"));}
            }
            HostProbeEvent::Lifecycle(MenuProbeEvent::Completed{request,selected,setter_calls,setter_result,complete_calls,complete_error,refused,cleanup_ok})=>{
                let expected=*state.borrow();let correct=expected==Some((*request,*selected))&&*complete_calls==1&&complete_error.is_none()&&!*refused&&*cleanup_ok&&
                    if mode_owned=="Cancel"{*setter_calls==0&&setter_result.is_none()}else{*setter_calls==1&&*setter_result==Some(Ok(()))};
                if !correct {failure.borrow_mut().push("real coordinator completed receipt did not match expected current selection".into());return}
                let selected=selected.map(|id|id.to_string()).unwrap_or_else(||"null".into());
                let receipt=format!(r#"{{"schema":"bowser-real-app-host-completed-v1","nonce":{},"request":"{}","selected":{},"mode":{},"setter_calls":{},"setter_ok":{},"complete_calls":1,"complete_ok":true,"cleanup_ok":true}}"#,quote(&observed_nonce),request,selected,quote(&mode_owned),setter_calls,mode_owned!="Cancel");
                if let Err(error)=atomic_receipt(&completion,&receipt){failure.borrow_mut().push(format!("settled coordinator receipt write failed: {error}"));}
            }
            _=>{}
        }
        sink.borrow_mut().push(event);
    })));
    let _observer=ObserverScope;current(h,proof);
    let mut child=Command::new("powershell.exe").args(["-NoProfile","-File"]).arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/scripts/probe-app-host-context-menu.ps1"))
        .args(["-OwnerPid",&std::process::id().to_string(),"-OwnerHandle",&(owner.0 as isize).to_string()]).arg("-OwnerExe").arg(std::env::current_exe().unwrap())
        .args(["-BrowserPid",&browser.to_string(),"-ClientX",&px.to_string(),"-ClientY",&py.to_string(),"-ViewportWidth",&client.right.to_string(),"-ViewportHeight",&client.bottom.to_string(),"-ControlLeft",&left.to_string(),"-ControlTop",&top.to_string(),"-ControlRight",&right.to_string(),"-ControlBottom",&bottom.to_string(),"-ProbeNonce",&nonce,"-SourceViewIdentity",&expected.to_string(),"-Proof0",&proof.0.to_string(),"-Proof1",&proof.1.to_string(),"-Proof2",&proof.2.to_string(),"-Mode",mode,"-SyntheticToken",token])
        .arg("-RequestReceipt").arg(&request_file).arg("-CompletedReceipt").arg(&completed_file).arg("-Out").arg(out).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    // Never kill the external clipboard owner from Rust. Its bounded failure path restores only
    // its last owned sequence before its retained-handle termination of this single test process.
    assert!(pump_until(Duration::from_secs(40),||child.try_wait().is_ok_and(|status|status.is_some())),"external native/clipboard guard did not settle; no acceptance (guard deliberately not killed)");
    let output=child.wait_with_output().unwrap();assert!(output.status.success(),"native host helper failed: {}",String::from_utf8_lossy(&output.stderr));assert!(errors.borrow().is_empty(),"{:?}",errors.borrow());
    let (request,selected)=expected_request.borrow().expect("actual full request receipt absent");
    assert!(pump_until(Duration::from_secs(10),||events.borrow().iter().any(|event|matches!(event,HostProbeEvent::Lifecycle(MenuProbeEvent::Completed{request:done,..}) if *done==request))),"helper exit is not production completion");
    let events=events.borrow();assert_eq!(events.len(),2,"one full BeforeShow and one matching real Completed required");
    let HostProbeEvent::Lifecycle(MenuProbeEvent::Completed{request:done,selected:actual,setter_calls,setter_result,complete_calls,complete_error,refused,cleanup_ok})=events[1]else{panic!("real completion absent")};
    assert_eq!(request,done);assert_eq!(selected,actual);assert_eq!(complete_calls,1);assert_eq!(complete_error,None);assert!(!refused);assert!(cleanup_ok);
    if mode=="Cancel"{assert_eq!(setter_calls,0);assert_eq!(setter_result,None)}else{assert_eq!(setter_calls,1);assert_eq!(setter_result,Some(Ok(())))}drop(events);
    current(h,proof);
    let after=script(&h.view,&format!("hostMenuSnapshot().trusted[{}]",quote(control)));assert_eq!(after,Value::Number((number(&source,"trusted")+1).to_string()),"trusted right-click did not reach exact COM source control");
    let report=json::parse(&std::fs::read_to_string(out.join("app-host-context-menu.json")).unwrap()).unwrap();
    assert_eq!(text(&report,"status"),if mode=="Cancel"{"observed-actual-App-native-cancellation"}else{"observed-actual-App-native-selection"});
    if mode!="Cancel"{assert_eq!(number(&report,"selected_command_id"),selected.unwrap() as i64)}
    if matches!(mode,"Copy"|"Paste"){assert_eq!(text(&report,"clipboard"),"restored-empty");assert!(array(&report,"owned_clipboard_sequences").len()>=3)}
    selected.unwrap_or(0)
}

fn snapshot(view:&ICoreWebView2)->Value {script(view,"hostMenuSnapshot()")}
fn unchanged_document(now:&Value,before:&Value) {
    for key in ["nonce","url","timeOrigin","ready","copy"]{assert_eq!(field(now,key),field(before,key),"source document identity/content changed: {key}")}
}
fn storage(root:&Path)->Vec<(String,Option<Vec<u8>>)> {
    ["notes.txt","notes-meta.txt","extensions.txt","extension_ids.txt"].iter().map(|name|((*name).into(),match std::fs::read(root.join(name)) { Ok(bytes)=>Some(bytes), Err(error) if error.kind()==std::io::ErrorKind::NotFound=>None, Err(error)=>panic!("storage snapshot read failed: {error}") })).collect()
}
fn effects(server:&Loopback,paste:usize,note:usize,destination:usize){let receipts=server.receipts.lock().unwrap();assert_eq!((receipts.paste,receipts.note,receipts.destination),(paste,note,destination));}
fn controller_bounds(h:&NativeContextProbeHandles,owner:HWND)->RECT {
    let mut bounds=RECT::default();let mut parent=HWND::default();let mut visible=windows::core::BOOL::default();
    unsafe{h.controller.Bounds(&mut bounds).unwrap();h.controller.ParentWindow(&mut parent).unwrap();h.controller.IsVisible(&mut visible).unwrap()};assert_eq!(parent,owner);assert!(visible.as_bool());assert!(bounds.right>bounds.left&&bounds.bottom>bounds.top);bounds
}
fn sequence(app:&AppRef,root:&Path,server:&Loopback,out:&Path) {
    let source_tab=App::active_tab_id(app);let mut source=None;
    assert!(pump_until(Duration::from_secs(60),||{source=handles(app,source_tab);source.as_ref().is_some_and(|h|(h.proof)().is_some())}),"actual source App controller/document did not settle");
    let source=source.unwrap();let proof=(source.proof)().unwrap();ordinary_profile(&source.view);isolated_environment(app,&source);
    let before=snapshot(&source.view);assert_eq!(text(&before,"url"),server.url.as_str());assert_eq!(text(&before,"nonce"),server.nonce);assert_eq!(text(&before,"ready"),"complete");
    let negative_tab=App::new_tab(app);let negative_url=server.url.join(&format!("/negative?nonce={}",server.nonce)).unwrap();App::submit(app,negative_url.as_str());let mut negative=None;
    assert!(pump_until(Duration::from_secs(30),||{negative=handles(app,negative_tab);negative.as_ref().is_some_and(|h|(h.proof)().is_some())}));
    let negative=negative.unwrap();let negative_proof=(negative.proof)().unwrap();let negative_before=snapshot(&negative.view);assert_eq!(text(&negative_before,"url"),negative_url.as_str());assert_eq!(text(&negative_before,"ready"),"complete");assert_ne!(source.view.as_raw(),negative.view.as_raw());
    assert!(App::select_tab(app,source_tab));current(&source,proof);current(&negative,negative_proof);
    assert!(App::split_view(app).is_none());let tabs_before=App::tab_ids(app);let persistent_before=storage(root);let token=&server.nonce;
    effects(server,0,0,0);
    native_input(app,&source,source_tab,proof,"Cancel","paste",token,&out.join("cancel"));
    // Bounded quiet evidence only, not an assertion of indefinite future non-delivery.
    pump_until(Duration::from_secs(2),||false);
    let cancel=snapshot(&source.view);unchanged_document(&cancel,&before);assert_eq!(text(&cancel,"paste"),"");assert_eq!(text(&cancel,"note"),"");assert!(array(field(&cancel,"effects").unwrap(),"paste").is_empty());assert!(array(field(&cancel,"effects").unwrap(),"note").is_empty());
    assert_eq!(snapshot(&negative.view),negative_before);assert_eq!(App::tab_ids(app),tabs_before);assert!(App::split_view(app).is_none());assert_eq!(storage(root),persistent_before);effects(server,0,0,0);current(&source,proof);current(&negative,negative_proof);
    native_input(app,&source,source_tab,proof,"Copy","copy",token,&out.join("copy"));
    let copied=snapshot(&source.view);unchanged_document(&copied,&before);assert_eq!(text(&copied,"paste"),"");assert_eq!(text(&copied,"note"),"");assert_eq!(field(&copied,"effects"),field(&cancel,"effects"));assert_eq!(snapshot(&negative.view),negative_before);assert_eq!(App::tab_ids(app),tabs_before);assert_eq!(storage(root),persistent_before);effects(server,0,0,0);
    native_input(app,&source,source_tab,proof,"Paste","paste",token,&out.join("paste"));
    let mut pasted=None;assert!(pump_until(Duration::from_secs(10),||{let value=snapshot(&source.view);let ready=field(&value,"pasteAck")==Some(&Value::String(token.clone()));pasted=Some(value);ready}),"native Paste source/server nonce acknowledgment absent");
    let pasted=pasted.unwrap();unchanged_document(&pasted,&before);assert_eq!(text(&pasted,"paste"),format!("paste-{token}"));assert_eq!(text(&pasted,"note"),"");let paste_events=array(field(&pasted,"effects").unwrap(),"paste");assert_eq!(paste_events.len(),1);assert_eq!(text(&paste_events[0],"value"),format!("paste-{token}"));assert_bool(&paste_events[0],"trusted",true);assert!(array(field(&pasted,"effects").unwrap(),"note").is_empty());effects(server,1,0,0);assert_eq!(snapshot(&negative.view),negative_before);assert_eq!(storage(root),persistent_before);assert_eq!(App::tab_ids(app),tabs_before);
    native_input(app,&source,source_tab,proof,"Note","note",token,&out.join("note"));
    let mut noted=None;assert!(pump_until(Duration::from_secs(10),||{let value=snapshot(&source.view);let ready=field(&value,"noteAck")==Some(&Value::String(token.clone()));noted=Some(value);ready}),"actual host Insert note source/server nonce acknowledgment absent");
    let noted=noted.unwrap();unchanged_document(&noted,&before);assert_eq!(text(&noted,"paste"),format!("paste-{token}"));assert_eq!(text(&noted,"note"),format!("note-{token}"));let note_events=array(field(&noted,"effects").unwrap(),"note");assert_eq!(note_events.len(),1);assert_eq!(text(&note_events[0],"value"),format!("note-{token}"));assert_eq!(array(field(&noted,"effects").unwrap(),"paste"),paste_events);assert_eq!(snapshot(&negative.view),negative_before);assert_eq!(storage(root),persistent_before);assert_eq!(App::tab_ids(app),tabs_before);effects(server,1,1,0);current(&source,proof);current(&negative,negative_proof);
    // Split is deliberately LAST: require the actual deferred window route to create exactly one
    // AppTab and a live source/destination split, not merely a requested URL or callback receipt.
    let owner=app.borrow().hwnd;let before_bounds=controller_bounds(&source,owner);
    native_input(app,&source,source_tab,proof,"Split","split",token,&out.join("split"));
    let mut destination=None;assert!(pump_until(Duration::from_secs(30),||{
        let ids=App::tab_ids(app);if ids.len()!=tabs_before.len()+1{return false}let added=ids.into_iter().filter(|tab|!tabs_before.contains(tab)).collect::<Vec<_>>();if added.len()!=1{return false}
        let tab=added[0];let Some(h)=handles(app,tab)else{return false};if (h.proof)().is_none(){return false}destination=Some((tab,h));true
    }),"actual split destination AppTab/controller/successful DocumentGate did not settle");
    let (destination_tab,destination)=destination.unwrap();let split=App::split_view(app).expect("actual App split_view absent");assert!(split.is_pair());assert_eq!(split.tabs().collect::<std::collections::HashSet<_>>(),[source_tab,destination_tab].into_iter().collect());
    let destination_url=server.url.join(&format!("/destination?nonce={token}")).unwrap();assert_eq!(App::tab_committed_url(app,destination_tab),Some(destination_url.clone()));
    let committed={let state=app.borrow();state.engine.as_ref().unwrap().successful_document_query(destination_tab)}.expect("actual successful destination document query absent");assert_eq!(committed(),Some(destination_url.clone()));
    let receipt=script(&destination.view,"({url:location.href,ready:document.readyState,nonce:document.body.getAttribute('data-destination-nonce')})");assert_eq!(text(&receipt,"url"),destination_url.as_str());assert_eq!(text(&receipt,"ready"),"complete");assert_eq!(text(&receipt,"nonce"),token);
    let source_bounds=controller_bounds(&source,owner);let destination_bounds=controller_bounds(&destination,owner);let mut client=RECT::default();unsafe{GetClientRect(owner,&mut client).unwrap()};
    for bounds in [source_bounds,destination_bounds]{assert!(bounds.left>=0&&bounds.top>=0&&bounds.right<=client.right&&bounds.bottom<=client.bottom)}
    assert!(source_bounds.right<=destination_bounds.left||destination_bounds.right<=source_bounds.left||source_bounds.bottom<=destination_bounds.top||destination_bounds.bottom<=source_bounds.top,"both real split controller bounds must be visible and nonoverlapping");assert_ne!((before_bounds.left,before_bounds.top,before_bounds.right,before_bounds.bottom),(source_bounds.left,source_bounds.top,source_bounds.right,source_bounds.bottom));
    let split_source=snapshot(&source.view);unchanged_document(&split_source,&before);for key in ["paste","note","effects","pasteAck","noteAck"]{assert_eq!(field(&split_source,key),field(&noted,key));}
    assert_eq!(snapshot(&negative.view),negative_before);current(&source,proof);current(&negative,negative_proof);assert_eq!(storage(root),persistent_before);effects(server,1,1,1);
    std::fs::write(out.join("app-host-context-menu-summary.json"),r#"{"scope":"actual-App-run-native-Copy-Paste-real-host-InsertNote-SplitLink","cancel":"matching-zero-setter-completion-and-2000ms-quiet-no-effect","copy":"native-current-Name-ID-physical-selection-generated-clipboard-exact-owner-restored-empty","paste":"native-current-Name-ID-source-only-exact-token-loopback-nonce-ack","note":"actual-inserted-parent-child-COM-identities-source-only-body-loopback-nonce-ack-storage-unchanged","split":"actual-inserted-COM-identity-exact-one-new-AppTab-live-split-real-controller-bounds-successful-committed-loopback-destination-nonce","unknown-native-entries":"retained-complete-stable-no-empty-inventory-requirement","search-completed-transport":"NOT RUN: configured engines require HTTPS; no HTTPS relaxation","permission-race-matrix":"NOT RUN","signed-Bitwarden-workflow":"NOT RUN","shipping-feature":"DEFAULT OFF","store-compatibility":"NOT CLAIMED"}"#).unwrap();
    println!("APP_HOST_CONTEXT actual_App_run=true actual_BowserNativeWindow=true cancel_native_Copy_Paste_host_note_split=true unknown_native_inventory_retained=true search_completed_transport_PERMISSION_MATRIX_RACES_BITWARDEN=NOT_RUN evidence={}",out.display());
}

#[test]
#[ignore="requires explicit disposable synthetic clipboard consent and an interactive unlocked Windows runner; run alone"]
fn live_probe_real_app_host_context_menu() {
    let args=std::env::args().skip(1).collect::<Vec<_>>();assert_eq!(args.iter().map(String::as_str).collect::<Vec<_>>(),vec![TEST_NAME,"--exact","--ignored","--nocapture","--test-threads=1"],"only this exact single-test executable may be retained by the failure guard");
    assert_eq!(std::env::var("BOWSER_APP_HOST_MENU_PROBE_CONSENT").as_deref(),Ok(CONSENT));
    assert!(crate::profile::data_root_override().is_none());assert!(std::env::var_os("BOWSER_DATA_DIR").is_none());
    let stamp=SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();let token=format!("{}-{stamp}",std::process::id());let root=std::env::temp_dir().join(format!("bowser-real-app-host-menu-{token}"));std::fs::create_dir(&root).unwrap();
    let evidence=std::env::current_dir().unwrap().join("store-real-app-host-context-menu");std::fs::create_dir(&evidence).expect("do not reuse evidence from a previous run");
    let server=Loopback::start(token.clone());let profile=crate::profile::Profile::default_in(&root);
    crate::profile::set_data_root(root.clone()).unwrap();crate::profile::set_dev_launch(true);
    // Empty bounded BOWSER records/map only. Never demand an empty native runtime inventory,
    // remove an unknown native entry, or classify it as trusted. Production retains every entry.
    extensions::write_records(&root.join("extensions.txt"),&[]).unwrap();std::fs::write(root.join("extension_ids.txt"),b"").unwrap();
    crate::productivity::write_note_list(&profile.notes_file(),&[crate::productivity::Note{id:1,title:"Disposable host menu note".into(),body:format!("note-{token}")}]).unwrap();
    let options=LaunchOptions::parse_from(vec!["--dev".into(),"--data-dir".into(),root.to_string_lossy().into_owned(),"--open-url".into(),server.url.to_string()],&Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/fixtures")).unwrap();
    assert!(options.dev&&!options.guest&&!options.agent&&options.benchmark_log.is_none());assert_eq!(options.data_dir.as_deref(),Some(root.as_path()));assert!(options.user_data_dir.is_none());
    let outcome=Rc::new(RefCell::new(None));let sink=outcome.clone();let destination=root.clone();let output=evidence.clone();let started=Instant::now();let timed_out=Rc::new(Cell::new(false));let expired=timed_out.clone();
    let deregistrations=Rc::new(RefCell::new(Vec::<Option<i32>>::new()));let removal=deregistrations.clone();
    // Lifecycle observer is rearmed by native_input and left on only for final native removal.
    crate::win32::set_app_menu_probe_hooks(Box::new(move |app|{
        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||sequence(&app,&destination,&server,&output)));
        *sink.borrow_mut()=Some(result.map_err(|_|"actual App host regression failed; inspect per-mode native/clipboard diagnostics".to_owned()));
        set_observer(Some(Rc::new(move |event|if let HostProbeEvent::Lifecycle(MenuProbeEvent::Deregistered{native_error})=event {removal.borrow_mut().push(native_error)})));
        let owner=app.borrow().hwnd;unsafe{let _=PostMessageW(Some(owner),WM_CLOSE,WPARAM(0),LPARAM(0));}
    }),Box::new(move |app|if started.elapsed()>Duration::from_secs(240)&&!expired.replace(true){let owner=app.borrow().hwnd;unsafe{let _=PostMessageW(Some(owner),WM_CLOSE,WPARAM(0),LPARAM(0));}}));
    unsafe{CoInitializeEx(None,COINIT_APARTMENTTHREADED).ok().unwrap()};let run=App::run(options,Rc::new(PerfLog::new(None).unwrap()));unsafe{CoUninitialize()};set_observer(None);
    assert!(run.is_ok(),"actual App::run failed: {run:?}");assert!(!timed_out.get());outcome.borrow_mut().take().expect("real ready hook absent").unwrap();
    let removed=deregistrations.borrow();assert!(removed.len()>=3,"source, negative, and split destination native removals required");assert!(removed.iter().all(Option::is_none));
    std::fs::write(evidence.join("native-handler-teardown.json"),format!(r#"{{"status":"observed-native-removal-success","native_handler_receipts":{}}}"#,removed.len())).unwrap();
    println!("APP_HOST_CONTEXT actual_native_handler_teardown=true disposable_profile_retained_for_evidence={}",root.display());
}

#[test]
fn host_probe_selection_uses_native_name_or_inserted_role_not_command_range() {
    let node=|name:&str,id,role|NodeWitness{path:vec![0],collection:1,identity:2,kind:0,command:id,enabled:true,checked:None,name:name.into(),rendered_fingerprint:"811c9dc5".into(),rendered_units:0,host_role:role,children:Vec::new()};
    let copy=node("copy",23019,None);assert_eq!(chosen(&[copy],"Copy"),Some(23019));
    let note=node("not-a-note-label",7,Some(HostRole::InsertNoteChild));assert_eq!(chosen(&[note],"Note"),Some(7));
    let wrong=node("copy",19,Some(HostRole::SplitLink));assert!(std::panic::catch_unwind(||chosen(&[wrong],"Copy")).is_err());
    let unknown=node("runtime-unknown",555,None);let copy=node("copy",4,None);let nodes=vec![unknown,copy];
    assert_eq!(chosen(&nodes,"Copy"),Some(4));assert_eq!(nodes.len(),2,"complete unknown native entry must not be removed to choose Copy");
    assert_eq!(chosen(&nodes,"Cancel"),None);
}

// Independent exact-test process and disposable profile; original clipboard lane is unchanged.
mod note_split;
