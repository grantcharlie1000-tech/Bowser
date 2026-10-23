//! Distinct, test-only actual-App same-URL Reload refusal lane. One fresh profile/process,
//! genuine keyed extension fixture, one physical right-click, then no further input.
//! No production cancellation/permission/routing/default changes. A popup that fails to
//! disappear spontaneously is a required failure, not permission to rescue the test.
use super::*;
use std::rc::Weak;
use windows::{core::BOOL, Win32::{
    System::Threading::GetCurrentThreadId,
    UI::Input::KeyboardAndMouse::IsWindowEnabled,
    UI::WindowsAndMessaging::{
        EnumWindows, GetGUIThreadInfo, GetMenuBarInfo, GetWindowRect, GetWindowThreadProcessId,
        GUITHREADINFO, GUI_POPUPMENUMODE, IsWindowVisible, KillTimer,
        MENUBARINFO, OBJID_CLIENT, SetTimer,
    },
}};

pub(crate) const RELOAD_TIMER:usize=0xB0A8;
const RELOAD_TEST_NAME:&str="engine::app_context_menu_probe::reload::live_probe_real_app_context_menu_same_url_reload";
const RELOAD_CONSENT:&str="probe-custom-context-menu-reload-no-further-input";
const EVIDENCE:&str="store-real-app-context-menu-reload";
// New-lane bounds only. The original Cancel/Select helper and its deadlines are unchanged.
const ARM_LIFETIME:Duration=Duration::from_secs(25);
const MAX_START_URL_BYTES:usize=4096;

// Diagnostic-only: six write attempts, one retained CreateNew stream, no BOM/raw URLs.
// Neither missing output nor a returned App call proves native Reload or navigation success.
const JOURNAL_MAX_BYTES:usize=1024;
const JOURNAL_MAX_WRITES:u8=6;
#[derive(Clone,Copy,Default)]
struct JournalSnapshot {
    stages:u8,phase:u8,engine:u8,native_seen:u64,recorded:u64,
    ids:[u64;2],lengths:[u64;2],event_phase:[u8;2],event_elapsed_ms:[u64;2],same_view:[bool;2],url_equal:[bool;2],stored:[bool;2],
    invalid:bool,saturated:bool,
}
struct ReloadJournal {
    file:Cell<Option<std::fs::File>>,snapshot:Cell<JournalSnapshot>,
    writes:Cell<u8>,io_failed:Cell<bool>,started:Instant,
}
impl ReloadJournal {
    fn open(path:&Path)->Rc<Self> {
        let file=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
            std::fs::OpenOptions::new().write(true).create_new(true).open(path).ok())).ok().flatten();
        let unavailable=file.is_none();
        let journal=Rc::new(Self{file:Cell::new(file),snapshot:Cell::new(JournalSnapshot::default()),
            writes:Cell::new(0),io_failed:Cell::new(unavailable),started:Instant::now()});
        journal.record(1);journal
    }
    fn record(&self,stage:u8) {
        // Diagnostic errors/panics must not replace an original error or become an action gate.
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(||self.write_snapshot(stage))).is_err() {
            self.io_failed.set(true);
        }
    }
    fn write_snapshot(&self,stage:u8) {
        let mut snapshot=self.snapshot.get();snapshot.stages|=stage;self.snapshot.set(snapshot);
        let previous=self.writes.get();if previous>=JOURNAL_MAX_WRITES{return;}
        let attempt=previous+1;self.writes.set(attempt);
        let elapsed=self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        let value=format!(r#"{{"v":1,"write":{attempt},"stage_bits":{},"call_phase":{},"engine":{},"elapsed_ms":{elapsed},"native_seen":{},"recorded":{},"ids":[{},{}],"url_bytes":[{},{}],"event_phase":[{},{}],"event_elapsed_ms":[{},{}],"same_view":[{},{}],"url_equal":[{},{}],"stored":[{},{}],"invalid":{},"saturated":{},"io_failed_before_write":{}}}"#,
            snapshot.stages,snapshot.phase,snapshot.engine,snapshot.native_seen,snapshot.recorded,
            snapshot.ids[0],snapshot.ids[1],snapshot.lengths[0],snapshot.lengths[1],snapshot.event_phase[0],snapshot.event_phase[1],
            snapshot.event_elapsed_ms[0],snapshot.event_elapsed_ms[1],snapshot.same_view[0],snapshot.same_view[1],
            snapshot.url_equal[0],snapshot.url_equal[1],snapshot.stored[0],snapshot.stored[1],snapshot.invalid,snapshot.saturated,self.io_failed.get());
        if value.len()>JOURNAL_MAX_BYTES {self.io_failed.set(true);return;}
        // Take the sole retained handle out of Cell. No probe-owned App/RefCell/TLS borrow
        // spans these best-effort synchronous writes; the unchanged caller's Core borrow may.
        let Some(mut file)=self.file.take() else {self.io_failed.set(true);return;};
        let result=std::io::Seek::rewind(&mut file).and_then(|_|file.write_all(value.as_bytes()))
            .and_then(|_|file.set_len(value.len() as u64)).and_then(|_|file.flush());
        if result.is_err(){self.io_failed.set(true);}self.file.set(Some(file));
    }
    fn pre_final_gates(&self,engine:u8) {
        let mut snapshot=self.snapshot.get();snapshot.engine=engine;self.snapshot.set(snapshot);self.record(2);
    }
    fn call_attempt(&self) {
        // In-memory only, after all refusal fences directly beside App::reload. NO I/O.
        let mut snapshot=self.snapshot.get();snapshot.phase=1;self.snapshot.set(snapshot);
    }
    fn returned(&self) {
        let mut snapshot=self.snapshot.get();snapshot.phase=2;self.snapshot.set(snapshot);self.record(8);
    }
    fn native_start(&self,id:u64,length:u64,same_view:bool,url_equal:bool,stored:bool,recorded:u64,invalid:bool,saturated:bool) {
        let mut snapshot=self.snapshot.get();let index=snapshot.native_seen;
        snapshot.native_seen=snapshot.native_seen.saturating_add(1).min(65);snapshot.recorded=recorded;
        snapshot.invalid|=invalid;snapshot.saturated|=saturated;
        if index<2 {let index=index as usize;snapshot.ids[index]=id;snapshot.lengths[index]=length;
            snapshot.event_phase[index]=snapshot.phase;
            snapshot.event_elapsed_ms[index]=self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
            snapshot.same_view[index]=same_view;snapshot.url_equal[index]=url_equal;snapshot.stored[index]=stored;}
        self.snapshot.set(snapshot);
        // First two classifications are sufficient to distinguish first-event from ambiguity.
        // Later existing observations update fixed memory only; return/error may persist them.
        if index<2 {self.record(4);}
    }
    fn error(&self) {self.record(16);}
}
fn journal_native_start(id:u64,length:usize,same_view:bool,url_equal:bool,stored:bool) {
    let _=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let journal=JOURNAL.with(|slot|slot.try_borrow().ok().and_then(|slot|slot.clone()));
        // Every TLS/vector borrow ends before invoking the recorder's best-effort I/O.
        let recorded=STARTS.with(|events|events.borrow().len()) as u64;
        let invalid=STARTS_INVALID.with(Cell::get);let saturated=STARTS_SATURATED.with(Cell::get);
        if let Some(journal)=journal {journal.native_start(id,length as u64,same_view,url_equal,stored,recorded,invalid,saturated);}
    }));
}


