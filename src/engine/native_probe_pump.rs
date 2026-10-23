//! Test-only native pump scheduling and bounded, in-memory call checkpoints.

use std::{cell::Cell,time::{Duration,Instant}};
const BURST:usize=32;
const DEPTH:usize=8;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum Phase { Idle,ReadyEnter,ReadyReturn,PeekEnter,PeekReturn,DispatchEnter,DispatchReturn }
impl Phase {
    fn name(self)->&'static str {match self {Self::Idle=>"idle",Self::ReadyEnter=>"ready-enter",Self::ReadyReturn=>"ready-return",Self::PeekEnter=>"peek-enter",Self::PeekReturn=>"peek-return",Self::DispatchEnter=>"dispatch-enter",Self::DispatchReturn=>"dispatch-return"}}
}
#[derive(Clone,Copy)]
struct Frame {
    id:u64,phase:Phase,outcome:&'static str,
    ready_enter:u64,ready_return:u64,peek_enter:u64,peek_return:u64,dispatch_enter:u64,dispatch_return:u64,
}
impl Frame {
    const EMPTY:Self=Self{id:0,phase:Phase::Idle,outcome:"unused",ready_enter:0,ready_return:0,peek_enter:0,peek_return:0,dispatch_enter:0,dispatch_return:0};
    fn json(self)->String {format!(r#"{{"id":{},"phase":"{}","outcome":"{}","ready_enter":{},"ready_return":{},"peek_enter":{},"peek_return":{},"dispatch_enter":{},"dispatch_return":{}}}"#,self.id,self.phase.name(),self.outcome,self.ready_enter,self.ready_return,self.peek_enter,self.peek_return,self.dispatch_enter,self.dispatch_return)}
}
#[derive(Clone,Copy)]
struct State {frames:[Frame;DEPTH],depth:usize,next:u64,overflows:u64,last:Option<Frame>}
thread_local! {static STATE:Cell<State>=const {Cell::new(State{frames:[Frame::EMPTY;DEPTH],depth:0,next:0,overflows:0,last:None})};}
fn active_frame()->Option<Frame>{STATE.with(|slot|{let s=slot.get();s.depth.checked_sub(1).map(|i|s.frames[i])})}
fn last_frame()->Option<Frame>{STATE.with(|slot|slot.get().last)}
pub(crate) fn diagnostic_json()->String {
    let state=STATE.with(Cell::get);
    let active=state.frames[..state.depth].iter().map(|f|f.json()).collect::<Vec<_>>().join(",");
    let last=state.last.map(Frame::json).unwrap_or_else(||"null".into());
    format!(r#"{{"scope":"bounded-in-memory-pump-checkpoints-not-authority","frame_bound":{DEPTH},"burst_bound":{BURST},"overflows":{},"active":[{active}],"last":{last}}}"#,state.overflows)
}
struct Scope {index:usize}
impl Scope {
    fn enter()->Option<Self>{STATE.with(|slot|{
        let mut s=slot.get();
        if s.depth==DEPTH{s.overflows=s.overflows.saturating_add(1);slot.set(s);return None;}
        let index=s.depth;s.depth+=1;s.next=s.next.saturating_add(1);
        s.frames[index]=Frame{id:s.next,outcome:"running",..Frame::EMPTY};slot.set(s);Some(Self{index})
    })}
    fn mark(&self,phase:Phase){STATE.with(|slot|{
        let mut s=slot.get();let f=&mut s.frames[self.index];f.phase=phase;
        let count=match phase {Phase::ReadyEnter=>&mut f.ready_enter,Phase::ReadyReturn=>&mut f.ready_return,Phase::PeekEnter=>&mut f.peek_enter,Phase::PeekReturn=>&mut f.peek_return,Phase::DispatchEnter=>&mut f.dispatch_enter,Phase::DispatchReturn=>&mut f.dispatch_return,Phase::Idle=>{slot.set(s);return;}};
        *count=count.saturating_add(1);slot.set(s);
    })}
    fn finish(self,outcome:&'static str)->bool {STATE.with(|slot|{let mut s=slot.get();s.frames[self.index].outcome=outcome;slot.set(s)});outcome=="ready"}
}
impl Drop for Scope {fn drop(&mut self){STATE.with(|slot|{let mut s=slot.get();s.last=Some(s.frames[self.index]);s.depth=self.index;slot.set(s)});}}

trait Driver {
    type Message;
    fn elapsed(&self)->Duration;
    fn peek(&mut self)->Option<Self::Message>;
    fn quit(message:&Self::Message)->Option<i32>;
    fn repost_quit(&mut self,code:i32);
    fn dispatch(&mut self,message:Self::Message);
    fn pause(&mut self,remaining:Duration);
}
fn drive<P:Driver>(budget:Duration,mut ready:impl FnMut()->bool,driver:&mut P)->bool {
    let Some(scope)=Scope::enter() else{return false};
    loop {
        if driver.elapsed()>=budget{return scope.finish("expired");}
        scope.mark(Phase::ReadyEnter);let done=ready();scope.mark(Phase::ReadyReturn);
        if driver.elapsed()>=budget{return scope.finish("expired");}
        if done{return scope.finish("ready");}
        // A permanently nonempty queue cannot prevent another readiness/deadline check.
        for _ in 0..BURST {
            if driver.elapsed()>=budget{return scope.finish("expired");}
            scope.mark(Phase::PeekEnter);let message=driver.peek();scope.mark(Phase::PeekReturn);
            // PeekMessage may itself dispatch nonqueued callbacks. Every removed message
            // still needs its exact-once disposition, even if that callback exhausted the
            // budget. Dispatch ordinary messages in place; never repost pointer-bearing MSG.
            if let Some(code)=message.as_ref().and_then(P::quit){driver.repost_quit(code);return scope.finish("quit");}
            let Some(message)=message else{
                if driver.elapsed()>=budget{return scope.finish("expired");}
                break;
            };
            scope.mark(Phase::DispatchEnter);driver.dispatch(message);scope.mark(Phase::DispatchReturn);
            if driver.elapsed()>=budget{return scope.finish("expired");}
        }
        let elapsed=driver.elapsed();if elapsed>=budget{return scope.finish("expired");}
        scope.mark(Phase::Idle);driver.pause((budget-elapsed).min(Duration::from_millis(10)));
    }
}

struct Native {started:Instant}
impl Driver for Native {
    type Message=windows::Win32::UI::WindowsAndMessaging::MSG;
    fn elapsed(&self)->Duration{self.started.elapsed()}
    fn peek(&mut self)->Option<Self::Message>{unsafe{
        use windows::Win32::UI::WindowsAndMessaging::{MSG,PM_REMOVE,PeekMessageW};
        let mut message=MSG::default();PeekMessageW(&mut message,None,0,0,PM_REMOVE).as_bool().then_some(message)
    }}
    fn quit(message:&Self::Message)->Option<i32>{(message.message==windows::Win32::UI::WindowsAndMessaging::WM_QUIT).then_some(message.wParam.0 as i32)}
    fn repost_quit(&mut self,code:i32){unsafe{windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(code)}}
    fn dispatch(&mut self,message:Self::Message){unsafe{let _=windows::Win32::UI::WindowsAndMessaging::TranslateMessage(&message);windows::Win32::UI::WindowsAndMessaging::DispatchMessageW(&message);}}
    fn pause(&mut self,remaining:Duration){std::thread::sleep(remaining)}
}
pub(crate) fn until(budget:Duration,ready:impl FnMut()->bool)->bool {
    // Native calls cannot be preempted here. Check after every callback and refuse late
    // readiness; the checkpoints identify which call did not return within the budget.
    drive(budget,ready,&mut Native{started:Instant::now()})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell,rc::Rc};
    enum Message { Ordinary, Quit(i32) }
    struct Fake {
        clock:Rc<Cell<u64>>,busy:bool,quit:Option<i32>,peek_delay:u64,dispatch_delay:u64,
        peeks:usize,dispatched:usize,reposted:Option<i32>,peek_checkpoint:bool,dispatch_checkpoint:bool,
    }
    impl Fake {
        fn new(clock:Rc<Cell<u64>>)->Self {Self{clock,busy:true,quit:None,peek_delay:0,dispatch_delay:0,peeks:0,dispatched:0,reposted:None,peek_checkpoint:false,dispatch_checkpoint:false}}
    }
    impl Driver for Fake {
        type Message=Message;
        fn elapsed(&self)->Duration {Duration::from_millis(self.clock.get())}
        fn peek(&mut self)->Option<Message>{
            self.peek_checkpoint=active_frame().is_some_and(|f|f.phase==Phase::PeekEnter);
            self.peeks+=1;self.clock.set(self.clock.get()+self.peek_delay);
            self.quit.take().map(Message::Quit).or_else(||self.busy.then_some(Message::Ordinary))
        }
        fn quit(message:&Message)->Option<i32>{match message{Message::Quit(code)=>Some(*code),_=>None}}
        fn repost_quit(&mut self,code:i32){self.reposted=Some(code);}
        fn dispatch(&mut self,_message:Message){
            self.dispatch_checkpoint=active_frame().is_some_and(|f|f.phase==Phase::DispatchEnter);
            self.dispatched+=1;self.clock.set(self.clock.get()+self.dispatch_delay);
        }
        fn pause(&mut self,remaining:Duration){self.clock.set(self.clock.get()+remaining.as_millis().min(10) as u64);}
    }
    #[test]
    fn continuously_busy_queue_cannot_starve_readiness(){
        let clock=Rc::new(Cell::new(0));let mut driver=Fake::new(clock);let mut checks=0;
        assert!(drive(Duration::from_secs(1),||{checks+=1;checks==2},&mut driver));
        assert_eq!(driver.dispatched,BURST);assert!(driver.peek_checkpoint&&driver.dispatch_checkpoint);
        let last=last_frame().unwrap();assert_eq!(last.ready_enter,2);assert_eq!(last.ready_return,2);
        assert_eq!(last.peek_enter,BURST as u64);assert_eq!(last.dispatch_return,BURST as u64);
    }
    #[test]
    fn readiness_returning_true_after_expiry_is_refused(){
        let clock=Rc::new(Cell::new(0));let observed=clock.clone();let mut driver=Fake::new(clock);
        assert!(!drive(Duration::from_millis(5),||{assert_eq!(active_frame().unwrap().phase,Phase::ReadyEnter);observed.set(6);true},&mut driver));
        assert_eq!(driver.peeks,0);assert_eq!(last_frame().unwrap().outcome,"expired");
    }
    #[test]
    fn peek_expiry_disposes_removed_message_once_without_late_readiness(){
        let clock=Rc::new(Cell::new(0));let mut driver=Fake::new(clock);driver.peek_delay=6;let mut checks=0;
        assert!(!drive(Duration::from_millis(5),||{checks+=1;false},&mut driver));
        assert_eq!(checks,1);assert_eq!(driver.peeks,1);assert_eq!(driver.dispatched,1);assert_eq!(driver.reposted,None);
        let last=last_frame().unwrap();assert_eq!((last.peek_enter,last.peek_return),(1,1));
        assert_eq!((last.dispatch_enter,last.dispatch_return),(1,1));assert_eq!(last.outcome,"expired");
    }
    #[test]
    fn peek_expiry_without_removed_message_never_dispatches_or_rechecks(){
        let clock=Rc::new(Cell::new(0));let mut driver=Fake::new(clock);driver.peek_delay=6;driver.busy=false;let mut checks=0;
        assert!(!drive(Duration::from_millis(5),||{checks+=1;false},&mut driver));
        assert_eq!(checks,1);assert_eq!(driver.peeks,1);assert_eq!(driver.dispatched,0);assert_eq!(driver.reposted,None);
    }
    #[test]
    fn dispatch_callback_expiry_is_refused_without_late_ready_check(){
        let clock=Rc::new(Cell::new(0));let mut driver=Fake::new(clock);driver.dispatch_delay=6;let mut checks=0;
        assert!(!drive(Duration::from_millis(5),||{checks+=1;false},&mut driver));
        assert_eq!(checks,1);assert_eq!(driver.dispatched,1);let last=last_frame().unwrap();assert_eq!((last.dispatch_enter,last.dispatch_return),(1,1));
    }
    #[test]
    fn quit_is_reposted_even_when_peek_callback_expires(){
        let clock=Rc::new(Cell::new(0));let mut driver=Fake::new(clock);driver.quit=Some(42);driver.peek_delay=6;
        assert!(!drive(Duration::from_millis(5),||false,&mut driver));assert_eq!(driver.reposted,Some(42));assert_eq!(driver.dispatched,0);
    }
    #[test]
    fn zero_budget_does_not_call_readiness_or_native_driver(){
        let clock=Rc::new(Cell::new(0));let mut driver=Fake::new(clock);
        assert!(!drive(Duration::ZERO,||panic!("late readiness must not run"),&mut driver));assert_eq!(driver.peeks,0);
    }
    #[test]
    fn idle_expiry_has_no_final_readiness_fallback(){
        let clock=Rc::new(Cell::new(0));let mut driver=Fake::new(clock);driver.busy=false;let mut checks=0;
        assert!(!drive(Duration::from_millis(5),||{checks+=1;checks>1},&mut driver));assert_eq!(checks,1);
    }
    #[test]
    fn maximum_counter_snapshot_remains_bounded_valid_json(){
        let saved=STATE.with(Cell::get);
        let full=Frame{id:u64::MAX,phase:Phase::PeekEnter,outcome:"running",ready_enter:u64::MAX,ready_return:u64::MAX,peek_enter:u64::MAX,peek_return:u64::MAX,dispatch_enter:u64::MAX,dispatch_return:u64::MAX};
        STATE.with(|slot|slot.set(State{frames:[full;DEPTH],depth:DEPTH,next:u64::MAX,overflows:u64::MAX,last:Some(full)}));
        let json=diagnostic_json();STATE.with(|slot|slot.set(saved));
        assert!(json.len()<8192);assert!(crate::extensions::json::parse(&json).is_some());
    }
}
