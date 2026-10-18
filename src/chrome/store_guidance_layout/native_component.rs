//! Disposable native component only. No App, Store command, browser bridge or production caller.
use std::{cell::{Cell,RefCell},collections::HashMap,marker::PhantomData,rc::Rc};
use windows::{core::{HSTRING,w},Win32::{Foundation::{HWND,LPARAM,LRESULT,RECT,SIZE,WPARAM},Graphics::Gdi::{ANTIALIASED_QUALITY,CreateCompatibleDC,DeleteDC,DeleteObject,DT_CALCRECT,DT_NOPREFIX,DT_WORDBREAK,GetTextExtentPoint32W,GetTextMetricsW,HDC,HFONT,HGDIOBJ,SelectObject,TEXTMETRICW},System::Threading::{GetCurrentProcessId,GetCurrentThreadId},UI::{Input::KeyboardAndMouse::{EnableWindow,IsWindowEnabled},Shell::{DefSubclassProc,RemoveWindowSubclass,SetWindowSubclass},WindowsAndMessaging::{CreateWindowExW,DestroyWindow,GetDlgCtrlID,GetParent,IsWindow,IsWindowVisible,SendMessageW,SetWindowPos,ShowWindow,BN_CLICKED,BS_PUSHBUTTON,HMENU,HWND_TOP,SW_HIDE,SW_SHOW,SWP_NOACTIVATE,SWP_SHOWWINDOW,WINDOW_EX_STYLE,WINDOW_STYLE,WM_COMMAND,WM_NCDESTROY,WM_SETFONT,WM_GETFONT,GetWindowThreadProcessId,GetClientRect,WS_CHILD,WS_TABSTOP}}}};
use super::{partition,Request,Rows};
use crate::chrome::{layout::Rect,paint};