#[derive(Clone)]
pub(crate) struct ShowWitness {
    pub(crate) request:usize,pub(crate) owner:HWND,pub(crate) owner_token:usize,
    pub(crate) request_epoch:u64,pub(crate) tab:TabId,pub(crate) view:usize,
    pub(crate) controller:usize,pub(crate) proof:(u64,u64,u64),pub(crate) hmenu:usize,
    pub(crate) fixture_command:i32,pub(crate) current:Rc<dyn Fn()->bool>,
    pub(crate) pure_current:Rc<dyn Fn()->bool>,
}
#[derive(Clone,Debug)]
struct NativeStart {tab:TabId,view:usize,id:u64,url_matches:bool,url_length:usize}
thread_local! {
    static CAPTURE:Cell<bool> = const {Cell::new(false)};
    static TIMER_BOUNDARY_FAILED:Cell<bool> = const {Cell::new(false)};
    static SHOW_SINK:RefCell<Option<Rc<dyn Fn(ShowWitness)>>> = RefCell::new(None);
    static STARTS:RefCell<Vec<NativeStart>> = const {RefCell::new(Vec::new())};
    static START_SOURCE:RefCell<Option<(TabId,usize,String)>> = const {RefCell::new(None)};
    static STARTS_SATURATED:Cell<bool> = const {Cell::new(false)};
    static STARTS_INVALID:Cell<bool> = const {Cell::new(false)};
    static REMOVALS:RefCell<Vec<(TabId,usize,Option<i32>)>> = const {RefCell::new(Vec::new())};
    static TIMER_STATE:RefCell<Option<TimerState>> = const {RefCell::new(None)};
    static JOURNAL:RefCell<Option<Rc<ReloadJournal>>> = const {RefCell::new(None)};
}
pub(crate) fn armed()->bool {SHOW_SINK.with(|slot|slot.borrow().is_some())}
pub(crate) fn observe_before_show(witness:ShowWitness) {
    let sink=SHOW_SINK.with(|slot|slot.borrow().clone());
    if let Some(sink)=sink {let _=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||sink(witness)));}
}
pub(crate) fn observe_navigation_start(tab:TabId,view:usize,id:u64,url:&str) {
    if !CAPTURE.with(Cell::get) {return;}
    // Store bounded classifications for the exact retained source only, NEVER copy native URLs.
    let source=START_SOURCE.with(|slot| {let slot=slot.borrow();let (expected_tab,expected_view,expected_url)=slot.as_ref()?;
        (*expected_tab==tab).then_some((*expected_view==view,url==expected_url.as_str()))
    });
    let Some((same_view,url_matches))=source else{return;};
    if !same_view||url.len()>MAX_START_URL_BYTES {STARTS_INVALID.with(|failed|failed.set(true));
        journal_native_start(id,url.len(),same_view,url_matches,false);return;}
    let mut stored=false;
    STARTS.with(|events| {let mut events=events.borrow_mut();
        if events.len()>=64 {STARTS_SATURATED.with(|failed|failed.set(true));return;}
        events.push(NativeStart {tab,view,id,url_matches,url_length:url.len()});stored=true;
    });
    journal_native_start(id,url.len(),same_view,url_matches,stored);
}
pub(crate) fn observe_deregistered(tab:TabId,view:usize,error:Option<i32>) {
    if CAPTURE.with(Cell::get) {REMOVALS.with(|events| {let mut events=events.borrow_mut();
        if events.len()<64 {events.push((tab,view,error));}
    });}
}
fn write_once(path:&Path,value:&str) {
    assert!(value.len()<=8192,"diagnostic witness exceeds bound");
    let pending=path.with_extension("pending");
    let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(&pending).unwrap();
    file.write_all(value.as_bytes()).unwrap();file.sync_all().unwrap();drop(file);
    assert!(!path.exists());std::fs::rename(pending,path).unwrap();
}
struct ProbeScope;
impl Drop for ProbeScope {fn drop(&mut self) {
    let sink=SHOW_SINK.with(|slot|slot.borrow_mut().take());drop(sink);
    let journal=JOURNAL.with(|slot|slot.borrow_mut().take());drop(journal);
    install_observer(Rc::new(|_|{}));
}}

