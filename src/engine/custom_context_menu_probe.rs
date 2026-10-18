//! Isolated documented native presentation. No production registration, fake extension item or API dispatch.
use super::*;
use std::{cell::Cell,collections::HashSet};
use windows::Win32::UI::Shell::{SetWindowSubclass,RemoveWindowSubclass,DefSubclassProc};
use webview2_com::{ContextMenuRequestedEventHandler,Microsoft::Web::WebView2::Win32::*};
use windows::{core::{Interface,PCWSTR},Win32::{Foundation::{POINT,WPARAM,LPARAM},System::Threading::GetCurrentThreadId,UI::WindowsAndMessaging::*}};
const CONTINUE:u32=WM_APP+0x5c1;
const EXPECTED_LABEL:&str="Bowser native context permission probe";

struct NativeMenu(HMENU);
impl NativeMenu{fn close(&mut self)->bool{if self.0==HMENU::default(){return true}match unsafe{DestroyMenu(self.0)}{Ok(())=>{self.0=HMENU::default();true},Err(e)=>{println!("CUSTOM_CONTEXT menu_destroy_error={}",e.code().0);false}}}}
impl Drop for NativeMenu{fn drop(&mut self){let _=self.close();}}
struct Choice {id:i32,kind:i32,enabled:bool,checked:bool,item:ICoreWebView2ContextMenuItem}
struct Completion {deferral:Option<ICoreWebView2Deferral>,outcome:Rc<Cell<Option<i32>>>}
impl Completion {fn complete(&mut self){if let Some(deferral)=self.deferral.take(){self.outcome.set(Some(unsafe{deferral.Complete()}.err().map_or(0,|e|e.code().0)));}}}
impl Drop for Completion{fn drop(&mut self){self.complete();}}
struct Pending {args:ICoreWebView2ContextMenuRequestedEventArgs,_completion:Completion,menu:NativeMenu,choices:Vec<Choice>,point:POINT,fixture_id:i32}
fn same_item(choice:&Choice)->bool{
    let mut id=0;let mut kind=COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND(0);let mut enabled=windows::core::BOOL::default();let mut checked=windows::core::BOOL::default();
    let ok=unsafe{choice.item.CommandId(&mut id)}.is_ok()&&unsafe{choice.item.Kind(&mut kind)}.is_ok()&&unsafe{choice.item.IsEnabled(&mut enabled)}.is_ok();
    ok&&id==choice.id&&kind.0==choice.kind&&enabled.as_bool()==choice.enabled&&(!(1..=2).contains(&kind.0)||(unsafe{choice.item.IsChecked(&mut checked)}.is_ok()&&checked.as_bool()==choice.checked))
}
struct Registration{native:ICoreWebView2_11,token:i64,pending:Rc<RefCell<Option<Pending>>>,removed:Rc<Cell<Option<i32>>>}
impl Drop for Registration{fn drop(&mut self){let result=unsafe{self.native.remove_ContextMenuRequested(self.token)};self.removed.set(Some(result.as_ref().err().map_or(0,|e|e.code().0)));if let Err(e)=result{println!("CUSTOM_CONTEXT remove_handler_error={}",e.code().0)}let pending=self.pending.borrow_mut().take();drop(pending);}}
fn engine_menu(collection:&ICoreWebView2ContextMenuItemCollection,depth:usize,visited:&mut usize,seen:&mut HashSet<i32>,choices:&mut Vec<Choice>,fixture:&mut Vec<i32>)->windows::core::Result<NativeMenu>{
    let reject=||windows::core::Error::from_hresult(windows::core::HRESULT(0x80070057u32 as i32));
    if depth>3{return Err(reject())}let mut count=0;unsafe{collection.Count(&mut count)?};if count>64usize.saturating_sub(*visited) as u32{return Err(reject())}
    let menu=NativeMenu(unsafe{CreatePopupMenu()?});
    for index in 0..count{
        *visited+=1;let item=unsafe{collection.GetValueAtIndex(index)?};let mut kind=COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND(0);unsafe{item.Kind(&mut kind)?};
        if kind.0==3 {unsafe{AppendMenuW(menu.0,MF_SEPARATOR,0,PCWSTR::null())?};continue}
        if !(0..=4).contains(&kind.0){return Err(reject())}
        let mut enabled=windows::core::BOOL::default();unsafe{item.IsEnabled(&mut enabled)?};
        let mut label=windows::core::PWSTR::null();let r=unsafe{item.Label(&mut label)};let (code,label,truncated,redacted)=native_context_text(r,label,128);
        if code!=0||truncated||redacted||label.is_empty(){return Err(reject())}let wide:Vec<_>=label.encode_utf16().chain([0]).collect();
        let mut flags=MF_STRING;if !enabled.as_bool(){flags|=MF_GRAYED;}
        if kind.0==4 {
            let child=engine_menu(&unsafe{item.Children()?},depth+1,visited,seen,choices,fixture)?;
            unsafe{AppendMenuW(menu.0,flags|MF_POPUP,child.0.0 as usize,PCWSTR(wide.as_ptr()))?};std::mem::forget(child);continue
        }
        let mut id=0;unsafe{item.CommandId(&mut id)?};if id<=0||!seen.insert(id){return Err(reject())}
        let mut checked=windows::core::BOOL::default();if (1..=2).contains(&kind.0){unsafe{item.IsChecked(&mut checked)?};if checked.as_bool(){flags|=MF_CHECKED}}
        // Preserve check/radio kind as native presentation; no host callback substitutes.
        unsafe{AppendMenuW(menu.0,flags,id as usize,PCWSTR(wide.as_ptr()))?};
        if kind.0==2{let info=MENUITEMINFOW{cbSize:std::mem::size_of::<MENUITEMINFOW>() as u32,fMask:MIIM_FTYPE,fType:MFT_RADIOCHECK,..Default::default()};unsafe{SetMenuItemInfoW(menu.0,id as u32,false,&info)?};}
        let mut name=windows::core::PWSTR::null();let r=unsafe{item.Name(&mut name)};let (code,name,nt,nr)=native_context_text(r,name,64);if code!=0||nt||nr{return Err(reject())}
        if name=="extension"&&label==EXPECTED_LABEL&&enabled.as_bool()&&kind.0==0{fixture.push(id)}
        choices.push(Choice{id,kind:kind.0,enabled:enabled.as_bool(),checked:checked.as_bool(),item});
    }
    Ok(menu)
}
fn profile(view:&ICoreWebView2)->Option<String>{
    let profile=unsafe{view.cast::<ICoreWebView2_13>().ok()?.Profile().ok()?};let mut name=windows::core::PWSTR::null();let mut private=windows::core::BOOL::default();
    let result=unsafe{profile.ProfileName(&mut name)};let (code,name,truncated,redacted)=native_context_text(result,name,64);
    if code!=0||truncated||redacted||name.is_empty()||unsafe{profile.IsInPrivateModeEnabled(&mut private)}.is_err()||private.as_bool()||name==crate::private::PRIVATE_PROFILE||name==crate::chrome_host::host::CHROME_PROFILE{return None}Some(name)
}
thread_local!{static NEXT_OWNER_OBSERVER:Cell<usize>=const{Cell::new(0)};}
struct OwnerState{owner:HWND,pid:u32,thread:u32,destroyed:Cell<bool>}
impl OwnerState{fn current(&self)->bool{let mut actual=0;!self.destroyed.get()&&unsafe{IsWindow(Some(self.owner))}.as_bool()&&unsafe{GetWindowThreadProcessId(self.owner,Some(&mut actual))}==self.thread&&actual==self.pid}}
unsafe extern "system" fn owner_lifetime(owner:HWND,message:u32,w:WPARAM,l:LPARAM,id:usize,data:usize)->windows::Win32::Foundation::LRESULT{
    unsafe{let state=&*(data as *const OwnerState);if message==WM_NCDESTROY{state.destroyed.set(true);let _=RemoveWindowSubclass(owner,Some(owner_lifetime),id);}DefSubclassProc(owner,message,w,l)}
}
struct OwnerObserver{state:Rc<OwnerState>,id:usize,detached:Rc<Cell<Option<bool>>>}
impl OwnerObserver{
    fn new(owner:HWND,pid:u32,thread:u32,detached:Rc<Cell<Option<bool>>>)->Option<Self>{
        if thread!=unsafe{GetCurrentThreadId()}||pid!=std::process::id(){return None}
        let state=Rc::new(OwnerState{owner,pid,thread,destroyed:Cell::new(false)});if !state.current(){return None}
        let id=NEXT_OWNER_OBSERVER.with(|next|{let id=next.get().checked_add(1)?;next.set(id);Some(id)})?;
        if !unsafe{SetWindowSubclass(owner,Some(owner_lifetime),id,Rc::as_ptr(&state) as usize)}.as_bool(){return None}
        let observer=Self{state,id,detached};if !observer.state.current(){return None}Some(observer)
    }
}
impl Drop for OwnerObserver{fn drop(&mut self){
    let removed=unsafe{GetCurrentThreadId()}==self.state.thread&&(self.state.destroyed.get()||(self.state.current()&&unsafe{RemoveWindowSubclass(self.state.owner,Some(owner_lifetime),self.id)}.as_bool()));
    self.detached.set(Some(removed));if !removed{let _=Rc::into_raw(self.state.clone());println!("CUSTOM_CONTEXT owner_callback_lifetime_preserved_cleanup_failed=true");}
}}
fn owner_ready(owner:HWND,pid:u32,thread:u32)->bool{let mut actual=0;unsafe{IsWindow(Some(owner))}.as_bool()&&unsafe{GetWindowThreadProcessId(owner,Some(&mut actual))}==thread&&actual==pid&&unsafe{GetForegroundWindow()}==owner}
struct Dispatch {pending:Rc<RefCell<Option<Pending>>>,completion:Rc<Cell<Option<i32>>>,selected:Rc<Cell<Option<i32>>>,errors:Rc<Cell<u32>>,requests:Rc<Cell<u32>>}
fn run_input(session:&mut Session,tab:TabId,profile_path:&Path,mode:&str,out:&Path)->i32{
    let view=session.engine.probe_document_webview(tab).unwrap();let native=view.cast::<ICoreWebView2_11>().unwrap();let expected=view.as_raw() as usize;
    let expected_profile=profile(&view).expect("ordinary native profile required");
    let document=Rc::new(session.engine.probe_context_document_fence(tab).expect("successful selected document required"));
    let authority=Rc::new(session.engine.probe_context_authority_fence().expect("strict profile authority required"));
    assert!(document()&&authority());let snapshot=session.engine.probe_context_authority().unwrap();assert_eq!(snapshot.0.len(),1);assert!(snapshot.0[0].enabled);assert_eq!(snapshot.1.get(&snapshot.0[0].id).map(String::as_str),Some(KEY_FIXTURE_ID));
    let owner=session.host;let pid=std::process::id();let thread=unsafe{GetCurrentThreadId()};assert!(owner_ready(owner,pid,thread));
    let owner_detached=Rc::new(Cell::new(None));let owner_observer=OwnerObserver::new(owner,pid,thread,owner_detached.clone()).expect("actual owner lifetime observer unavailable");let owner_state=owner_observer.state.clone();
    let state=Dispatch{pending:Rc::new(RefCell::new(None)),completion:Rc::new(Cell::new(None)),selected:Rc::new(Cell::new(None)),errors:Rc::new(Cell::new(0)),requests:Rc::new(Cell::new(0))};
    let pending=state.pending.clone();let completion=state.completion.clone();let errors=state.errors.clone();let requests=state.requests.clone();let doc=document.clone();let auth=authority.clone();let receiving=view.clone();let ordinary=expected_profile.clone();let observed_owner=owner_state.clone();
    let handler=ContextMenuRequestedEventHandler::create(Box::new(move|sender,args|{
        requests.set(requests.get().saturating_add(1));
        let Some(args)=args else{errors.set(errors.get()+1);return Ok(())};
        // A second request never replaces the owning request or its one-shot deferral.
        unsafe{args.SetHandled(true)?};
        let guard=Completion{deferral:Some(unsafe{args.GetDeferral()?}),outcome:completion.clone()};
        if !observed_owner.current()||requests.get()!=1||pending.borrow().is_some()||!sender.as_ref().is_some_and(|v|v.as_raw() as usize==expected)||!doc()||!auth()||!owner_ready(owner,pid,thread){errors.set(errors.get()+1);return Ok(())}
        let result=(||->windows::core::Result<Pending>{
            let reject=||windows::core::Error::from_hresult(windows::core::HRESULT(0x80070057u32 as i32));
            let mut initial=0;unsafe{args.SelectedCommandId(&mut initial)?};if initial!=-1{return Err(reject())}
            let target=unsafe{args.ContextMenuTarget()?};let mut editable=windows::core::BOOL::default();unsafe{target.IsEditable(&mut editable)?};if !editable.as_bool(){return Err(reject())}
            let mut choices=Vec::new();let mut fixture=Vec::new();let mut visited=0;let menu=engine_menu(&unsafe{args.MenuItems()?},0,&mut visited,&mut HashSet::new(),&mut choices,&mut fixture)?;
            if fixture.len()!=1||profile(&receiving).as_ref()!=Some(&ordinary)||!doc()||!auth(){return Err(reject())}
            let mut point=POINT::default();unsafe{args.Location(&mut point)?};if !unsafe{ClientToScreen(owner,&mut point)}.as_bool()||!doc()||!auth(){return Err(reject())}
            println!("CUSTOM_CONTEXT request_native_view=true ordinary_profile=true editable=true complete_engine_items={visited} unique_fixture_id={} deferred_modal=true",fixture[0]);
            Ok(Pending{args:args.clone(),_completion:guard,menu,choices,point,fixture_id:fixture[0]})
        })();
        match result{Ok(request)=>{*pending.borrow_mut()=Some(request);if unsafe{PostThreadMessageW(thread,CONTINUE,WPARAM(0),LPARAM(0))}.is_err(){{let abandoned={pending.borrow_mut().take()};drop(abandoned);}errors.set(errors.get()+1)}},Err(e)=>{println!("CUSTOM_CONTEXT request_refused_error={}",e.code().0);errors.set(errors.get()+1)}}
        Ok(())
    }));
    let mut token=0;unsafe{native.add_ContextMenuRequested(&handler,&mut token).unwrap()};
    let removal_result=Rc::new(Cell::new(None));let registration=Registration{native:native.clone(),token,pending:state.pending.clone(),removed:removal_result.clone()};
    pump_until(Duration::from_millis(300),||false);assert_eq!(state.requests.get(),0,"no-input baseline produced a request");
    let trusted_before=command_view(session,tab,"Number(document.documentElement.getAttribute('data-bowser-context-trusted')||0)").unwrap().parse::<u32>().unwrap();
    let geometry=command_view(session,tab,"(()=>{const r=document.getElementById('context-target').getBoundingClientRect();return [Math.round(r.x+r.width/2),Math.round(r.y+r.height/2),innerWidth,innerHeight]})()").and_then(|v|extensions::json::parse(&v)).unwrap();
    let extensions::json::Value::Array(geometry)=geometry else{panic!("owned textarea geometry missing")};let points:Vec<i32>=geometry.iter().map(|v|match v{extensions::json::Value::Number(n)=>n.parse().unwrap(),_=>panic!("nonintegral geometry")}).collect();assert_eq!(points.len(),4);
    assert_eq!(command_view(session,tab,&format!("document.elementFromPoint({},{})===document.getElementById('context-target')",points[0],points[1])).as_deref(),Some("true"));assert!(document()&&authority());
    let mut browser=0;unsafe{view.BrowserProcessId(&mut browser).unwrap()};assert!(document());std::fs::create_dir_all(out).unwrap();
    let mut child=std::process::Command::new("powershell.exe").args(["-NoProfile","-File"]).arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/scripts/probe-custom-context-menu.ps1"))
        .args(["-OwnerPid",&pid.to_string(),"-OwnerHandle",&(owner.0 as isize).to_string(),"-OwnerExe"]).arg(std::env::current_exe().unwrap()).args(["-BrowserPid",&browser.to_string(),"-ClientX",&points[0].to_string(),"-ClientY",&points[1].to_string(),"-ViewportWidth",&points[2].to_string(),"-ViewportHeight",&points[3].to_string(),"-Mode",mode,"-Out"]).arg(out)
        .stdout(std::process::Stdio::from(std::fs::File::create(out.join("helper.stdout.log")).unwrap())).stderr(std::process::Stdio::from(std::fs::File::create(out.join("helper.stderr.log")).unwrap())).spawn().unwrap();
    let clock=Instant::now();let mut status=None;
    while clock.elapsed()<Duration::from_secs(25){
        unsafe{let mut msg=MSG::default();while PeekMessageW(&mut msg,None,0,0,PM_REMOVE).as_bool(){
            if msg.message==CONTINUE&&msg.hwnd==HWND::default(){
                let request=state.pending.borrow_mut().take();
                if let Some(mut request)=request{
                    // Private thread continuation is dispatched only after the native callback returned.
                    if owner_state.current()&&state.requests.get()==1&&state.errors.get()==0&&document()&&authority()&&owner_ready(owner,pid,thread)&&profile(&view).as_ref()==Some(&expected_profile)&&document()&&owner_ready(owner,pid,thread)&&owner_state.current()&&state.requests.get()==1&&state.errors.get()==0{
                        let selected=TrackPopupMenu(request.menu.0,TPM_RETURNCMD|TPM_NONOTIFY,request.point.x,request.point.y,Some(0),owner,None).0;
                        state.selected.set(Some(selected));
                        if selected>0 {
                            let chosen=request.choices.iter().find(|c|c.id==selected);
                            let allowed=owner_state.current()&&state.requests.get()==1&&state.errors.get()==0&&chosen.is_some_and(|c|c.enabled&&same_item(c))&&selected==request.fixture_id&&profile(&view).as_ref()==Some(&expected_profile)&&owner_ready(owner,pid,thread)&&authority()&&document()&&owner_state.current()&&state.requests.get()==1&&state.errors.get()==0;
                            if allowed{let result=request.args.SetSelectedCommandId(selected);println!("CUSTOM_CONTEXT genuine_os_selection_id={selected} native_dispatch_ok={} normalized_error={:?}",result.is_ok(),result.as_ref().err().map(|e|e.code().0));if result.is_err(){state.errors.set(state.errors.get()+1)}}else{println!("CUSTOM_CONTEXT stale_or_unowned_selection_refused=true");state.errors.set(state.errors.get()+1)}
                        }
                    }else{state.errors.set(state.errors.get()+1)}
                    request._completion.complete();
                    if !request.menu.close(){state.errors.set(state.errors.get()+1)}
                }else{state.errors.set(state.errors.get()+1)}
            }else{let _=TranslateMessage(&msg);DispatchMessageW(&msg);}
        }}
        if let Some(done)=child.try_wait().unwrap(){status=Some(done);break}std::thread::sleep(Duration::from_millis(10));
    }
    if status.is_none(){let _=child.kill();let _=child.wait();panic!("custom native menu helper unavailable; no acceptance")}
    drop(registration);assert_eq!(removal_result.get(),Some(0),"native event deregistration must succeed");
    assert!(status.unwrap().success(),"real owned custom-menu input failed");assert_eq!(state.requests.get(),1);assert_eq!(state.errors.get(),0);assert_eq!(state.completion.get(),Some(0),"deferral must complete exactly once successfully");assert!(document()&&authority());
    assert_eq!(command_view(session,tab,"Number(document.documentElement.getAttribute('data-bowser-context-trusted')||0)").unwrap().parse::<u32>().unwrap(),trusted_before+1,"exact current textarea must receive the genuine trusted right click");assert!(document());
    let report=extensions::json::parse(&std::fs::read_to_string(out.join("custom-context-menu.json")).unwrap()).unwrap();println!("CUSTOM_CONTEXT helper_receipt={report:?}");
    let selected=state.selected.get().expect("native menu did not return actual user choice");if mode=="Cancel"{assert_eq!(selected,0)}else{assert!(selected>0);assert_eq!(command_field(&report,"selected_command_id"),Some(&extensions::json::Value::Number(selected.to_string())))}
    assert!(owner_state.current());drop(owner_observer);assert_eq!(owner_detached.get(),Some(true),"native owner lifetime observer must detach successfully");let _=profile_path;selected
}
fn receipt_document(session:&mut Session,tab:TabId,page:&Url)->String{
    let script=format!("[location.href==={},performance.timeOrigin,document.readyState==='complete']",crate::chrome_host::protocol::json_string(page.as_str()));
    let receipt=command_view(session,tab,&script).unwrap();assert!(receipt.len()<=64);
    assert!(matches!(extensions::json::parse(&receipt),Some(extensions::json::Value::Array(a)) if a.len()==3&&a[0]==extensions::json::Value::Bool(true)&&matches!(&a[1],extensions::json::Value::Number(n) if n.parse::<f64>().is_ok_and(|v|v.is_finite()&&v>0.0))&&a[2]==extensions::json::Value::Bool(true)));receipt
}
fn ready_focus(session:&mut Session,tab:TabId){session.engine.select_tab(tab).unwrap();session.engine.set_visible(true).unwrap();unsafe{let _=ShowWindow(session.host,SW_SHOW);let _=SetForegroundWindow(session.host);}session.engine.focus_page().unwrap();pump_until(Duration::from_millis(300),||false);assert_eq!(command_view(session,tab,"document.hasFocus()").as_deref(),Some("true"));}
fn marker(session:&mut Session,tab:TabId,role:&str)->Option<String>{command_view(session,tab,&format!("document.documentElement.getAttribute('data-bowser-native-command-{role}')"))}
fn denied_for(receipt:&extensions::json::Value,id:&extensions::json::Value){use extensions::json::Value;let Some(Value::Array(attempts))=command_field(receipt,"preflight")else{panic!("genuine preflight attempts absent")};assert!(attempts.iter().any(|a|command_field(a,"id")==Some(id)&&command_field(a,"allowed")==Some(&Value::Bool(false))&&command_field(a,"permissionDenied")==Some(&Value::Bool(true))),"permission-specific pre-gesture denial must match actual event ID");}
#[test]
#[ignore="requires genuine owned OS context-menu cancellation and selection in one isolated disposable test"]
fn live_probe_custom_context_menu(){
    use extensions::json::Value;
    let args:Vec<_>=std::env::args().skip(1).collect();assert_eq!(args.iter().map(String::as_str).collect::<Vec<_>>(),vec!["engine::extension_probe_live::custom_context_menu::live_probe_custom_context_menu","--exact","--ignored","--nocapture","--test-threads=1"]);
    unsafe{CoInitializeEx(None,COINIT_APARTMENTTHREADED).ok().unwrap()};let root=temp_root();
    for active_tab in [true,false]{
        let label=if active_tab{"active-tab"}else{"no-active-tab"};let profile_path=root.join(label);let source=root.join(format!("source-{label}"));std::fs::create_dir(&source).unwrap();
        for name in ["manifest.json","worker.js","popup.html","popup.js"]{std::fs::copy(fixture("probe-custom-context-menu").join(name),source.join(name)).unwrap();}
        if !active_tab{let manifest=std::fs::read_to_string(source.join("manifest.json")).unwrap().replace(",\n    \"activeTab\"","");assert!(!manifest.contains("\"activeTab\""));std::fs::write(source.join("manifest.json"),manifest).unwrap();}
        let record=extensions::install(&source,&profile_path.join("Extensions"),&profile_path.join("extensions.txt")).unwrap();let folder=extensions::folder_for(&profile_path.join("Extensions"),&record.id).unwrap();let loopback=Loopback::start();let page=loopback.page("context-menu-observation");
        let mut session=Session::open(&profile_path,vec![(record.id.clone(),folder.clone())],vec![(record.id.clone(),true)],&page);let (native,failure)=session.wait_for_add(&profile_path,&record.id);assert!(failure.is_none());assert_eq!(native.as_deref(),Some(KEY_FIXTURE_ID));
        session.engine.select_tab(TabId(2)).unwrap();session.engine.navigate_tab(TabId(2),&page).unwrap();
        let ready=format!("location.href==={}&&document.readyState==='complete'",crate::chrome_host::protocol::json_string(page.as_str()));for tab in [TabId(1),TabId(2)]{assert!(pump_until(Duration::from_secs(20),||command_view(&mut session,tab,&ready).as_deref()==Some("true")));}
        let documents=[receipt_document(&mut session,TabId(1),&page),receipt_document(&mut session,TabId(2),&page)];
        session.open_page(&record.id,&folder,extensions::PageKind::Popup).unwrap();assert!(pump_until(Duration::from_secs(10),||session.page_script("document.documentElement.getAttribute('data-ready')").as_deref()==Some("\"yes\"")));
        let mut creation=None;assert!(pump_until(Duration::from_secs(10),||{creation=command_receipt(&mut session,"native-command-state");creation.as_ref().and_then(|r|command_field(r,"creation")).is_some_and(|c|command_field(c,"complete")==Some(&Value::Bool(true)))}));let creation=creation.unwrap();let worker=command_field(&creation,"workerInstance").unwrap().clone();let installed=command_field(&creation,"creation").unwrap().clone();assert_eq!(command_field(&installed,"ok"),Some(&Value::Bool(true)));assert_eq!(command_field(&installed,"error"),Some(&Value::Null));
        let before=command_receipt(&mut session,"native-command-preflight").unwrap();println!("CUSTOM_CONTEXT scope=sole-consented-keyed-fixture-one-owner-feasibility active_tab={active_tab} pregesture={before:?}");
        let Some(Value::Array(denials))=command_field(&before,"preflight")else{panic!("native query IDs missing")};assert!(!denials.is_empty());for a in denials{assert_eq!(command_field(a,"allowed"),Some(&Value::Bool(false)));assert_eq!(command_field(a,"permissionDenied"),Some(&Value::Bool(true)));}
        ready_focus(&mut session,TabId(1));run_input(&mut session,TabId(1),&profile_path,"Cancel",&PathBuf::from("store-custom-context-menu").join(label).join("cancel"));
        pump_until(Duration::from_secs(2),||false);let after=command_receipt(&mut session,"native-command-preflight").unwrap();assert_eq!(command_field(&after,"workerInstance"),Some(&worker));assert_eq!(command_field(&after,"creation"),Some(&installed));assert_eq!(command_field(&after,"events"),Some(&Value::Array(vec![])));for a in denials{denied_for(&after,command_field(a,"id").unwrap());}
        for (index,tab) in [TabId(1),TabId(2)].into_iter().enumerate(){assert_eq!(receipt_document(&mut session,tab,&page),documents[index]);assert_eq!(marker(&mut session,tab,"primary").as_deref(),Some("null"));assert_eq!(marker(&mut session,tab,"negative").as_deref(),Some("null"));}
        let mut event_ids=Vec::new();let mut tokens=Vec::new();let mut other_attempts=Vec::new();
        for (index,tab) in [TabId(1),TabId(2)].into_iter().enumerate(){
            ready_focus(&mut session,tab);run_input(&mut session,tab,&profile_path,"Select",&PathBuf::from("store-custom-context-menu").join(label).join(format!("select-{}",index+1)));
            let mut receipt=None;assert!(pump_until(Duration::from_secs(10),||{receipt=command_receipt(&mut session,"native-command-state");receipt.as_ref().and_then(|r|command_field(r,"events")).is_some_and(|events|matches!(events,Value::Array(events) if events.len()==index+1&&command_field(&events[index],"complete")==Some(&Value::Bool(true))))}),"genuine native onClicked/settled injection absent; no functionality acceptance");
            let receipt=receipt.unwrap();println!("CUSTOM_CONTEXT actual_event_receipt={receipt:?}");assert_eq!(command_field(&receipt,"workerInstance"),Some(&worker));assert_eq!(command_field(&receipt,"creation"),Some(&installed));
            let Some(Value::Array(events))=command_field(&receipt,"events")else{unreachable!()};let event=&events[index];assert!(command_field(event,"error").is_none(),"native event probes must settle without an error");other_attempts.push(command_field(event,"otherTabAttempts").expect("settled native other-view attempts missing").clone());assert_eq!(command_field(event,"eventKind"),Some(&Value::String("contextMenus.onClicked".into())));let info=command_field(event,"info").unwrap();assert_eq!(command_field(info,"menuItemId"),Some(&Value::String("bowser-native-context-probe".into())));assert_eq!(command_field(info,"editable"),Some(&Value::Bool(true)));assert_eq!(command_field(info,"frameId"),Some(&Value::Number("0".into())));
            let id=command_field(command_field(event,"nativeTab").unwrap(),"id").unwrap().clone();assert!(matches!(&id,Value::Number(n) if n.parse::<i64>().is_ok()));denied_for(&before,&id);event_ids.push(id.clone());let token=command_field(event,"primaryToken").unwrap().clone();assert!(matches!(&token,Value::String(t) if t.starts_with("native-context-primary-")));tokens.push(token.clone());let injection=command_field(event,"injection").unwrap();assert_eq!(command_field(injection,"attempted"),Some(&Value::Bool(true)));assert_eq!(command_field(injection,"id"),Some(&id));assert_eq!(command_field(injection,"allowed"),Some(&Value::Bool(active_tab)));
            if active_tab{
                let Some(Value::Array(results))=command_field(injection,"nativeResults")else{panic!("actual native execution result missing")};assert_eq!(results.len(),1);assert_eq!(command_field(&results[0],"frameId"),Some(&Value::Number("0".into())));assert_eq!(command_field(&results[0],"result"),Some(&token));let Value::String(t)=&token else{unreachable!()};assert_eq!(marker(&mut session,tab,"primary"),Some(crate::chrome_host::protocol::json_string(t)));
                if index==0{assert_eq!(marker(&mut session,TabId(2),"primary").as_deref(),Some("null"));assert_eq!(marker(&mut session,TabId(2),"negative").as_deref(),Some("null"));}
            }else{assert_eq!(command_field(injection,"permissionDenied"),Some(&Value::Bool(true)));}
            for (i,t) in [TabId(1),TabId(2)].into_iter().enumerate(){assert_eq!(receipt_document(&mut session,t,&page),documents[i]);}
        }
        assert_ne!(event_ids[0],event_ids[1]);assert_ne!(tokens[0],tokens[1]);
        for (index,other) in [1_usize,0].into_iter().enumerate(){
            let Value::Array(attempts)=&other_attempts[index]else{panic!("settled other-view native receipt missing")};
            let attempt=attempts.iter().find(|a|command_field(a,"id")==Some(&event_ids[other])).expect("actual other owned native ID absent from settled attempts");
            if index==0||!active_tab{assert_eq!(command_field(attempt,"allowed"),Some(&Value::Bool(false)));assert_eq!(command_field(attempt,"permissionDenied"),Some(&Value::Bool(true)));}
            else{assert!(matches!(command_field(attempt,"allowed"),Some(Value::Bool(_))),"prior grant observation must settle, not be labelled a denial");}
        }
        if active_tab{for (index,tab) in [TabId(1),TabId(2)].into_iter().enumerate(){let Value::String(token)=&tokens[index]else{unreachable!()};assert_eq!(marker(&mut session,tab,"primary"),Some(crate::chrome_host::protocol::json_string(token)),"a later gesture/negative probe must not replace the earlier primary token");}}

        if !active_tab{for tab in [TabId(1),TabId(2)]{for role in ["primary","negative"]{assert_eq!(marker(&mut session,tab,role).as_deref(),Some("null"));}}}
        if active_tab{
            let origin=Url::parse(&format!("http://localhost:{}/context-menu-observation",loopback.port)).unwrap();session.engine.navigate_tab(TabId(2),&origin).unwrap();let ready=format!("location.href==={}&&document.readyState==='complete'",crate::chrome_host::protocol::json_string(origin.as_str()));assert!(pump_until(Duration::from_secs(20),||command_view(&mut session,TabId(2),&ready).as_deref()==Some("true")));let document=receipt_document(&mut session,TabId(2),&origin);
            let receipt=command_receipt(&mut session,"native-command-again").unwrap();println!("CUSTOM_CONTEXT retained_id_cross_origin={receipt:?}");let again=command_field(&receipt,"again").unwrap();assert_eq!(command_field(again,"id"),Some(&event_ids[1]));assert_eq!(command_field(again,"allowed"),Some(&Value::Bool(false)));assert_eq!(command_field(again,"permissionDenied"),Some(&Value::Bool(true)));assert_eq!(command_field(&receipt,"workerInstance"),Some(&worker));for role in ["primary","negative"]{assert_eq!(marker(&mut session,TabId(2),role).as_deref(),Some("null"));}assert_eq!(receipt_document(&mut session,TabId(2),&origin),document);
        }
        println!("CUSTOM_CONTEXT active_tab={active_tab} native_cancellation_no_grant=true actual_engine_dispatch_and_native_event=true duplicate_view_native_ids_differ=true settled_permission_controls=true scope=warm-worker-top-level-sole-fixture-single-owner-not-production-or-Bitwarden");session.close();drop(loopback);
    }
    remove_with_retries(&root);
}