pub(super) const NOTE: &str = "Google installs into Chrome. Bowser uses a separate verified package and permission review. Extension compatibility varies.";
pub(super) const LABEL: &str = "Add to Bowser";
const NOTE_ID: i32=0x5b10;
const BUTTON_ID: i32=0x5b11;
const SUBCLASS_ID: usize=0x5b12;
#[link(name="user32")]
unsafe extern "system" { #[link_name="DrawTextW"] fn measured_text(dc:HDC,text:*const u16,count:i32,rect:*mut RECT,flags:u32)->i32; }

struct Font {font:HFONT,dc:HDC,previous:HGDIOBJ}
impl Font {
 fn new(dpi:u32,scale:u32)->Option<Self>{
  let mut value=Self{font:paint::create_message_font_with(dpi,ANTIALIASED_QUALITY,scale,None),dc:unsafe{CreateCompatibleDC(None)},previous:HGDIOBJ::default()};
  if value.font.0.is_null()||value.dc.0.is_null(){return None}
  value.previous=unsafe{SelectObject(value.dc,value.font.into())};
  if value.previous.0.is_null()||value.previous.0 as isize == -1{return None}
  Some(value)
 }
 fn wrapped(&self,text:&str,width:i32)->Option<i32>{
  if width<=0{return None}let units:Vec<_>=text.encode_utf16().collect();if units.is_empty(){return None}
  let mut rect=RECT{left:0,top:0,right:width,bottom:0};
  let height=unsafe{measured_text(self.dc,units.as_ptr(),units.len() as i32,&mut rect,(DT_CALCRECT|DT_WORDBREAK|DT_NOPREFIX).0)};
  (height>0&&rect.bottom-rect.top==height&&rect.right-rect.left<=width).then_some(height)
 }
 fn extent(&self,text:&str)->Option<SIZE>{let units:Vec<_>=text.encode_utf16().collect();let mut size=SIZE::default();unsafe{GetTextExtentPoint32W(self.dc,&units,&mut size)}.as_bool().then_some(size)}
}
impl Font {
 fn release(&mut self)->bool{unsafe{
  let restored=if self.dc.0.is_null()||self.previous.0.is_null(){true}else{let old=SelectObject(self.dc,self.previous);!old.0.is_null()&&old.0 as isize != -1};
  let deleted=self.dc.0.is_null()||DeleteDC(self.dc).as_bool();if deleted{self.dc=HDC::default();}
  let font_deleted=(restored||deleted)&&(self.font.0.is_null()||DeleteObject(self.font.into()).as_bool());if font_deleted{self.font=HFONT::default();}
  restored&&deleted&&font_deleted
 }}
}
impl Drop for Font {fn drop(&mut self){let _=self.release();}}


#[derive(Clone,Copy,Debug)]
pub(super) struct Placement {pub note:Rect,pub button:Rect,pub rows:Rows,pub stacked:bool}
fn placement(font:&Font,width:i32,height:i32,dpi:u32)->Option<Placement>{
 if !(96..=768).contains(&dpi)||!(1..=8192).contains(&width)||!(1..=8192).contains(&height){return None}
 let pad=(8*dpi as i32/96).max(1);let button_text=font.extent(LABEL)?;let mut metric=TEXTMETRICW::default();if !unsafe{GetTextMetricsW(font.dc,&mut metric)}.as_bool(){return None}
 let button_width=button_text.cx.checked_add(3*pad)?;let button_height=metric.tmHeight.checked_add(2*pad)?;
 let widest_word=NOTE.split_whitespace().map(|word|font.extent(word).map(|s|s.cx)).collect::<Option<Vec<_>>>()?.into_iter().max()?;
 let content=width.checked_sub(2*pad)?;if content<button_width||content<widest_word{return None}
 let beside_width=content.checked_sub(button_width+pad)?;let stacked=beside_width<widest_word;
 let note_width=if stacked{content}else{beside_width};let note_height=font.wrapped(NOTE,note_width)?;
 let row_height=if stacked{note_height.checked_add(button_height)?.checked_add(3*pad)?}else{note_height.max(button_height).checked_add(2*pad)?};
 let rows=partition(Request{area:Rect::new(0,0,width,height),bookmark_height:0,bookmark_bottom:false,bookmark_reserved:true,measured_guidance:Some(row_height),minimum_page_height:96*dpi as i32/96})?;
 rows.guidance?;
 let note=Rect::new(pad,pad,pad+note_width,pad+note_height);
 let button_top=if stacked{note.bottom+pad}else{pad+(row_height-2*pad-button_height)/2};
 let button_left=if stacked{pad}else{note.right+pad};
 let button=Rect::new(button_left,button_top,button_left+button_width,button_top+button_height);
 Some(Placement{note,button,rows,stacked})
}

// This private STA registration coordinates only this fixture constructor, not arbitrary subclasses.
thread_local! {static OWNER_CLAIMS:RefCell<HashMap<isize,u64>>=RefCell::new(HashMap::new());static NEXT_CLAIM:Cell<u64>=const{Cell::new(0)};}
struct OwnerClaim{owner:HWND,token:u64,thread:u32,_sta:PhantomData<Rc<()>>}
fn retire_claim(owner:HWND,token:u64,thread:u32)->bool{
    if thread!=unsafe{GetCurrentThreadId()}{return false}
    OWNER_CLAIMS.with(|claims|{let mut claims=claims.borrow_mut();let key=owner.0 as isize;if claims.get(&key)!=Some(&token){return false}claims.remove(&key);true})
}
impl OwnerClaim{
    fn new(owner:HWND)->Option<Self>{
        let token=NEXT_CLAIM.with(|next|{let value=next.get().checked_add(1)?;next.set(value);Some(value)})?;
        let claimed=OWNER_CLAIMS.with(|claims|{let mut claims=claims.borrow_mut();let key=owner.0 as isize;match claims.entry(key){std::collections::hash_map::Entry::Vacant(entry)=>{entry.insert(token);true},std::collections::hash_map::Entry::Occupied(_)=>false}});
        claimed.then_some(Self{owner,token,thread:unsafe{GetCurrentThreadId()},_sta:PhantomData})
    }
    fn retire(&self)->bool{retire_claim(self.owner,self.token,self.thread)}
}
#[test]fn native_band_registration_tokens_refuse_stale_cleanup(){
    let owner=HWND(std::ptr::dangling_mut());let first=OwnerClaim::new(owner).unwrap();assert!(OwnerClaim::new(owner).is_none());
    // Known nonattached failure can retire only its own claim; another claim then gets a new token.
    assert!(first.retire());let second=OwnerClaim::new(owner).unwrap();assert_ne!(first.token,second.token);
    assert!(!first.retire());assert!(OwnerClaim::new(owner).is_none());assert!(second.retire());
    let third=OwnerClaim::new(owner).unwrap();assert!(third.retire());
}
struct Authority {
    owner: HWND,
    pid: u32,
    thread: u32,
    button: Cell<HWND>,
    note: Cell<HWND>,
    button_alive: Cell<bool>,
    note_alive: Cell<bool>,
    alive: Cell<bool>,
    owner_destroyed:Cell<bool>,
    published: Cell<bool>,
    generation: Cell<u64>,
    clicks: Cell<u32>,
    retired_buttons: Cell<u32>,
}
impl Authority {
    fn same_owner(&self) -> bool {
        let mut pid=0;
        self.alive.get() && unsafe { IsWindow(Some(self.owner)).as_bool()
            && GetWindowThreadProcessId(self.owner,Some(&mut pid))==self.thread && pid==self.pid }
    }
    fn same_size(&self,width:i32,height:i32)->bool {
        let mut rect=RECT::default();self.same_owner() && unsafe{GetClientRect(self.owner,&mut rect)}.is_ok() && rect.left==0 && rect.top==0 && rect.right==width && rect.bottom==height
    }
    fn child(&self, child: HWND, id: i32) -> bool {
        let alive=if id==BUTTON_ID { self.button_alive.get() } else { self.note_alive.get() };
        alive && self.same_owner() && unsafe { IsWindow(Some(child)).as_bool()
            && GetParent(child).ok()==Some(self.owner) && GetDlgCtrlID(child)==id }
    }
    fn advance(&self) -> bool {
        self.published.set(false);
        if let Some(next)=self.generation.get().checked_add(1) { self.generation.set(next);true }
        else { self.alive.set(false);false }
    }
}
unsafe extern "system" fn notification(owner:HWND,message:u32,w:WPARAM,l:LPARAM,_id:usize,data:usize)->LRESULT {
    unsafe {
        let state=&*(data as *const Authority);
        if message==WM_NCDESTROY { state.owner_destroyed.set(true);state.alive.set(false);state.published.set(false);let _=state.advance(); }
        if message==WM_COMMAND && state.published.get() && state.same_owner()
            && state.owner==owner && (w.0&0xffff) as i32==BUTTON_ID && ((w.0>>16)&0xffff) as u32==BN_CLICKED {
            let button=HWND(l.0 as *mut _);
            if button==state.button.get() && state.child(button,BUTTON_ID)
                && IsWindowEnabled(button).as_bool() && IsWindowVisible(button).as_bool() {
                // This test component cannot download/install or call App/Store functions.
                state.clicks.set(state.clicks.get().saturating_add(1));
            }
        }
        DefSubclassProc(owner,message,w,l)
    }
}
unsafe extern "system" fn child_lifetime(child:HWND,message:u32,w:WPARAM,l:LPARAM,id:usize,data:usize)->LRESULT {
    unsafe {
        let state=&*(data as *const Authority);
        if message==WM_NCDESTROY {
            if id==BUTTON_ID as usize && child==state.button.get() { state.button_alive.set(false);let _=state.advance(); }
            if id==NOTE_ID as usize && child==state.note.get() { state.note_alive.set(false);let _=state.advance(); }
            state.published.set(false);
            let _=RemoveWindowSubclass(child,Some(child_lifetime),id);
        }
        DefSubclassProc(child,message,w,l)
    }
}
#[derive(Clone,Copy,Debug)]
pub(super) struct Prepared {pub plan:Placement,generation:u64}
/// Native component fixture only: there is deliberately no installation function or App reference.
pub(super) struct Band {state:Box<Authority>,claim:OwnerClaim,font:Option<Font>,pub placement:Option<Placement>}
impl Band {
    pub fn new(owner:HWND)->Option<Self> {
        let mut pid=0;let thread=unsafe{GetWindowThreadProcessId(owner,Some(&mut pid))};
        if pid!=unsafe{GetCurrentProcessId()} || thread!=unsafe{GetCurrentThreadId()} || !unsafe{IsWindow(Some(owner))}.as_bool() {return None}
        let claim=OwnerClaim::new(owner)?;
        let state=Box::new(Authority{owner,pid,thread,button:Cell::new(HWND::default()),note:Cell::new(HWND::default()),button_alive:Cell::new(false),note_alive:Cell::new(false),alive:Cell::new(true),owner_destroyed:Cell::new(false),published:Cell::new(false),generation:Cell::new(1),clicks:Cell::new(0),retired_buttons:Cell::new(0)});
        if !unsafe{SetWindowSubclass(owner,Some(notification),SUBCLASS_ID,(&*state as *const Authority) as usize)}.as_bool(){assert!(claim.retire(),"known nonattached constructor failure must retire its matching claim");return None}
        let band=Self{state,claim,font:None,placement:None};
        if !band.state.same_owner(){return None}
        Some(band)
    }
    /// Disable/hide before the caller releases the row. Failure must retain the reservation.
    pub fn hide(&mut self)->bool {
        self.state.published.set(false);
        for (child,id,alive) in [(self.state.button.get(),BUTTON_ID,self.state.button_alive.get()),(self.state.note.get(),NOTE_ID,self.state.note_alive.get())] {
            if !alive {continue}
            if !self.state.child(child,id) {return false}
            unsafe { let _=EnableWindow(child,false); }
            if !self.state.child(child,id){return false}
            unsafe { let _=ShowWindow(child,SW_HIDE); }
            if !self.state.child(child,id) || unsafe{IsWindowVisible(child).as_bool()||IsWindowEnabled(child).as_bool()} {return false}
        }
        self.placement=None;true
    }
    pub fn release(&mut self)->bool {
        if !self.hide() {return false}
        for (child,id,alive) in [(self.state.button.get(),BUTTON_ID,self.state.button_alive.get()),(self.state.note.get(),NOTE_ID,self.state.note_alive.get())] {
            if !alive {continue}
            if !self.state.child(child,id) {return false}
            if unsafe{DestroyWindow(child)}.is_err() || unsafe{IsWindow(Some(child))}.as_bool() {return false}
            if id==BUTTON_ID {self.state.button_alive.set(false);self.state.retired_buttons.set(self.state.retired_buttons.get().saturating_add(1))}else{self.state.note_alive.set(false)}
        }
        if self.state.button_alive.get()||self.state.note_alive.get() {return false}
        self.state.button.set(HWND::default());self.state.note.set(HWND::default());
        if let Some(mut font)=self.font.take(){if !font.release(){self.font=Some(font);return false}}
        self.state.advance()
    }
    /// Build hidden and disabled. Caller applies the returned reserve before calling show.
    pub fn prepare(&mut self,width:i32,height:i32,dpi:u32,scale:u32)->Option<Prepared> {
        if !self.release()||!self.state.same_owner(){return None}
        if !(100..=200).contains(&scale)||!(96..=768).contains(&dpi)||!self.state.same_size(width,height){return None}
        let font=Font::new(dpi,scale)?;let plan=placement(&font,width,height,dpi)?;
        if !self.state.same_size(width,height){return None}
        self.font=Some(font);let generation=self.state.generation.get();
        let data=(&*self.state as *const Authority) as usize;
        for (class,text,id,style,rect) in [(w!("STATIC"),NOTE,NOTE_ID,WINDOW_STYLE(0x80),plan.note),(w!("BUTTON"),LABEL,BUTTON_ID,WS_TABSTOP|WINDOW_STYLE(BS_PUSHBUTTON as u32),plan.button)] {
            let created=unsafe{CreateWindowExW(WINDOW_EX_STYLE::default(),class,&HSTRING::from(text),WS_CHILD|style,0,0,1,1,Some(self.state.owner),Some(HMENU(id as usize as *mut _)),None,None)};
            let Ok(child)=created else {let _=self.release();return None};
            if id==BUTTON_ID {self.state.button.set(child);self.state.button_alive.set(true)}else{self.state.note.set(child);self.state.note_alive.set(true)}
            if !self.state.same_owner()||self.state.generation.get()!=generation {let _=self.release();return None}
            if !unsafe{SetWindowSubclass(child,Some(child_lifetime),id as usize,data)}.as_bool() {let _=self.release();return None}
            if !self.state.child(child,id)||self.state.generation.get()!=generation {let _=self.release();return None}
            unsafe {let _=EnableWindow(child,false);}
            if !self.state.child(child,id)||self.state.generation.get()!=generation {let _=self.release();return None}
            unsafe {let _=SendMessageW(child,WM_SETFONT,Some(WPARAM(self.font.as_ref()?.font.0 as usize)),Some(LPARAM(0)));}
            if !self.state.child(child,id)||self.state.generation.get()!=generation {let _=self.release();return None}
            let assigned=unsafe{SendMessageW(child,WM_GETFONT,None,None)};
            if assigned.0 as usize!=self.font.as_ref()?.font.0 as usize||!self.state.child(child,id)||self.state.generation.get()!=generation {let _=self.release();return None}
            if unsafe{SetWindowPos(child,Some(HWND_TOP),rect.left,rect.top,rect.width(),rect.height(),SWP_NOACTIVATE)}.is_err()||!self.state.child(child,id)||self.state.generation.get()!=generation {let _=self.release();return None}
        }
        Some(Prepared{plan,generation})
    }
    pub fn show(&mut self,prepared:Prepared)->bool {
        if self.state.generation.get()!=prepared.generation||!self.state.same_size(prepared.plan.rows.page.right,prepared.plan.rows.page.bottom){return false}
        for (child,id) in [(self.state.note.get(),NOTE_ID),(self.state.button.get(),BUTTON_ID)] {
            if !self.state.child(child,id){return false}
            unsafe {let _=ShowWindow(child,SW_SHOW);}
            if !self.state.child(child,id)||!unsafe{IsWindowVisible(child)}.as_bool()||self.state.generation.get()!=prepared.generation {let _=self.hide();return false}
        }
        if !self.state.child(self.state.note.get(),NOTE_ID)||!self.state.child(self.state.button.get(),BUTTON_ID)||self.state.generation.get()!=prepared.generation {let _=self.hide();return false}
        unsafe {let _=EnableWindow(self.state.note.get(),true);}
        if !self.state.child(self.state.note.get(),NOTE_ID)||!self.state.child(self.state.button.get(),BUTTON_ID)||self.state.generation.get()!=prepared.generation {let _=self.hide();return false}
        unsafe {let _=EnableWindow(self.state.button.get(),true);}
        if !self.state.child(self.state.button.get(),BUTTON_ID)||!self.state.child(self.state.note.get(),NOTE_ID)||!unsafe{IsWindowVisible(self.state.button.get())}.as_bool()||!unsafe{IsWindowVisible(self.state.note.get())}.as_bool()||!unsafe{IsWindowEnabled(self.state.note.get())}.as_bool()||!unsafe{IsWindowEnabled(self.state.button.get())}.as_bool()||!self.state.same_size(prepared.plan.rows.page.right,prepared.plan.rows.page.bottom)||self.state.generation.get()!=prepared.generation {let _=self.hide();return false}
        self.placement=Some(prepared.plan);self.state.published.set(true);true
    }
    pub fn note(&self)->HWND{self.state.note.get()}
    pub fn button(&self)->HWND{self.state.button.get()}
    pub fn clicks(&self)->u32{self.state.clicks.get()}
    pub fn retired_buttons(&self)->u32{self.state.retired_buttons.get()}
    pub fn owner_alive(&self)->bool{self.state.alive.get()}
}
impl Drop for Band {
    fn drop(&mut self) {
        let released=self.release();
        let detached=if self.state.owner_destroyed.get(){true}else if self.state.same_owner(){unsafe{RemoveWindowSubclass(self.state.owner,Some(notification),SUBCLASS_ID)}.as_bool()}else{false};
        let retired=released&&detached&&(self.state.owner_destroyed.get()||self.state.same_owner())&&self.claim.retire();
        if !retired {
            // Preserve uncertain native callback/font lifetime until this disposable process exits.
            // The test must assert explicit release before returning; this is not successful cleanup.
            self.state.published.set(false);
            if let Some(font)=self.font.take(){std::mem::forget(font)}
            let replacement=Box::new(Authority{owner:HWND::default(),pid:0,thread:0,button:Cell::new(HWND::default()),note:Cell::new(HWND::default()),button_alive:Cell::new(false),note_alive:Cell::new(false),alive:Cell::new(false),owner_destroyed:Cell::new(false),published:Cell::new(false),generation:Cell::new(0),clicks:Cell::new(0),retired_buttons:Cell::new(0)});
            std::mem::forget(std::mem::replace(&mut self.state,replacement));
            eprintln!("INSTALLER_BAND_COMPONENT native cleanup uncertain; fixture lifetime preserved, not accepted");
        }
    }
}