// Exact original AppRef lifetime plus native HWND/PID/creating STA, never HWND alone.
#[derive(Clone)]
struct OwnerLifetime {app:Weak<RefCell<App>>,hwnd:HWND,thread:u32,token:usize,native_lifetime:Rc<dyn Fn()->bool>}
impl OwnerLifetime {
    fn current(&self)->Option<AppRef> {
        if !(self.native_lifetime)() {return None;}
        let app=self.app.upgrade()?;
        if app.try_borrow().ok()?.hwnd!=self.hwnd {return None;}
        let mut pid=0;let mut class=[0;64];
        if self.thread!=unsafe{GetCurrentThreadId()}
            || !unsafe{IsWindow(Some(self.hwnd))}.as_bool()
            || unsafe{GetWindowThreadProcessId(self.hwnd,Some(&mut pid))}!=self.thread
            || pid!=std::process::id() {return None;}
        let n=unsafe{GetClassNameW(self.hwnd,&mut class)};
        (String::from_utf16_lossy(&class[..n.max(0) as usize])=="BowserNativeWindow"&&(self.native_lifetime)()).then_some(app)
    }
    fn disarm(&self)->bool {
        self.current().is_some() && (self.native_lifetime)() && unsafe{KillTimer(Some(self.hwnd),RELOAD_TIMER)}.is_ok()
    }
}
#[derive(Default,Debug)]
struct TimerResult {arm_return:Option<usize>,reload_calls:u32,kill_timer:Option<bool>,popup:Option<(usize,usize)>,failure:Option<String>}
struct TimerState {
    owner:OwnerLifetime,handles:NativeContextProbeHandles,tab:TabId,proof:(u64,u64,u64),
    witness:Rc<RefCell<Option<ShowWitness>>>,out:std::path::PathBuf,nonce:String,
    started:Instant,result:Rc<RefCell<TimerResult>>,trusted_before:i64,journal:Rc<ReloadJournal>,
}
struct TimerScope {owner:OwnerLifetime,result:Rc<RefCell<TimerResult>>}
impl Drop for TimerScope {fn drop(&mut self) {
    let pending=TIMER_STATE.with(|slot|slot.borrow_mut().take());
    if let Some(state)=pending {
        // No TLS/App/engine borrow spans KillTimer. Failed identity means no reused-HWND action.
        let killed=self.owner.disarm();
        let mut result=self.result.borrow_mut();result.kill_timer=Some(killed);
        if result.failure.is_none(){result.failure=Some("timer retired without an authorized Reload".into());}
        drop(result);drop(state);
    }
}}
fn arm_timer(app:&AppRef,h:&NativeContextProbeHandles,tab:TabId,proof:(u64,u64,u64),witness:Rc<RefCell<Option<ShowWitness>>>,out:&Path,nonce:&str,result:Rc<RefCell<TimerResult>>,trusted_before:i64,journal:Rc<ReloadJournal>)->TimerScope {
    let hwnd=app.borrow().hwnd;
    let (token,native_lifetime)=super::super::webview2::probe_reload_owner_lifetime(hwnd).expect("existing native Owner lifetime unavailable before test timer arm");
    let owner=OwnerLifetime{app:Rc::downgrade(app),hwnd,thread:unsafe{GetCurrentThreadId()},token,native_lifetime};
    assert!(owner.current().is_some());
    TIMER_STATE.with(|slot|{let mut slot=slot.borrow_mut();assert!(slot.is_none());*slot=Some(TimerState{
        owner:owner.clone(),handles:h.clone(),tab,proof,witness,out:out.into(),nonce:nonce.into(),started:Instant::now(),result:result.clone(),trusted_before,journal,
    });});
    // Own cleanup before native setup or its success assertion can fail. With an HWND,
    // the requested ID remains the callback/KillTimer identity; success is merely nonzero.
    let scope=TimerScope{owner,result:result.clone()};
    let timer=unsafe{SetTimer(Some(hwnd),RELOAD_TIMER,50,Some(reload_timer))};
    result.borrow_mut().arm_return=Some(timer);
    assert_ne!(timer,0,"unique exact-owner test timer was not armed");
    scope
}
struct PopupScan {thread:u32,pid:u32,examined:usize,complete:bool,windows:Vec<HWND>}
unsafe extern "system" fn collect_popup(hwnd:HWND,param:LPARAM)->BOOL {
    let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // Pointer is to our live, synchronous EnumWindows stack state only.
        let scan=unsafe{&mut *(param.0 as *mut PopupScan)};scan.examined+=1;
        if scan.examined>128 {scan.complete=false;return BOOL(0);}
        let mut pid=0;let thread=unsafe{GetWindowThreadProcessId(hwnd,Some(&mut pid))};
        if pid==scan.pid&&thread==scan.thread&&unsafe{IsWindowVisible(hwnd)}.as_bool() {
            let mut class=[0;64];let n=unsafe{GetClassNameW(hwnd,&mut class)};
            if String::from_utf16_lossy(&class[..n.max(0) as usize])=="#32768" {scan.windows.push(hwnd);}
        }
        BOOL(1)
    }));
    result.unwrap_or(BOOL(0))
}
fn native_popup(owner:&OwnerLifetime,witness:&ShowWitness,popup:usize)->bool {
    // All required COM getters precede the native observations below. No COM after them.
    if !(witness.current)() || owner.current().is_none() {return false;}
    let mut gui=GUITHREADINFO{cbSize:std::mem::size_of::<GUITHREADINFO>() as u32,..Default::default()};
    if unsafe{GetGUIThreadInfo(owner.thread,&mut gui)}.is_err() || gui.hwndActive!=owner.hwnd
        || gui.hwndMenuOwner!=owner.hwnd || !gui.flags.contains(GUI_POPUPMENUMODE)
        || unsafe{GetForegroundWindow()}!=owner.hwnd {return false;}
    let mut scan=PopupScan{thread:owner.thread,pid:std::process::id(),examined:0,complete:true,windows:Vec::new()};
    let enumeration=unsafe{EnumWindows(Some(collect_popup),LPARAM(std::ptr::addr_of_mut!(scan) as isize))};
    if enumeration.is_err()||!scan.complete||scan.windows.len()!=1||scan.windows[0].0 as usize!=popup {return false;}
    let hwnd=scan.windows[0];let mut rect=RECT::default();
    let mut bar=MENUBARINFO{cbSize:std::mem::size_of::<MENUBARINFO>() as u32,..Default::default()};
    if !unsafe{IsWindowEnabled(hwnd)}.as_bool()||unsafe{GetWindowRect(hwnd,&mut rect)}.is_err()
        ||rect.right<=rect.left||rect.bottom<=rect.top
        ||unsafe{GetMenuBarInfo(hwnd,OBJID_CLIENT,0,&mut bar)}.is_err()||bar.hMenu.0 as usize!=witness.hmenu {return false;}
    // Fresh end-of-read owner/menu/proof check, not ID/URL/order inference.
    bar=MENUBARINFO{cbSize:std::mem::size_of::<MENUBARINFO>() as u32,..Default::default()};
    let mut pid=0;
    if !unsafe{IsWindow(Some(hwnd))}.as_bool()||!unsafe{IsWindowVisible(hwnd)}.as_bool()
        ||unsafe{GetWindowThreadProcessId(hwnd,Some(&mut pid))}!=owner.thread||pid!=std::process::id()
        ||unsafe{GetMenuBarInfo(hwnd,OBJID_CLIENT,0,&mut bar)}.is_err()||bar.hMenu.0 as usize!=witness.hmenu {return false;}
    gui=GUITHREADINFO{cbSize:std::mem::size_of::<GUITHREADINFO>() as u32,..Default::default()};
    if unsafe{GetGUIThreadInfo(owner.thread,&mut gui)}.is_err()||gui.hwndActive!=owner.hwnd
        ||gui.hwndMenuOwner!=owner.hwnd||!gui.flags.contains(GUI_POPUPMENUMODE)
        ||unsafe{GetForegroundWindow()}!=owner.hwnd {return false;}
    // Final separate pure owner/request/controller/document/token fence, with NO COM/native call.
    (owner.native_lifetime)()&&(witness.pure_current)()
}
fn require_arm_lifetime(state:&TimerState) {
    assert!(state.started.elapsed()<ARM_LIFETIME,"new-lane timer lifetime expired; no Reload");
}
fn timer_step(state:&TimerState)->bool {
    require_arm_lifetime(state);
    let Some(witness)=({state.witness.borrow().clone()}) else{return false;};
    let path=state.out.join("native-visible.json");
    let Ok(metadata)=std::fs::metadata(&path) else{return false;};
    assert!(metadata.len()>0&&metadata.len()<=8192);
    let visible=json::parse(&std::fs::read_to_string(path).unwrap()).unwrap();require_arm_lifetime(state);
    assert_eq!(text(&visible,"schema"),"bowser-real-app-menu-reload-visible-v1");assert_eq!(text(&visible,"nonce"),state.nonce);
    assert_eq!(text(&visible,"request"),witness.request.to_string());assert_eq!(number(&visible,"owner"),state.owner.hwnd.0 as isize as i64);
    assert_eq!(text(&visible,"owner_token"),witness.owner_token.to_string());assert_eq!(text(&visible,"request_epoch"),witness.request_epoch.to_string());
    assert_eq!(number(&visible,"owner_pid"),std::process::id() as i64);assert_eq!(number(&visible,"owner_thread"),state.owner.thread as i64);
    assert_eq!(number(&visible,"menu_handle") as usize,witness.hmenu);let popup=number(&visible,"popup_handle") as usize;assert!(popup>0);
    assert_eq!(witness.owner,state.owner.hwnd);assert_eq!(witness.owner_token,state.owner.token);assert_eq!(witness.tab,state.tab);assert_eq!(witness.proof,state.proof);
    assert_eq!(witness.view,state.handles.view.as_raw() as usize);assert_eq!(witness.controller,state.handles.controller.as_raw() as usize);
    assert!(witness.owner_token>0&&witness.request_epoch>0&&witness.request>0&&witness.hmenu>0);
    let app=state.owner.current().expect("original exact App owner lifetime unavailable");require_arm_lifetime(state);
    assert_eq!(App::active_tab_id(&app),state.tab);current(&state.handles,state.proof);ordinary_profile(&state.handles.view);require_arm_lifetime(state);isolated_environment(&app,&state.handles);require_arm_lifetime(state);
    assert!(native_popup(&state.owner,&witness,popup),"exact request HMENU is not freshly native-visible");require_arm_lifetime(state);
    assert_eq!(script(&state.handles.view,"Number(document.documentElement.getAttribute('data-app-context-trusted')||0)"),Value::Number((state.trusted_before+1).to_string()),"one trusted right-click must reach the exact retained source document before Reload");require_arm_lifetime(state);
    no_effects(&state.handles.view);require_arm_lifetime(state);current(&state.handles,state.proof);require_arm_lifetime(state);
    // TIMER_STATE was taken before this callback. Disarm and retain the actual result BEFORE
    // ordinary App reload, with no PROBE-OWNED App/Core/gate/TLS/observer borrow held.
    // The unchanged production engine.reload path still owns its existing Core RefMut.
    let killed=state.owner.disarm();state.result.borrow_mut().kill_timer=Some(killed);assert!(killed,"actual KillTimer failed");require_arm_lifetime(state);
    write_once(&state.out.join("reload-trigger.json"),&format!(r#"{{"schema":"bowser-real-app-menu-reload-trigger-v1","request":"{}","owner_token":"{}","request_epoch":"{}","menu_handle":{},"popup_handle":{},"actual_kill_timer":true,"original_navigation_id":"{}"}}"#,witness.request,witness.owner_token,witness.request_epoch,witness.hmenu,popup,state.proof.0));require_arm_lifetime(state);
    // Pure diagnostic classification only: 0 unavailable, 1 missing, 2 present. Release the
    // App borrow before journal I/O; all existing final action fences still run AFTER it.
    let engine={app.try_borrow().ok().map(|app|if app.engine.is_some(){2}else{1}).unwrap_or(0)};
    state.journal.pre_final_gates(engine);
    // Final freshness check after I/O; never rebind the request to a replacement document.
    current(&state.handles,state.proof);require_arm_lifetime(state);assert!(native_popup(&state.owner,&witness,popup));require_arm_lifetime(state);
    assert_eq!(App::active_tab_id(&app),state.tab);
    assert_eq!(state.result.borrow().reload_calls,0);
    assert!((state.owner.native_lifetime)()&&(witness.pure_current)());
    require_arm_lifetime(state); // Final pure deadline fence after ALL reads and before Reload.
    // Count an actual App call attempt only AFTER every refusal fence, with no native getter.
    {let mut result=state.result.borrow_mut();result.reload_calls=1;result.popup=Some((popup,witness.hmenu));}
    state.journal.call_attempt(); // Pure phase1, no I/O and no new refusal condition.
    App::reload(&app); // Real ordinary active-tab engine path; no fabricated navigation or input.
    state.journal.returned(); // Phase2 means App returned, NOT native Reload/navigation success.
    true
}
fn reload_timer_inner(hwnd:HWND,id:usize) {
    // Take the state before every native/App call. Nested dispatch sees an empty slot.
    let state=TIMER_STATE.with(|slot|slot.borrow_mut().take());let Some(state)=state else{return;};
    let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert_eq!(hwnd,state.owner.hwnd);assert_eq!(id,RELOAD_TIMER);assert!(state.owner.current().is_some());
        timer_step(&state)
    }));
    match result {
        Ok(false)=>TIMER_STATE.with(|slot|{let mut slot=slot.borrow_mut();assert!(slot.is_none());*slot=Some(state);}),
        Ok(true)=>{},
        Err(_)=>{
            // Never unwind across Win32 or attempt EndMenu/WM_CLOSE/input as failure rescue.
            let prior={state.result.borrow().kill_timer};let killed=prior.unwrap_or_else(||state.owner.disarm());
            {let mut result=state.result.borrow_mut();if result.kill_timer.is_none(){result.kill_timer=Some(killed);}result.failure=Some("test timer native/request fence or Reload failed".into());}
            let _=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||write_once(&state.out.join("timer-failure.json"),&format!(r#"{{"status":"failed-no-acceptance","actual_kill_timer":{},"no_rescue_input":true}}"#,killed))));
            state.journal.error();
        }
    }
}

unsafe extern "system" fn reload_timer(hwnd:HWND,_message:u32,id:usize,_time:u32) {
    // The ENTIRE extern boundary, including TLS take/restore and failure reporting, is contained.
    // Keep only clone-only emergency lifetime state; no RefCell guard survives native calls.
    let mut emergency=None;
    let outcome=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        emergency=TIMER_STATE.with(|slot|slot.try_borrow().ok().and_then(|state|state.as_ref().map(|state|
            (state.owner.clone(),state.result.clone(),state.out.clone(),state.journal.clone()))));
        reload_timer_inner(hwnd,id);
    }));
    if outcome.is_err() {
        let _=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            TIMER_BOUNDARY_FAILED.with(|failed|failed.set(true));
            if let Some((owner,result,out,journal))=emergency {
                let prior={result.borrow().kill_timer};let killed=prior.unwrap_or_else(||owner.disarm());
                {let mut result=result.borrow_mut();if result.kill_timer.is_none(){result.kill_timer=Some(killed);}result.failure=Some("contained TIMERPROC panic; no acceptance".into());}
                write_once(&out.join("timer-boundary-failure.json"),&format!(r#"{{"status":"contained-TIMERPROC-panic-no-acceptance","actual_kill_timer":{killed},"no_rescue_input":true}}"#));
                journal.error();
            }
        }));
    }
}

