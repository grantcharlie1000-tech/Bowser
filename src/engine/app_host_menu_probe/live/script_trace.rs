//! Fixed primitive-only telemetry for existing test script calls. Never source or authority.
use super::*;
use super::super::super::app_context_menu_probe::StartupPump;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum ScriptPhase { ExecuteEntered, ExecuteReturned, PumpEntered, PumpExited }

pub(super) struct ScriptCall {
    ordinal:u64,label:&'static str,started:Instant,phase:Cell<ScriptPhase>,
    execute_elapsed:Cell<Option<u128>>,execute_ok:Cell<Option<bool>>,callback_seen:Cell<bool>,callback_ok:Cell<Option<bool>>,
    callback_elapsed:Cell<Option<u128>>,pump_ok:Cell<Option<bool>>,
    pub(super) pump:StartupPump,
}
impl ScriptCall {
    fn new(ordinal:u64,label:&'static str)->Self {
        Self {ordinal,label,started:Instant::now(),phase:Cell::new(ScriptPhase::ExecuteEntered),execute_elapsed:Cell::new(None),execute_ok:Cell::new(None),callback_seen:Cell::new(false),callback_ok:Cell::new(None),callback_elapsed:Cell::new(None),pump_ok:Cell::new(None),pump:StartupPump::for_timeout(Duration::from_secs(10))}
    }
    pub(super) fn callback(&self,ok:bool){self.callback_seen.set(true);self.callback_ok.set(Some(ok));self.callback_elapsed.set(Some(self.started.elapsed().as_millis()));}
    pub(super) fn execute_returned(&self,ok:bool){self.execute_elapsed.set(Some(self.started.elapsed().as_millis()));self.execute_ok.set(Some(ok));self.phase.set(ScriptPhase::ExecuteReturned);}
    pub(super) fn pump_entered(&self){self.phase.set(ScriptPhase::PumpEntered);}
    pub(super) fn pump_exited(&self,ok:bool){self.pump_ok.set(Some(ok));self.phase.set(ScriptPhase::PumpExited);}
    fn json(&self)->String {
        let optional=|v:Option<u128>|v.map_or_else(||"null".into(),|v|v.to_string());
        let boolean=|v:Option<bool>|v.map_or_else(||"null".into(),|v|v.to_string());
        format!(r#"{{"ordinal":{},"label":"{}","phase":"{:?}","elapsed_ms":{},"execute_returned_ms":{},"execute_ok":{},"callback_seen":{},"callback_ok":{},"callback_elapsed_ms":{},"pump_ok":{},"pump":{}}}"#,
            self.ordinal,self.label,self.phase.get(),self.started.elapsed().as_millis(),optional(self.execute_elapsed.get()),boolean(self.execute_ok.get()),self.callback_seen.get(),boolean(self.callback_ok.get()),optional(self.callback_elapsed.get()),boolean(self.pump_ok.get()),self.pump.diagnostic_json())
    }
}

pub(super) struct ScriptTrace {ordinal:Cell<u64>,active:Cell<Option<Rc<ScriptCall>>>,reports:Cell<u8>}
impl ScriptTrace {
    pub(super) fn new()->Self {Self {ordinal:Cell::new(0),active:Cell::new(None),reports:Cell::new(0)}}
    pub(super) fn start(&self,label:&'static str)->Rc<ScriptCall> {
        let ordinal=self.ordinal.get().saturating_add(1);self.ordinal.set(ordinal);
        let call=Rc::new(ScriptCall::new(ordinal,label));self.active.set(Some(call.clone()));call
    }
    fn snapshot(&self)->Option<String>{let call=self.active.take();let result=call.as_ref().map(|call|call.json());self.active.set(call);result}
    pub(super) fn report(&self,reason:&'static str){
        if self.reports.get()>=4{return}self.reports.set(self.reports.get()+1);
        let _=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{
            let value=self.snapshot().unwrap_or_else(||"null".into());
            let line=format!(r#"HOST_SCRIPT_DIAGNOSTIC {{"scope":"failure-only-fixed-operation-not-source-or-authority","reason":"{reason}","call":{value}}}"#);
            if line.len()<=2048 {let _=writeln!(std::io::stdout(),"{line}");}
        }));
    }
}
thread_local! {static ACTIVE:Cell<Option<Rc<ScriptTrace>>>=const{Cell::new(None)};}
pub(super) fn active()->Option<Rc<ScriptTrace>>{ACTIVE.with(|slot|{let value=slot.take();let copy=value.clone();slot.set(value);copy})}
pub(super) struct TraceScope {previous:Option<Rc<ScriptTrace>>}
impl TraceScope {pub(super) fn enter(trace:Rc<ScriptTrace>)->Self {Self {previous:ACTIVE.with(|slot|slot.replace(Some(trace)))}}}
impl Drop for TraceScope {fn drop(&mut self){ACTIVE.with(|slot|slot.set(self.previous.take()));}}

#[test]
fn synthetic_script_trace_preserves_synchronous_callback_before_execute_return(){
    let trace=ScriptTrace::new();let call=trace.start("fixture-snapshot");call.callback(true);
    assert_eq!(call.phase.get(),ScriptPhase::ExecuteEntered);assert!(call.callback_seen.get());
    call.execute_returned(true);call.pump_entered();call.pump_exited(true);
    let value=json::parse(&trace.snapshot().unwrap()).unwrap();
    assert_eq!(text(&value,"label"),"fixture-snapshot");assert_eq!(number(&value,"ordinal"),1);
    assert_bool(&value,"callback_seen",true);assert_bool(&value,"callback_ok",true);assert_bool(&value,"pump_ok",true);
    assert_eq!(text(&value,"phase"),"PumpExited");
}
#[test]
fn synthetic_script_trace_late_old_callback_cannot_relabel_active_call(){
    let trace=ScriptTrace::new();let old=trace.start("fixture-snapshot");let current=trace.start("fixture-geometry");old.callback(false);
    let value=json::parse(&trace.snapshot().unwrap()).unwrap();
    assert_eq!(text(&value,"label"),"fixture-geometry");assert_eq!(number(&value,"ordinal"),2);assert_bool(&value,"callback_seen",false);
    assert_eq!(current.phase.get(),ScriptPhase::ExecuteEntered);assert!(field(&value,"callback_ok")==Some(&Value::Null));
}