#[test]
#[ignore="requires a visible native disposable component and actual owned OS button input"]
fn actual_native_installer_band_component() {
    use std::{path::{Path,PathBuf},time::{Duration,Instant}};
    use windows::Win32::{Foundation::RECT,UI::{HiDpi::GetDpiForWindow,WindowsAndMessaging::{MSG,PeekMessageW,PM_REMOVE,TranslateMessage,DispatchMessageW,SetForegroundWindow,GetWindowRect,WS_POPUP}}};
    let args:Vec<_>=std::env::args().skip(1).collect();
    assert_eq!(args.iter().map(String::as_str).collect::<Vec<_>>(),vec!["chrome::store_guidance_layout::native_component::actual_native_installer_band_component","--exact","--ignored","--nocapture","--test-threads=1"]);
    let owner=unsafe{CreateWindowExW(WINDOW_EX_STYLE::default(),w!("STATIC"),w!("Bowser installer band component - no install authority"),WS_POPUP,0,0,1044,788,None,None,None,None)}.unwrap();
    struct Host(HWND);impl Drop for Host{fn drop(&mut self){if unsafe{IsWindow(Some(self.0))}.as_bool(){let _=unsafe{DestroyWindow(self.0)};}}}
    let host=Host(owner);unsafe{let _=ShowWindow(owner,SW_SHOW);}
    let dpi=unsafe{GetDpiForWindow(owner)};assert!((96..=768).contains(&dpi));
    let page=unsafe{CreateWindowExW(WINDOW_EX_STYLE::default(),w!("STATIC"),w!("Disposable reserved page area. This is not Google or an install-capable browser."),WS_CHILD,0,0,1,1,Some(owner),Some(HMENU(0x5b13_usize as *mut _)),None,None)}.unwrap();
    let mut band=Band::new(owner).unwrap();assert!(Band::new(owner).is_none(),"duplicate owner subclass must not replace the live component authority");let out=PathBuf::from("store-native-installer-band-component");std::fs::create_dir_all(&out).unwrap();
    for scale in [100_u32,150,200] {for width in [1044_i32,320] {
        assert!(band.hide(),"must hide controls before releasing old reservation");
        unsafe{SetWindowPos(owner,Some(HWND_TOP),0,0,width,788,SWP_NOACTIVATE|SWP_SHOWWINDOW).unwrap();}
        let mut client=RECT::default();unsafe{GetClientRect(owner,&mut client).unwrap()};
        let old_button=band.button();let old_retired=band.retired_buttons();
        let prepared=band.prepare(client.right,client.bottom,dpi,scale).expect("complete real-font component must fit these representative sizes");
        assert_eq!(band.retired_buttons(),old_retired+u32::from(old_button!=HWND::default()),"real old-button destruction must precede font recreation; no raw-HWND ABA inference");
        let area=prepared.plan.rows.page;
        unsafe{SetWindowPos(page,Some(HWND_TOP),area.left,area.top,area.width(),area.height(),SWP_NOACTIVATE|SWP_SHOWWINDOW).unwrap();}
        assert!(band.show(prepared));
        // No webpage/App/Store function is present: the only possible click effect is this counter.
        if scale==100&&width==1044 {unsafe{let _=SetForegroundWindow(owner);}}
        let sample=format!("font-{scale}-width-{width}");
        let mut command=std::process::Command::new("powershell.exe");command.args(["-NoProfile","-File"]).arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/scripts/probe-native-installer-band-component.ps1"));
        command.args(["-OwnerPid",&std::process::id().to_string(),"-OwnerHandle",&(owner.0 as isize).to_string(),"-OwnerExe",std::env::current_exe().unwrap().to_str().unwrap(),"-NoteHandle",&(band.note().0 as isize).to_string(),"-ButtonHandle",&(band.button().0 as isize).to_string(),"-PageHandle",&(page.0 as isize).to_string(),"-NoteLeft",&prepared.plan.note.left.to_string(),"-NoteTop",&prepared.plan.note.top.to_string(),"-NoteWidth",&prepared.plan.note.width().to_string(),"-NoteHeight",&prepared.plan.note.height().to_string(),"-ButtonLeft",&prepared.plan.button.left.to_string(),"-ButtonTop",&prepared.plan.button.top.to_string(),"-ButtonWidth",&prepared.plan.button.width().to_string(),"-ButtonHeight",&prepared.plan.button.height().to_string(),"-PageTop",&area.top.to_string(),"-FontScale",&scale.to_string(),"-Dpi",&dpi.to_string(),"-Sample",&sample,"-Out"]).arg(&out);
        if scale==100&&width==1044 {command.arg("-Click");}
        let stdout=std::fs::File::create(out.join(format!("{sample}.stdout.log"))).unwrap();let stderr=std::fs::File::create(out.join(format!("{sample}.stderr.log"))).unwrap();
        let mut child=command.stdout(std::process::Stdio::from(stdout)).stderr(std::process::Stdio::from(stderr)).spawn().unwrap();let started=Instant::now();let mut status=None;
        while started.elapsed()<Duration::from_secs(30){if let Some(done)=child.try_wait().unwrap(){status=Some(done);break}unsafe{let mut msg=MSG::default();while PeekMessageW(&mut msg,None,0,0,PM_REMOVE).as_bool(){let _=TranslateMessage(&msg);DispatchMessageW(&msg);}}std::thread::sleep(Duration::from_millis(10));}
        if status.is_none(){let _=child.kill();let _=child.wait();panic!("native component helper did not return; not accepted")}
        assert!(status.unwrap().success(),"owned native component capture/input failed; no visual acceptance");
        // Helper exit proves dispatch only; drain the owning STA until the actual notification arrives.
        let notification_started=Instant::now();
        loop {
            unsafe{let mut msg=MSG::default();while PeekMessageW(&mut msg,None,0,0,PM_REMOVE).as_bool(){let _=TranslateMessage(&msg);DispatchMessageW(&msg);}}
            if band.clicks()!=0||notification_started.elapsed()>=Duration::from_secs(2){break}
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(band.clicks(),1,"exactly one observed native first-sample click must be the only test-local notification");
        println!("INSTALLER_BAND_COMPONENT common_controls_context=unmanifested-libtest-not-production-v6 dpi={dpi} font_scale={scale} width={width} stacked={} row_height={} page_top={} captured=true visual_verdict=pending-independent-review install_authority=false",prepared.plan.stacked,prepared.plan.rows.guidance.unwrap().height(),area.top);
    }}
    assert!(band.hide());unsafe{SetWindowPos(owner,Some(HWND_TOP),0,0,120,100,SWP_NOACTIVATE|SWP_SHOWWINDOW).unwrap();}
    assert!(band.prepare(120,100,dpi,200).is_none(),"no-space state must not create a clipped/reserved component");
    assert!(band.placement.is_none());assert_eq!(band.clicks(),1);
    assert!(!band.state.button_alive.get()&&!band.state.note_alive.get());
    assert_eq!(band.button(),HWND::default());assert_eq!(band.note(),HWND::default());assert!(band.font.is_none());
    // The fixture caller must release the actual native page reservation on refusal, not only metadata.
    let mut tiny_client=RECT::default();unsafe{GetClientRect(owner,&mut tiny_client).unwrap();SetWindowPos(page,Some(HWND_TOP),0,0,tiny_client.right,tiny_client.bottom,SWP_NOACTIVATE|SWP_SHOWWINDOW).unwrap();}
    let mut tiny_owner=RECT::default();let mut tiny_page=RECT::default();unsafe{GetWindowRect(owner,&mut tiny_owner).unwrap();GetWindowRect(page,&mut tiny_page).unwrap();}
    assert_eq!((tiny_page.left,tiny_page.top,tiny_page.right-tiny_page.left,tiny_page.bottom-tiny_page.top),(tiny_owner.left,tiny_owner.top,tiny_client.right,tiny_client.bottom),"actual placeholder must fill the no-border tiny owner client after reservation release");
    assert!(band.release());assert_eq!(band.clicks(),1);let retired_token=band.claim.token;drop(band);
    let mut band=Band::new(owner).expect("successful real detach must permit a new registration");
    assert_ne!(band.claim.token,retired_token);assert!(!retire_claim(owner,retired_token,unsafe{GetCurrentThreadId()}));assert!(Band::new(owner).is_none());
    // Destroying the real owner invalidates child authority before the component object drops.
    unsafe{DestroyWindow(host.0).unwrap();}assert!(!band.owner_alive());assert!(band.release());
    assert_eq!(band.clicks(),0);drop(band);assert!(OwnerClaim::new(owner).is_some_and(|claim|claim.retire()));drop(host);
    println!("INSTALLER_BAND_COMPONENT no_space_refused=true actual_no_space_page_reservation_released=true actual_owner_close_observed=true only_test_local_notification=true no_production_activation=true");
}