fn reload_input(app:&AppRef,h:&NativeContextProbeHandles,tab:TabId,proof:(u64,u64,u64),out:&Path) {
    current(h,proof);ordinary_profile(&h.view);isolated_environment(app,h);
    let owner=app.borrow().hwnd;
    let mut class=[0;64];let n=unsafe{GetClassNameW(owner,&mut class)};
    assert_eq!(String::from_utf16_lossy(&class[..n as usize]),"BowserNativeWindow");
    assert!(unsafe{IsWindow(Some(owner))}.as_bool());
    // Complete DOM/geometry/observer preparation before the one native activation below.
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
    let observed=Rc::new(RefCell::new(Vec::new()));let sink=observed.clone();
    let failure=Rc::new(RefCell::new(None::<String>));let errors=failure.clone();
    let show=Rc::new(RefCell::new(None::<ShowWitness>));let pending_show=show.clone();
    let request_path=request_file.clone();let token=nonce.clone();let expected=h.view.as_raw() as usize;
    let controller=h.controller.as_raw() as usize;
    SHOW_SINK.with(|slot|{assert!(slot.borrow().is_none());*slot.borrow_mut()=Some(Rc::new(move |witness| {
        if pending_show.borrow().is_some()||witness.owner!=owner||witness.tab!=tab||witness.view!=expected
            ||witness.controller!=controller||witness.proof!=proof||witness.fixture_command<=0 {
            *errors.borrow_mut()=Some("wrong/repeated exact coordinator show witness".into());return;
        }
        let value=format!(r#"{{"schema":"bowser-real-app-menu-reload-v1","nonce":"{token}","phase":"before-show","owner":{},"tab":"{}","request":"{}","view_identity":"{}","proof0":"{}","proof1":"{}","proof2":"{}","fixture_command":{},"owner_token":"{}","request_epoch":"{}","menu_handle":{}}}"#,
            owner.0 as isize,tab.0,witness.request,witness.view,proof.0,proof.1,proof.2,witness.fixture_command,witness.owner_token,witness.request_epoch,witness.hmenu);
        *pending_show.borrow_mut()=Some(witness);write_once(&request_path,&value);
    }));});
    let completing=show.clone();let completion_path=out.join("actual-completion.json");let completion_nonce=nonce.clone();
    install_observer(Rc::new(move |event| {
        // Serialize only actual production completion counters after real Complete/cleanup.
        if let MenuProbeEvent::Completed{request,selected,setter_calls,setter_result,complete_calls,complete_error,refused,cleanup_ok}=event {
            let witness={completing.borrow().clone()};
            if let Some(witness)=witness.filter(|witness|witness.request==request) {
                let selected=selected.map(|value|value.to_string()).unwrap_or_else(||"null".into());
                let setter=match setter_result {None=>"null".into(),Some(Ok(()))=>"\"native-success\"".into(),Some(Err(error))=>error.to_string()};
                let error=complete_error.map(|value|value.to_string()).unwrap_or_else(||"null".into());
                write_once(&completion_path,&format!(r#"{{"schema":"bowser-real-app-menu-reload-complete-v1","nonce":"{completion_nonce}","request":"{request}","owner":{},"owner_token":"{}","request_epoch":"{}","menu_handle":{},"selected":{selected},"setter_calls":{setter_calls},"setter_result":{setter},"complete_calls":{complete_calls},"complete_error":{error},"refused":{refused},"cleanup_ok":{cleanup_ok}}}"#,witness.owner.0 as isize,witness.owner_token,witness.request_epoch,witness.hmenu));
            }
        }
        sink.borrow_mut().push(event);
    }));
    let _scope=ProbeScope;
    let journal=ReloadJournal::open(&out.join("reload-observations.json"));
    let _=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let old=JOURNAL.with(|slot|slot.try_borrow_mut().ok().and_then(|mut slot|slot.replace(journal.clone())));drop(old);
    }));
    let timer_result=Rc::new(RefCell::new(TimerResult::default()));
    let _timer=arm_timer(app,h,tab,proof,show.clone(),out,&nonce,timer_result.clone(),number(&source,"trusted"),journal);
    current(h,proof);
    // All receipt/script/observer setup is now complete. This remains the ONLY activation;
    // neither this Rust path nor the right-click-only helper may retry or choose another HWND.
    unsafe{assert!(SetForegroundWindow(owner).as_bool(),"exact-owner activation failed; no retry or helper rescue");h.controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC).unwrap()};
    assert!(pump_until(Duration::from_secs(3),||unsafe{GetForegroundWindow()}==owner));
    assert_eq!(script(&h.view,"document.hasFocus()&&document.activeElement===document.getElementById('app-context-target')"),Value::Bool(true));
    current(h,proof);
    let foreground=unsafe{GetForegroundWindow()};
    if foreground!=owner {super::record_foreground_refusal(out,owner,foreground,browser);}
    assert_eq!(foreground,owner,"exact owner displaced after final script; no retry or helper launch");
    write_once(&out.join("source-conditioning.json"),&format!(r#"{{"schema":"bowser-real-app-menu-reload-conditioning-v1","setup_complete_before_activation":true,"activation_owner":"Rust-exact-original-App-owner","activation_attempts":1,"actual_activation_return":true,"owner":{},"foreground_after_final_script":{},"controller":"{}","original_proof":["{}","{}","{}"]}}"#,owner.0 as isize,foreground.0 as isize,h.controller.as_raw() as usize,proof.0,proof.1,proof.2));
    assert_eq!(unsafe{GetForegroundWindow()},owner,"exact owner displaced before helper launch; no retry");
    let mut child=Command::new("powershell.exe").args(["-NoProfile","-File"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/scripts/probe-app-context-menu-reload.ps1"))
        .args(["-OwnerPid",&std::process::id().to_string(),"-OwnerHandle",&(owner.0 as isize).to_string()])
        .arg("-OwnerExe").arg(std::env::current_exe().unwrap())
        .args(["-BrowserPid",&browser.to_string(),"-ClientX",&px.to_string(),"-ClientY",&py.to_string(),"-ViewportWidth",&client.right.to_string(),"-ViewportHeight",&client.bottom.to_string(),"-ControlLeft",&left.to_string(),"-ControlTop",&top.to_string(),"-ControlRight",&right.to_string(),"-ControlBottom",&bottom.to_string(),"-ProbeNonce",&nonce,"-SourceViewIdentity",&expected.to_string(),"-Proof0",&proof.0.to_string(),"-Proof1",&proof.1.to_string(),"-Proof2",&proof.2.to_string()])
        .arg("-RequestReceipt").arg(&request_file).arg("-Out").arg(out)
        .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    if !pump_until(Duration::from_secs(25),||child.try_wait().is_ok_and(|status|status.is_some())) {
        let _=child.kill();let _=child.wait();panic!("Reload helper timed out; no acceptance");
    }
    let output=child.wait_with_output().unwrap();
    println!("APP_CONTEXT_MENU_RELOAD input_helper_exit={:?} stdout={} stderr={}",output.status.code(),String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr));
    std::fs::write(out.join("timer-result.txt"),format!("{:?}\n",timer_result.borrow())).unwrap();
    assert!(output.status.success(),"right-click-only helper unavailable/failed; no Reload acceptance");
    assert!(failure.borrow().is_none(),"{:?}",failure.borrow());
    {let result=timer_result.borrow();assert_eq!(result.reload_calls,1);assert_eq!(result.kill_timer,Some(true));assert!(result.popup.is_some());assert!(result.failure.is_none());}
    assert!(!TIMER_BOUNDARY_FAILED.with(Cell::get),"TIMERPROC boundary panic was contained, not acceptance");
    assert!(TIMER_STATE.with(|slot|slot.borrow().is_none()),"Reload timer state must remain disarmed");
    let witness=show.borrow().clone().expect("exact production request/HMENU witness absent");
    assert!(!(witness.current)(),"original request must never regain dispatch authority after Reload");
    let events=observed.borrow();assert_eq!(events.len(),2,"one real before-show and one actual Complete are mandatory");
    let MenuProbeEvent::BeforeShow{request,tab:actual,view_identity,proof:actual_proof,fixture_command}=events[0] else{panic!("actual before-show absent")};
    assert_eq!(request,witness.request);assert_eq!(actual,tab);assert_eq!(view_identity,expected);assert_eq!(actual_proof,proof);assert_eq!(fixture_command,witness.fixture_command);
    let MenuProbeEvent::Completed{request:completed,selected,setter_calls,setter_result,complete_calls,complete_error,cleanup_ok,..}=events[1] else{panic!("matching actual completion absent")};
    assert_eq!(completed,request);assert_eq!(selected,None);assert_eq!(setter_calls,0);assert_eq!(setter_result,None);
    assert_eq!(complete_calls,1);assert_eq!(complete_error,None);assert!(cleanup_ok,"actual explicit HMENU cleanup failed");drop(events);
    let native=json::parse(&std::fs::read_to_string(out.join("app-context-menu.json")).unwrap()).unwrap();
    assert_eq!(text(&native,"status"),"observed-real-App-owned-popup-spontaneous-reload-disappearance-and-actual-completion");
    assert_bool(&native,"right_input_called",true);assert_bool(&native,"escape_called",false);assert_bool(&native,"selection_called",false);
    assert_bool(&native,"spontaneous_disappearance",true);assert_eq!(field(&native,"selected_command_id"),Some(&Value::Null));
    // The old pinned document fence is NOT used as a post-Reload prerequisite or replaced.
    assert!(!(h.document_fence)(),"Reload must retire the original successful-document fence");
}

fn deny_all(receipt:&Value)->Vec<i64> {
    assert!(array(receipt,"events").is_empty(),"genuine onClicked must remain absent");
    let denials=array(receipt,"preflight");assert!(denials.len()>=2&&denials.len()<=16);
    let mut ids=Vec::new();
    for denial in denials {
        let id=number(denial,"id");assert!(id>=0);assert_bool(denial,"allowed",false);assert_bool(denial,"permissionDenied",true);
        assert!(!text(denial,"error").is_empty());ids.push(id);
    }
    ids.sort_unstable();assert!(ids.windows(2).all(|pair|pair[0]!=pair[1]),"native returned IDs must be unique");ids
}
fn no_effects(view:&ICoreWebView2) {for role in ["primary","negative"] {assert_eq!(marker(view,role),Value::Null);}}
fn sequence(app:&AppRef,root:&Path,page:&url::Url,bowser_id:&str,folder:&Path,identities:&Rc<RefCell<Vec<(TabId,usize)>>>) {
    let source_tab=App::active_tab_id(app);let mut primary=None;
    assert!(pump_until(Duration::from_secs(60),||{primary=handles(app,source_tab);primary.as_ref().is_some_and(|h|(h.proof)().is_some())}));
    let primary=primary.unwrap();let original=(primary.proof)().unwrap();ordinary_profile(&primary.view);isolated_environment(app,&primary);
    let before_doc=document(&primary.view,page);current(&primary,original);
    assert!(pump_until(Duration::from_secs(30),||std::fs::read_to_string(root.join("extension_ids.txt")).is_ok_and(|mapping|mapping.lines().any(|line|line==format!("{bowser_id}\t{FIXTURE_ID}")))));authority(root,bowser_id);
    let other_tab=App::new_tab(app);App::submit(app,page.as_str());let mut negative=None;
    assert!(pump_until(Duration::from_secs(30),||{negative=handles(app,other_tab);negative.as_ref().is_some_and(|h|(h.proof)().is_some())}));
    let negative=negative.unwrap();let negative_doc=document(&negative.view,page);
    assert_ne!(primary.view.as_raw(),negative.view.as_raw());
    {let mut ids=identities.borrow_mut();ids.push((source_tab,primary.view.as_raw() as usize));ids.push((other_tab,negative.view.as_raw() as usize));}
    assert!(App::select_tab(app,source_tab));current(&primary,original);
    App::open_extension_page(app,bowser_id.into(),folder.into(),extensions::PageKind::Popup,"Disposable Reload fixture receipt".into());
    let mut popup=None;assert!(pump_until(Duration::from_secs(30),||{popup=receipt_view(app);popup.is_some()}));let popup=popup.unwrap();
    assert!(pump_until(Duration::from_secs(20),||script(&popup,"document.documentElement.getAttribute('data-ready')")==Value::String("yes".into())));
    let mut ready=None;assert!(pump_until(Duration::from_secs(10),||{let receipt=worker(&popup,"native-command-state");let complete=field(&receipt,"creation").is_some_and(|creation|field(creation,"complete")==Some(&Value::Bool(true)));ready=Some(receipt);complete}));
    let ready=ready.unwrap();let worker_id=text(&ready,"workerInstance").to_owned();let creation=field(&ready,"creation").unwrap().clone();
    assert_bool(&creation,"ok",true);assert_eq!(field(&creation,"error"),Some(&Value::Null));
    let before=worker(&popup,"native-command-preflight");assert_eq!(text(&before,"workerInstance"),worker_id);assert_eq!(field(&before,"creation"),Some(&creation));
    let native_ids=deny_all(&before);for view in [&primary.view,&negative.view] {no_effects(view);}
    let out=std::env::current_dir().unwrap().join(EVIDENCE);std::fs::write(out.join("genuine-native-preflight-before.txt"),format!("{before:?}\n")).unwrap();
    assert!(page.as_str().len()<=MAX_START_URL_BYTES,"controlled expected source URL exceeds capture bound");
    START_SOURCE.with(|slot|*slot.borrow_mut()=Some((source_tab,primary.view.as_raw() as usize,page.to_string())));
    let start_offset=STARTS.with(|events|events.borrow().len());assert!(start_offset<64);
    reload_input(app,&primary,source_tab,original,&out.join("reload"));
    let mut fresh=None;
    assert!(pump_until(Duration::from_secs(30),||{fresh=(primary.proof)();fresh.is_some_and(|proof|proof!=original)}),"same retained controller/gate did not produce a fresh successful native document proof");
    let fresh=fresh.unwrap();assert_ne!(fresh.0,original.0);assert!(!(primary.document_fence)());
    assert!(!STARTS_SATURATED.with(Cell::get)&&!STARTS_INVALID.with(Cell::get),"native-start capture saturated or source/URL length invalid");
    let starts=STARTS.with(|events|events.borrow().clone());assert!(starts.len()<=64);
    let source_starts=starts[start_offset..].iter().filter(|start|start.tab==source_tab).collect::<Vec<_>>();
    assert_eq!(source_starts.len(),1,"exactly one actual native NavigationStarting is required, not proof-only inference");
    let start=source_starts[0];assert_eq!(start.view,primary.view.as_raw() as usize);assert_ne!(start.id,original.0);assert_ne!(start.id,0);
    assert_eq!(start.id,fresh.0,"fresh successful proof must bind to the independently observed actual NavigationStarting ID");assert!(start.url_matches);assert_eq!(start.url_length,page.as_str().len());
    let after_doc=document(&primary.view,page);assert_ne!(field(&after_doc,"timeOrigin"),field(&before_doc,"timeOrigin"),"same URL must have a genuinely new document timeOrigin");
    assert_eq!((primary.proof)(),Some(fresh));assert_eq!(App::active_tab_id(app),source_tab);ordinary_profile(&primary.view);isolated_environment(app,&primary);
    // Fixed quiet window: bounded non-delivery/no-grant evidence, not an indefinite claim.
    pump_until(Duration::from_secs(2),||false);
    let after=worker(&popup,"native-command-preflight");assert_eq!(text(&after,"workerInstance"),worker_id);assert_eq!(field(&after,"creation"),Some(&creation));
    assert_eq!(deny_all(&after),native_ids,"retain stable genuinely returned query IDs, without attributing any to a COM view");
    std::fs::write(out.join("genuine-native-preflight-after.txt"),format!("{after:?}\n")).unwrap();
    let quiet=worker(&popup,"native-command-state");assert_eq!(text(&quiet,"workerInstance"),worker_id);assert_eq!(field(&quiet,"creation"),Some(&creation));assert!(array(&quiet,"events").is_empty());
    for view in [&primary.view,&negative.view] {no_effects(view);}
    assert_eq!(document(&primary.view,page),after_doc);assert_eq!((primary.proof)(),Some(fresh));
    assert_eq!(document(&negative.view,page),negative_doc);authority(root,bowser_id);
    let ids=native_ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    write_once(&out.join("reload-summary.json"),&format!(r#"{{"scope":"actual-App-fresh-profile-one-right-click-then-ordinary-same-URL-Reload","native_start_id":"{}","fresh_successful_proof":["{}","{}","{}"],"same_url_new_time_origin":true,"zero_native_setters":true,"actual_complete_and_cleanup":true,"spontaneous_popup_disappearance_capture_release":true,"no_event_quiet_window_ms":2000,"stable_worker":true,"permission_specific_denied_native_query_ids":[{ids}],"native_ID_to_source_COM_mapping":"not-inferred","exact_source_and_negative_effects":"unchanged-null","negative_document":"unchanged","other_navigation_close_shutdown_races":"not-run","marketplace_compatibility":"not-claimed"}}"#,start.id,fresh.0,fresh.1,fresh.2));
    println!("APP_CONTEXT_MENU_RELOAD actual_App=true one_right_click_no_further_input=true spontaneous_disappearance=true actual_native_start_new_id=true same_URL_new_timeOrigin=true zero_setters=true actual_complete_cleanup=true native_query_ids_denied_no_source_ID_inference=true bounded_no_event_ms=2000");
}

#[test]
#[ignore="requires consented fresh profile, actual App::run, interactive Windows, one physical right-click and spontaneous in-modal Reload cancellation"]
fn live_probe_real_app_context_menu_same_url_reload() {
    let args=std::env::args().skip(1).collect::<Vec<_>>();
    assert_eq!(args.iter().map(String::as_str).collect::<Vec<_>>(),vec![RELOAD_TEST_NAME,"--exact","--ignored","--nocapture","--test-threads=1"],"must run alone: failure watchdog may terminate ONLY this exact retained single-test executable");
    assert_eq!(std::env::var("BOWSER_APP_MENU_RELOAD_PROBE_CONSENT").as_deref(),Ok(RELOAD_CONSENT),"explicit consent to the exact retained fixture is required");
    assert!(crate::profile::data_root_override().is_none(),"never reuse an initialized process profile");
    assert!(std::env::var_os("BOWSER_DATA_DIR").is_none(),"do not inherit another profile override");
    let stamp=SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let root=std::env::temp_dir().join(format!("bowser-real-app-menu-reload-{}-{stamp}",std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let evidence=std::env::current_dir().unwrap().join(EVIDENCE);
    assert!(!evidence.exists(),"do not reuse a prior diagnostic run");std::fs::create_dir(&evidence).unwrap();
    let server=Loopback::start();
    let options=LaunchOptions::parse_from(vec!["--dev".into(),"--data-dir".into(),root.to_string_lossy().into_owned(),"--open-url".into(),server.url.to_string()],&Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/fixtures")).unwrap();
    assert!(options.dev&&!options.guest&&!options.agent&&options.benchmark_log.is_none());assert_eq!(options.data_dir.as_deref(),Some(root.as_path()));assert!(options.user_data_dir.is_none());
    // App::run does not do main.rs's process setup: mirror its data-root/dev/COM steps here.
    // Never override WEBVIEW2_* or touch updater/accounts/real installed Bowser profiles.
    crate::profile::set_data_root(root.clone()).unwrap();crate::profile::set_dev_launch(true);
    let record=extensions::install(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/extensions/probe-custom-context-menu"),&root.join("Extensions"),&root.join("extensions.txt")).unwrap();
    let folder=extensions::folder_for(&root.join("Extensions"),&record.id).unwrap();
    CAPTURE.with(|capture|{assert!(!capture.replace(true));});
    STARTS.with(|events|events.borrow_mut().clear());STARTS_SATURATED.with(|failed|failed.set(false));STARTS_INVALID.with(|failed|failed.set(false));
    START_SOURCE.with(|slot|{slot.borrow_mut().take();});REMOVALS.with(|events|events.borrow_mut().clear());
    DEREGISTRATIONS.with(|events|events.borrow_mut().clear());install_observer(Rc::new(|_|{}));
    let identities=Rc::new(RefCell::new(Vec::new()));let source_identities=identities.clone();
    let outcome=Rc::new(RefCell::new(None));let sink=outcome.clone();let destination=root.clone();let page=server.url.clone();
    let started=Instant::now();let timed_out=Rc::new(Cell::new(false));let expired=timed_out.clone();
    crate::win32::set_app_menu_probe_hooks(Box::new(move |app|{
        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||sequence(&app,&destination,&page,&record.id,&folder,&source_identities)));
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
    let removed=REMOVALS.with(|events|events.borrow().clone());
    assert!(removed.len()<64,"native deregistration capture saturated");
    let identities=identities.borrow();assert_eq!(identities.len(),2);
    for (tab,view) in identities.iter() {
        let matching=removed.iter().filter(|(actual_tab,actual_view,_)|actual_tab==tab&&actual_view==view).collect::<Vec<_>>();
        assert_eq!(matching.len(),1,"exact source/negative native ContextMenuRequested registration must really retire once");
        assert_eq!(matching[0].2,None,"actual exact-view native deregistration failed");
    }
    assert!(removed.len()>=2,"actual source and negative page handler deregistration receipts are required");
    assert!(removed.iter().all(|entry|entry.2.is_none()),"real native handler deregistration failed: {removed:?}");
    let records=removed.iter().map(|(tab,view,error)|format!(r#"{{"tab":"{}","view_identity":"{view}","native_error":{}}}"#,tab.0,error.map(|error|error.to_string()).unwrap_or_else(||"null".into()))).collect::<Vec<_>>().join(",");
    write_once(&std::env::current_dir().unwrap().join(EVIDENCE).join("native-handler-teardown.json"),&format!(r#"{{"status":"observed-native-removal-success","exact_source_negative_removal_once":true,"native_handler_receipts":{},"actual_native_removals":[{records}]}}"#,removed.len()));
    CAPTURE.with(|capture|capture.set(false));
    println!("APP_CONTEXT_MENU_RELOAD exact_native_source_negative_handler_teardown=true disposable_profile_retained_for_evidence={}",root.display());
}
