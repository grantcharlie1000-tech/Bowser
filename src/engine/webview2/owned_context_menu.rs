//! Default-off generic presentation of the exact ordinary WebView's engine-owned menu.
//! No menu text, item category, or destination is signer/extension identity. The runtime lease
//! fences cooperating Bowser changes only; this is not an extension API adapter or a new grant.
use super::*;
use super::engine_menu::{EngineMenu, MenuSelection};
use super::menu_inventory::{MenuInventoryAuthority, NativeInventory};
use std::rc::Weak;
use windows::Win32::{Foundation::POINT, Graphics::Gdi::ClientToScreen,
    System::Threading::GetCurrentThreadId, UI::WindowsAndMessaging::*};

pub(crate) const TIMER: usize = 0x6ff2_0001;
const INVENTORY_WAIT: Duration = Duration::from_secs(5);
const MAX_DEFERRALS:usize=64;
#[cfg(test)]
use super::super::app_context_menu_probe::MenuProbeEvent;
#[cfg(test)]
thread_local! {static PROBE_OBSERVER:RefCell<Option<Rc<dyn Fn(MenuProbeEvent)>>> = RefCell::new(None);}
#[cfg(test)]
pub(crate) fn set_probe_observer(observer:Rc<dyn Fn(MenuProbeEvent)>) {
    PROBE_OBSERVER.with(|slot|*slot.borrow_mut()=Some(observer));
}
#[cfg(test)]
fn observe(event:MenuProbeEvent)->bool {
    let observer=PROBE_OBSERVER.with(|slot|slot.borrow().clone());
    observer.is_none_or(|observer|std::panic::catch_unwind(std::panic::AssertUnwindSafe(||observer(event))).is_ok())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase { Inventory, Show, Showing, Refresh, Dispatch, Dispatching, Done }
struct Owner { hwnd: HWND, pid: u32, thread: u32, token: usize, alive: Cell<bool>, request_epoch: Cell<u64> }
impl Owner {
    fn next_request_epoch(&self)->Option<u64> {
        match self.request_epoch.get().checked_add(1) {
            Some(epoch)=>{self.request_epoch.set(epoch);Some(epoch)},
            None=>{self.alive.set(false);None},
        }
    }
    fn current(&self) -> bool {
        let mut pid = 0;
        self.alive.get() && unsafe { IsWindow(Some(self.hwnd)) }.as_bool()
            && unsafe { GetWindowThreadProcessId(self.hwnd, Some(&mut pid)) } == self.thread
            && pid == self.pid && self.thread == unsafe { GetCurrentThreadId() }
    }
}
struct Entry { owner: Rc<Owner>, claim: Option<usize>, pending: Option<Rc<Request>> }
// TLS teardown is not an owning-STA completion opportunity: App/COM may already be gone.
// Preserve any still-held exact Rc rather than invoke Complete or release its lease there.
#[derive(Default)]
struct OwnerRegistry(HashMap<isize,Entry>);
impl std::ops::Deref for OwnerRegistry {type Target=HashMap<isize,Entry>;fn deref(&self)->&Self::Target{&self.0}}
impl std::ops::DerefMut for OwnerRegistry {fn deref_mut(&mut self)->&mut Self::Target{&mut self.0}}
impl Drop for OwnerRegistry {fn drop(&mut self){for (_,entry) in self.0.drain(){if let Some(request)=entry.pending{let _=Rc::into_raw(request);}}}}
#[derive(Default)]
struct OrphanRequests(Vec<Rc<Request>>);
impl std::ops::Deref for OrphanRequests {type Target=Vec<Rc<Request>>;fn deref(&self)->&Self::Target{&self.0}}
impl std::ops::DerefMut for OrphanRequests {fn deref_mut(&mut self)->&mut Self::Target{&mut self.0}}
impl Drop for OrphanRequests {fn drop(&mut self){for request in self.0.drain(..){let _=Rc::into_raw(request);}}}
#[derive(Default)]
struct AdmissionCancellations(Vec<Rc<CancelledAdmission>>);
impl std::ops::Deref for AdmissionCancellations {type Target=Vec<Rc<CancelledAdmission>>;fn deref(&self)->&Self::Target{&self.0}}
impl std::ops::DerefMut for AdmissionCancellations {fn deref_mut(&mut self)->&mut Self::Target{&mut self.0}}
impl Drop for AdmissionCancellations {fn drop(&mut self){for request in self.0.drain(..){let _=Rc::into_raw(request);}}}
thread_local! {
    static OWNERS: RefCell<OwnerRegistry> = RefCell::new(OwnerRegistry::default());
    static NEXT: Cell<usize> = const { Cell::new(0) };
    static ORPHANS: RefCell<OrphanRequests> = RefCell::new(OrphanRequests::default());
    static CANCELLED_ADMISSIONS:RefCell<AdmissionCancellations> = RefCell::new(AdmissionCancellations::default());
    static LIVE_DEFERRALS:Cell<usize> = const {Cell::new(0)};
}
/// Bounded admission occurs BEFORE GetDeferral, not after cleanup becomes uncertain. A retained
/// permit blocks replacement admission until its exact deferral and authority really retire.
struct CompletionPermit;
impl CompletionPermit {
    fn acquire()->Option<Self> {LIVE_DEFERRALS.with(|count| {
        let next=count.get().checked_add(1)?;if next>MAX_DEFERRALS {return None;}count.set(next);Some(Self)
    })}
}
impl Drop for CompletionPermit {fn drop(&mut self){LIVE_DEFERRALS.with(|count| {
    if let Some(next)=count.get().checked_sub(1){count.set(next);}else{extension_trace("engine_menu_permit_invariant_failed");}
});}}
fn next_token() -> Option<usize> { NEXT.with(|n| { let v=n.get().checked_add(1)?; n.set(v); Some(v) }) }
fn owner(hwnd: HWND) -> Option<Rc<Owner>> {
    if let Some(existing) = OWNERS.with(|all| all.borrow().get(&(hwnd.0 as isize)).map(|e| e.owner.clone())) {
        return existing.current().then_some(existing);
    }
    let mut pid=0; let thread=unsafe { GetWindowThreadProcessId(hwnd,Some(&mut pid)) };
    if pid!=std::process::id() || thread!=unsafe { GetCurrentThreadId() } { return None; }
    let owned=Rc::new(Owner { hwnd,pid,thread,token:next_token()?,alive:Cell::new(true),request_epoch:Cell::new(0) });
    if !owned.current() { return None; }
    OWNERS.with(|all| all.borrow_mut().insert(hwnd.0 as isize,Entry { owner:owned.clone(),claim:None,pending:None }));
    Some(owned)
}
fn retire_claim(hwnd:HWND,owner_token:usize,request_token:Option<usize>) {
    OWNERS.with(|all| {let mut all=all.borrow_mut();if let Some(e)=all.get_mut(&(hwnd.0 as isize))
        && e.owner.token==owner_token && e.claim==request_token && e.pending.is_none() {e.claim=None;}});
}
/// A refused construction still keeps any acquired lease through its native cancellation.
struct CancelledAdmission {
    deferral:RefCell<Option<ICoreWebView2Deferral>>,
    authority:RefCell<Option<MenuInventoryAuthority>>,
    available:Rc<dyn Fn()->bool>,
    _permit:CompletionPermit,
    attempted:Cell<bool>,uncertain_preserved:Cell<bool>,
}
impl CancelledAdmission {
    fn settle(self:&Rc<Self>) {
        if self.attempted.get() {return;}
        if !(self.available)() {
            let queued=CANCELLED_ADMISSIONS.with(|all| {let mut all=all.borrow_mut();
                if all.iter().any(|entry|Rc::ptr_eq(entry,self)){return true;}
                if all.len()>=MAX_DEFERRALS {return false;}all.push(self.clone());true
            });
            if !queued {let _=Rc::into_raw(self.clone());extension_trace("engine_menu_admission_cleanup_preserved");}
            return;
        }
        let deferral={self.deferral.borrow_mut().take()};
        let authority={self.authority.borrow_mut().take()};
        if let Some(deferral)=deferral {
            self.attempted.set(true);
            let result=unsafe {deferral.Complete()};
            extension_trace(&format!("engine_menu_cancel_complete native_error={:?}",result.as_ref().err().map(|error|error.code().0)));
            if result.is_err() {
                *self.deferral.borrow_mut()=Some(deferral);
                *self.authority.borrow_mut()=authority;
                // An uncertain Complete is never retried or treated as authority retirement.
                // The pre-admission permit makes this process-lifetime preservation bounded.
                if !self.uncertain_preserved.replace(true) {let _=Rc::into_raw(self.clone());}
                extension_trace("engine_menu_cancel_complete_uncertain_lease_preserved");
                return;
            }
        }
        drop(authority);
    }
}
struct Request {
    token: usize, request_epoch:u64, owner: Rc<Owner>, core: Weak<RefCell<EngineCore>>, tab: TabId,
    view: ICoreWebView2, controller: ICoreWebView2Controller,
    gate: Rc<RefCell<super::super::document_gate::DocumentGate>>, proof:(u64,u64,u64),
    args: ICoreWebView2ContextMenuRequestedEventArgs,
    deferral: RefCell<Option<ICoreWebView2Deferral>>,
    authority: MenuInventoryAuthority,
    menu: RefCell<Option<EngineMenu>>, selection: RefCell<Option<MenuSelection>>,
    shown: RefCell<Option<NativeInventory>>, fresh: RefCell<Option<NativeInventory>>,
    phase: Cell<Phase>, valid: Cell<bool>, deadline: Cell<Instant>, point: POINT, bounds: RECT,
    post: Rc<dyn Fn(usize)->bool>, available:Rc<dyn Fn()->bool>, setter_called: Cell<bool>,
    setter_result:Cell<Option<std::result::Result<(),i32>>>,
    complete_calls:Cell<u32>,complete_error:Cell<Option<i32>>,selected:Cell<Option<i32>>,cleanup_ok:Cell<bool>,
    _permit:CompletionPermit,
    uncertain_preserved:Cell<bool>,
}
impl Request {
    /// No Core or gate borrow survives any COM getter. The final pure check follows all getters.
    fn current(&self) -> bool {
        if !self.pure_current() { return false; }
        let mut visible=BOOL::default();
        if unsafe { self.controller.IsVisible(&mut visible) }.is_err() || !visible.as_bool() { return false; }
        let mut bounds=RECT::default();
        if unsafe { self.controller.Bounds(&mut bounds) }.is_err()
            || (bounds.left,bounds.top,bounds.right,bounds.bottom)!=(self.bounds.left,self.bounds.top,self.bounds.right,self.bounds.bottom) {return false;}
        current_web_url(&self.view).is_some_and(|u| matches!(u.scheme(),"http"|"https"))
            && self.pure_current()
    }
    fn pure_current(&self) -> bool {
        if !(self.available)() || !self.valid.get() || self.owner.request_epoch.get()!=self.request_epoch || !self.owner.current() || unsafe { GetForegroundWindow() }!=self.owner.hwnd { return false; }
        let registered=OWNERS.with(|all|all.try_borrow().is_ok_and(|all|all.get(&(self.owner.hwnd.0 as isize)).is_some_and(|e|
            e.owner.token==self.owner.token && e.pending.as_ref().is_some_and(|p|p.token==self.token))));
        if !registered { return false; }
        let Some(core)=self.core.upgrade() else { return false; };
        let Ok(state)=core.try_borrow() else { return false; };
        !state.closed && state.parent==self.owner.hwnd && state.active_tab==Some(self.tab)
            && !state.private_tabs.contains(&self.tab)
            && state.tabs.get(&self.tab).is_some_and(|tab|Rc::ptr_eq(&tab.document_gate,&self.gate)
                && tab.webview.as_ref().is_some_and(|v|v.as_raw()==self.view.as_raw())
                && tab.controller.as_ref().is_some_and(|v|v.as_raw()==self.controller.as_raw()))
            && self.gate.try_borrow().is_ok_and(|g|g.proof()==Some(self.proof))
    }
    fn complete(&self)->bool {
        if self.complete_calls.get()!=0 {return self.complete_error.get().is_none();}
        let deferral={self.deferral.borrow_mut().take()};
        if let Some(deferral)=deferral {
            self.complete_calls.set(1);
            let result=unsafe { deferral.Complete() };
            self.complete_error.set(result.as_ref().err().map(|error|error.code().0));
            extension_trace(&format!("engine_menu_complete native_error={:?}",self.complete_error.get()));
            if result.is_err() {*self.deferral.borrow_mut()=Some(deferral);return false;}
        }
        true
    }
    fn post(self:&Rc<Self>) { if !(self.post)(self.token) { self.cancel(); } }
    fn cancel(self:&Rc<Self>) {
        self.valid.set(false);
        if matches!(self.phase.get(),Phase::Showing|Phase::Dispatching) { return; }
        self.finish();
    }
    fn finish(self:&Rc<Self>) {
        self.valid.set(false);
        if !(self.available)() {
            let retained=OWNERS.with(|all|all.try_borrow().is_ok_and(|all|all.get(&(self.owner.hwnd.0 as isize))
                .is_some_and(|e|e.pending.as_ref().is_some_and(|p|p.token==self.token))));
            if !retained {
                let queued=ORPHANS.with(|all| {let mut all=all.borrow_mut();
                    if all.iter().any(|request|request.token==self.token) {return true;}
                    if all.len()>=MAX_DEFERRALS {return false;}all.push(self.clone());true
                });
                if !queued {
                    // Unknown teardown: preserve the exact request/lease/deferral, never invoke
                    // reentrant completion or release authority underneath an outer App borrow.
                    let _=Rc::into_raw(self.clone());extension_trace("engine_menu_uncertain_cleanup_preserved");
                }
            }
            return;
        }
        if self.phase.replace(Phase::Done)==Phase::Done {return;}
        self.valid.set(false);
        let removed=OWNERS.with(|all| {
            let mut all=all.borrow_mut();
            let Some(e)=all.get_mut(&(self.owner.hwnd.0 as isize)) else { return None; };
            if e.owner.token==self.owner.token && e.claim==Some(self.token) { e.claim=None;e.pending.take() } else { None }
        });
        // Retire the old timer before any reentrant destruction/completion can admit a new menu.
        // A destroyed owner needs no KillTimer and its possibly reused HWND must not be touched.
        if self.owner.current() {unsafe { let _=KillTimer(Some(self.owner.hwnd),TIMER); }}
        let menu={self.menu.borrow_mut().take()};
        if let Some(mut menu)=menu {if menu.close().is_err(){self.cleanup_ok.set(false);extension_trace("engine_menu_cleanup_failed");}}
        let selection={self.selection.borrow_mut().take()}; drop(selection);
        // Complete while this request (and its required lease) is still alive, with no map borrow.
        if !self.complete() {
            self.cleanup_ok.set(false);
            if !self.uncertain_preserved.replace(true) {let _=Rc::into_raw(self.clone());}
            extension_trace("engine_menu_complete_uncertain_lease_preserved");
        }
        #[cfg(test)]
        let _=observe(MenuProbeEvent::Completed {request:self.token,selected:self.selected.get(),
            setter_calls:u32::from(self.setter_called.get()),setter_result:self.setter_result.get(),complete_calls:self.complete_calls.get(),
            complete_error:self.complete_error.get(),cleanup_ok:self.cleanup_ok.get(),refused:self.selected.get().is_some()&&!self.setter_called.get()});
        drop(removed);
    }
    fn enumerate(self: &Rc<Self>, refresh: bool) {
        self.deadline.set(Instant::now()+INVENTORY_WAIT);
        self.phase.set(if refresh {Phase::Refresh}else{Phase::Inventory});
        let weak=Rc::downgrade(self);
        self.authority.enumerate(Box::new(move |result| {
            let Some(request)=weak.upgrade() else { return; };
            let expected=if refresh {Phase::Refresh}else{Phase::Inventory};
            if request.phase.get()!=expected || !request.current() || Instant::now()>=request.deadline.get() {request.cancel();return;}
            match result {
                Ok(inventory) => {
                    if refresh { *request.fresh.borrow_mut()=Some(inventory);request.phase.set(Phase::Dispatch); }
                    else { *request.shown.borrow_mut()=Some(inventory);request.phase.set(Phase::Show); }
                    request.post();
                }
                Err(_) => {extension_trace("engine_menu_inventory_refused");request.cancel();}
            }
        }));
    }
}
// There is deliberately no Drop fallback calling native Complete. All normal retirement is
// explicit; failed/transient native state is retained by its bounded Rc ownership.

pub(super) struct Registration { native:ICoreWebView2_11, token:Option<i64>, alive:Rc<Cell<bool>> }
impl Registration {
    pub(super) fn close(&mut self)->Result<()> {
        self.alive.set(false);
        if let Some(token)=self.token.take() {
            let result=unsafe {self.native.remove_ContextMenuRequested(token)};
            #[cfg(test)] let _=observe(MenuProbeEvent::Deregistered {native_error:result.as_ref().err().map(|error|error.code().0)});
            result
        } else {Ok(())}
    }
}
impl Drop for Registration {fn drop(&mut self){if self.close().is_err(){extension_trace("engine_menu_deregister_failed");}}}

pub(super) fn install(
    core: Weak<RefCell<EngineCore>>, view:&ICoreWebView2, controller:&ICoreWebView2Controller,
    parent:HWND, tab:TabId, gate:Rc<RefCell<super::super::document_gate::DocumentGate>>,
    augmenters:Vec<ContextAugmenter>, post:Rc<dyn Fn(usize)->bool>, available:Rc<dyn Fn()->bool>,
) -> Result<Registration> {
    let owned=owner(parent).ok_or_else(||Error::from(windows::Win32::Foundation::E_FAIL))?;
    let receiving=view.clone(); let receiving_controller=controller.clone();
    let native=view.cast::<ICoreWebView2_11>()?; let mut registration=0;
    let alive=Rc::new(Cell::new(true));let registered=alive.clone();
    unsafe { native.add_ContextMenuRequested(&ContextMenuRequestedEventHandler::create(Box::new(move |sender,args| {
        let Some(args)=args else { return Ok(()); };
        if !registered.get() {args.SetHandled(true)?;return Ok(());}
        if !available() {
            let _=owned.next_request_epoch();
            let pending=OWNERS.with(|all|all.borrow().get(&(parent.0 as isize)).and_then(|e|e.pending.clone()));
            if let Some(pending)=pending {pending.valid.set(false);let _=(pending.post)(pending.token);}
            // No augmentation, authority acquisition or deferral is started under an outer App
            // borrow. Handled alone cancels this request when its native callback returns.
            args.SetHandled(true)?;return Ok(());
        }
        let epoch=owned.next_request_epoch();
        // Bind to the document that actually raised this request BEFORE any COM getter can
        // reenter navigation. Never replace this token with a newer successful document.
        let original_proof=gate.try_borrow().ok().and_then(|gate|gate.proof());
        let initial_receiver=core.upgrade().is_some_and(|core|core.try_borrow().is_ok_and(|state|
            !state.closed && state.parent==parent && state.active_tab==Some(tab) && !state.private_tabs.contains(&tab)
            && state.tabs.get(&tab).is_some_and(|current|Rc::ptr_eq(&current.document_gate,&gate)
                && current.webview.as_ref().is_some_and(|v|v.as_raw()==receiving.as_raw())
                && current.controller.as_ref().is_some_and(|v|v.as_raw()==receiving_controller.as_raw()))));
        // Reserve before reentrant COM calls. A second request invalidates the current one and
        // cannot overwrite either an in-construction claim or a pending/modal request.
        let token=next_token();
        let (claimed,existing)=OWNERS.with(|all| {
            let mut all=all.borrow_mut();
            let Some(entry)=all.get_mut(&(parent.0 as isize)) else {return (false,None);};
            if entry.owner.token!=owned.token || entry.claim.is_some() || token.is_none() {
                return (false,entry.pending.clone());
            }
            entry.claim=token;(true,None)
        });
        if let Some(existing)=existing {existing.valid.set(false);let _=(existing.post)(existing.token);}
        // A coordinated request never falls through to a partially augmented/truncated native menu.
        if let Err(error)=args.SetHandled(true) {retire_claim(parent,owned.token,token);return Err(error);}
        let Some(permit)=CompletionPermit::acquire() else {retire_claim(parent,owned.token,token);return Ok(());};
        let mut permit=Some(permit);
        let mut deferral=match args.GetDeferral() {Ok(value)=>Some(value),Err(error)=>{retire_claim(parent,owned.token,token);return Err(error);}};
        let mut acquired_authority=None;
        let admitted=(|| -> Option<Rc<Request>> {
            if !claimed || !initial_receiver || epoch.is_none() || !owned.current() || !sender.as_ref().is_some_and(|v|v.as_raw()==receiving.as_raw()) {return None;}
            for augment in &augmenters { if augment(sender.clone(),Some(args.clone())).is_err() {return None;} }
            let core_now=core.upgrade()?;
            acquired_authority=Some(MenuInventoryAuthority::acquire(&core_now,&receiving).ok()?);
            let proof=original_proof?;
            if gate.try_borrow().ok()?.proof()!=Some(proof) {return None;}
            let menu=match EngineMenu::capture(args.MenuItems().ok()?) {
                Ok(menu)=>menu,
                Err(error)=>{extension_trace(&format!("engine_menu_capture_refused category={error:?}"));return None;}
            };
            let mut point=POINT::default(); args.Location(&mut point).ok()?;
            let mut bounds=RECT::default(); receiving_controller.Bounds(&mut bounds).ok()?;
            if point.x<0 || point.y<0 || point.x>=bounds.right.checked_sub(bounds.left)? || point.y>=bounds.bottom.checked_sub(bounds.top)? {return None;}
            point.x=point.x.checked_add(bounds.left)?;point.y=point.y.checked_add(bounds.top)?;
            if !ClientToScreen(parent,&mut point).as_bool() {return None;}
            let request=Rc::new(Request { token:token?,request_epoch:epoch?,owner:owned.clone(),core:core.clone(),tab,
                view:receiving.clone(),controller:receiving_controller.clone(),gate:gate.clone(),proof,
                args:args.clone(),deferral:RefCell::new(Some(deferral.take()?)),authority:acquired_authority.take()?,menu:RefCell::new(Some(menu)),
                selection:RefCell::new(None),shown:RefCell::new(None),fresh:RefCell::new(None),phase:Cell::new(Phase::Inventory),
                valid:Cell::new(true),deadline:Cell::new(Instant::now()+INVENTORY_WAIT),point,bounds,post:post.clone(),available:available.clone(),setter_called:Cell::new(false),setter_result:Cell::new(None),complete_calls:Cell::new(0),complete_error:Cell::new(None),selected:Cell::new(None),cleanup_ok:Cell::new(true),_permit:permit.take()?,uncertain_preserved:Cell::new(false) });
            let installed=OWNERS.with(|all| {let mut all=all.borrow_mut();
                let Some(e)=all.get_mut(&(parent.0 as isize)) else {return false;};
                if e.owner.token!=owned.token || e.claim!=token || e.pending.is_some() {return false;}
                e.pending=Some(request.clone());true
            });
            if !installed {request.finish();return None;}
            if !request.current() {request.finish();return None;}
            if SetTimer(Some(parent),TIMER,250,None)==0 {request.finish();return None;}
            Some(request)
        })();
        if let Some(request)=admitted { request.enumerate(false); }
        else {
            if claimed {retire_claim(parent,owned.token,token);}
            if deferral.is_some() {
                if let Some(permit)=permit {
                    Rc::new(CancelledAdmission {deferral:RefCell::new(deferral),authority:RefCell::new(acquired_authority),available:available.clone(),_permit:permit,attempted:Cell::new(false),uncertain_preserved:Cell::new(false)}).settle();
                }
            }
            extension_trace("engine_menu_admission_refused");
        }
        Ok(())
    })),&mut registration)?; }
    Ok(Registration{native,token:Some(registration),alive})
}

pub(crate) fn continue_request(hwnd:HWND,token:usize) {
    drain_destroyed();
    let request=OWNERS.with(|all|all.borrow().get(&(hwnd.0 as isize)).and_then(|e|e.pending.as_ref().filter(|p|p.token==token).cloned()));
    let Some(request)=request else {return;};
    if Instant::now()>=request.deadline.get() || !request.current() {request.cancel();return;}
    let Some(core)=request.core.upgrade() else {request.cancel();return;};
    match request.phase.get() {
        Phase::Show => {
            let inventory={request.shown.borrow().clone()};
            let Some(inventory)=inventory else {request.cancel();return;};
            if request.authority.validate(&core,&request.view,&inventory).is_err() || !request.current() {request.cancel();return;}
            let menu={request.menu.borrow_mut().take()};let Some(mut menu)=menu else {request.cancel();return;};
            #[cfg(test)]
            if let Some(fixture_command)=menu.probe_fixture_command() {
                if !observe(MenuProbeEvent::BeforeShow {request:request.token,tab:request.tab,
                    view_identity:request.view.as_raw() as usize,proof:request.proof,fixture_command}) {request.cancel();return;}
            }
            request.phase.set(Phase::Showing);
            let choice=menu.show(hwnd,request.point,||
                request.authority.validate(&core,&request.view,&inventory).is_ok() && request.current());
            request.phase.set(Phase::Show);
            *request.menu.borrow_mut()=Some(menu);
            if !request.current() {request.cancel();return;}
            match choice {
                Ok(Some(selection)) => { *request.selection.borrow_mut()=Some(selection);request.enumerate(true); }
                _ => request.finish(),
            }
        }
        Phase::Dispatch => {
            let shown={request.shown.borrow().clone()};let fresh={request.fresh.borrow().clone()};
            let (Some(shown),Some(fresh))=(shown,fresh) else {request.cancel();return;};
            if request.authority.validate_unchanged(&core,&request.view,&shown,&fresh).is_err() {request.cancel();return;}
            let menu={request.menu.borrow_mut().take()};let selection={request.selection.borrow_mut().take()};
            let (Some(mut menu),Some(selection))=(menu,selection) else {request.cancel();return;};
            let admitted=request.authority.validate_unchanged(&core,&request.view,&shown,&fresh).is_ok() && request.current();
            let selected=if admitted {menu.validate_selection(&selection)} else {Err(super::engine_menu::MenuError::InvalidOwner)};
            if let Err(error)=selected {extension_trace(&format!("engine_menu_selection_refused category={error:?}"));}

            if let Ok(id)=selected {
                request.selected.set(Some(id));
                // The setter may execute native built-ins, existing Bowser callbacks or a genuine
                // extension event. All safety checks precede it; no retry and no manual callback.
                if request.authority.current(&core,&request.view).is_ok()
                    && request.current() && menu.recheck_selected(&selection,id).is_ok()
                    && request.authority.pure_owner_current(&core) && request.pure_current()
                    && !request.setter_called.replace(true) {
                    request.phase.set(Phase::Dispatching);
                    let result=unsafe { request.args.SetSelectedCommandId(id) };
                    request.setter_result.set(Some(result.as_ref().map(|_|()).map_err(|error|error.code().0)));
                    request.phase.set(Phase::Dispatch);
                    extension_trace(&format!("engine_menu_dispatch native_ok={} native_error={:?}",result.is_ok(),result.err().map(|error|error.code().0)));
                }
            }
            if menu.close().is_err() {request.cleanup_ok.set(false);}
            drop(menu);request.finish();
        }
        _ => {},
    }
}
pub(crate) fn tick(hwnd:HWND) {
    drain_destroyed();
    let request=OWNERS.with(|all|all.borrow().get(&(hwnd.0 as isize)).and_then(|e|e.pending.clone()));
    if let Some(request)=request {
        if request.phase.get()!=Phase::Showing && (Instant::now()>=request.deadline.get() || !request.pure_current()) {request.cancel();}
    } else {unsafe {let _=KillTimer(Some(hwnd),TIMER);}}
}
pub(crate) fn drain_destroyed() {
    let cancelled=CANCELLED_ADMISSIONS.with(|all|std::mem::take(&mut all.borrow_mut().0));
    for cancelled in cancelled {cancelled.settle();}
    let pending=ORPHANS.with(|all|std::mem::take(&mut all.borrow_mut().0));
    for request in pending {request.finish();}
}
pub(crate) fn owner_destroyed(hwnd:HWND) {
    let entry=OWNERS.with(|all|all.borrow_mut().remove(&(hwnd.0 as isize)));
    if let Some(entry)=entry {entry.owner.alive.set(false);if let Some(request)=entry.pending {request.cancel();}}
}
pub(super) fn cancel_core(core:&Rc<RefCell<EngineCore>>,tab:Option<TabId>) {
    let pending:Vec<_>=OWNERS.with(|all|all.borrow().values().filter_map(|e|e.pending.clone()).filter(|r|
        r.core.ptr_eq(&Rc::downgrade(core)) && tab.is_none_or(|tab|r.tab==tab)).collect());
    for request in pending {
        // Engine methods can be called while App is borrowed. Invalidate synchronously, but never
        // Complete a deferral (a reentrant COM call) until the real window continuation/timer.
        request.valid.set(false);
        let _=(request.post)(request.token);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dummy_owner(token:usize)->Rc<Owner> {
        // Opaque key only. No test dereferences this pointer or calls a native function with it.
        Rc::new(Owner {hwnd:HWND(std::ptr::dangling_mut()),pid:0,thread:0,token,
            alive:Cell::new(true),request_epoch:Cell::new(0)})
    }
    #[test]
    fn every_request_advances_the_owner_fence_and_overflow_refuses() {
        let owner=dummy_owner(3);assert_eq!(owner.next_request_epoch(),Some(1));
        assert_eq!(owner.next_request_epoch(),Some(2));owner.request_epoch.set(u64::MAX);
        assert_eq!(owner.next_request_epoch(),None);assert!(!owner.alive.get());
    }
    #[test]
    fn old_request_or_owner_cannot_retire_a_new_claim() {
        let owner=dummy_owner(3);let key=owner.hwnd.0 as isize;
        OWNERS.with(|all| {assert!(!all.borrow().contains_key(&key));all.borrow_mut().insert(key,Entry {owner:owner.clone(),claim:Some(8),pending:None});});
        retire_claim(owner.hwnd,2,Some(8));retire_claim(owner.hwnd,3,Some(7));
        assert_eq!(OWNERS.with(|all|all.borrow()[&key].claim),Some(8));
        retire_claim(owner.hwnd,3,Some(8));
        assert_eq!(OWNERS.with(|all|all.borrow()[&key].claim),None);
        OWNERS.with(|all|{all.borrow_mut().remove(&key);});
    }
    #[test]
    fn uncertain_completion_cannot_admit_unbounded_deferrals() {
        let permits:Vec<_>=(0..MAX_DEFERRALS).map(|_|CompletionPermit::acquire().unwrap()).collect();
        assert!(CompletionPermit::acquire().is_none());drop(permits);
        let permit=CompletionPermit::acquire().expect("actual retirement releases one slot");drop(permit);
        LIVE_DEFERRALS.with(|count|assert_eq!(count.get(),0));
    }
    #[test]
    fn experimental_feature_is_not_a_shipping_default() {
        let cargo=include_str!("../../../Cargo.toml");
        assert!(cargo.contains("experimental-engine-context-menu = []"));
        assert!(!cargo.lines().any(|line|line.trim_start().starts_with("default =")&&line.contains("experimental-engine-context-menu")));
    }
}
