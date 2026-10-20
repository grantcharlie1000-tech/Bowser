//! Separate, test-armed actual-App native Copy/Paste and host-action regression lane.
//! Never installs handlers, creates engine items, invokes callbacks, or sets a command ID.
//! Unknown native inventory entries and unknown menu nodes remain complete and unchanged.
use std::{cell::RefCell, rc::Rc};
use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2ContextMenuItem, ICoreWebView2ContextMenuRequestedEventArgs};
use windows::core::{IUnknown, Interface};
use super::app_context_menu_probe::MenuProbeEvent;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HostRole { InsertNoteParent, InsertNoteChild, SplitLink }
impl HostRole { pub(crate) fn name(self)->&'static str {match self {Self::InsertNoteParent=>"insert-note-parent",Self::InsertNoteChild=>"insert-note-child",Self::SplitLink=>"split-link"}} }
#[derive(Clone)]
pub(crate) struct NodeWitness {
    pub(crate) path: Vec<usize>, pub(crate) collection: usize, pub(crate) identity: usize,
    pub(crate) kind: i32, pub(crate) command: i32, pub(crate) enabled: bool,
    pub(crate) checked: Option<bool>, pub(crate) name: String,
    pub(crate) rendered_fingerprint: String, pub(crate) rendered_units: usize,
    pub(crate) host_role: Option<HostRole>, pub(crate) children: Vec<NodeWitness>,
}
#[derive(Clone)]
pub(crate) enum HostProbeEvent {
    BeforeShow {request: usize, tab: crate::tabs::TabId, view: usize, args: usize,
        proof: (u64,u64,u64), nodes: Vec<NodeWitness>},
    Lifecycle(MenuProbeEvent),
}
struct Arm {observer:Rc<dyn Fn(HostProbeEvent)>, args:Option<IUnknown>, inserted:Vec<(IUnknown,HostRole)>}
thread_local! {static ARM:RefCell<Option<Arm>> = const {RefCell::new(None)};}
pub(crate) fn set_observer(observer: Option<Rc<dyn Fn(HostProbeEvent)>>) {
    let next=observer.map(|observer|Arm{observer,args:None,inserted:Vec::new()});
    let old=ARM.with(|arm|std::mem::replace(&mut *arm.borrow_mut(),next));
    drop(old);
}
pub(crate) fn armed()->bool {ARM.with(|arm|arm.borrow().is_some())}
pub(crate) fn begin(args:&ICoreWebView2ContextMenuRequestedEventArgs)->windows::core::Result<()> {
    if !armed(){return Ok(())} let identity=args.cast::<IUnknown>()?;
    let mut captured=Some(identity);
    let retired=ARM.with(|arm|{
        let mut slot=arm.borrow_mut();
        slot.as_mut().map(|arm|(std::mem::replace(&mut arm.args,captured.take()),std::mem::take(&mut arm.inserted)))
    });
    drop(retired);drop(captured);Ok(())
}
/// Called only AFTER the actual production InsertValueAtIndex succeeds. Labels and numeric
/// ranges cannot assign roles. The exact canonical args/item references are retained per request.
pub(crate) fn inserted(args:&ICoreWebView2ContextMenuRequestedEventArgs,item:&ICoreWebView2ContextMenuItem,role:HostRole) {
    if !armed(){return} let (Ok(args),Ok(item))=(args.cast::<IUnknown>(),item.cast::<IUnknown>()) else{return};
    let mut captured=Some((item,role));
    ARM.with(|arm|if let Some(arm)=arm.borrow_mut().as_mut(){
        if arm.args.as_ref().is_some_and(|expected|expected.as_raw()==args.as_raw()) && arm.inserted.len()<256 {arm.inserted.push(captured.take().unwrap());}
    });
    drop(captured);
}
pub(crate) fn role(identity:&IUnknown)->Option<HostRole> {
    ARM.with(|arm|arm.borrow().as_ref().and_then(|arm|{
        let mut roles=arm.inserted.iter().filter(|(item,_)|item.as_raw()==identity.as_raw());
        let role=roles.next().map(|(_,role)|*role)?;roles.next().is_none().then_some(role)
    }))
}
pub(crate) fn observe(event:HostProbeEvent)->bool {
    let observer=ARM.with(|arm|arm.borrow().as_ref().map(|arm|arm.observer.clone()));
    observer.is_none_or(|observer|std::panic::catch_unwind(std::panic::AssertUnwindSafe(||observer(event))).is_ok())
}

#[cfg(test)]
mod live;
