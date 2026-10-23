//! Test-only private-pipe membership service. A process list is only one conjunct:
//! the actual retained request, document/controller and exact native root must remain current.
use std::{cell::{Cell,RefCell},io::{Read,Write},path::Path,rc::Rc,sync::mpsc};
use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2,ICoreWebView2_2,ICoreWebView2Controller,ICoreWebView2Environment,ICoreWebView2Environment8,ICoreWebView2ContextMenuRequestedEventArgs,ICoreWebView2ContextMenuItemCollection,COREWEBVIEW2_PROCESS_KIND};
use windows::core::{Interface,IUnknown};
use windows::Win32::Foundation::{HWND,RECT,FILETIME};

const REQUEST:&str="BOWSER-MEMBERSHIP-V1";
const REPLY:&str="BOWSER-MEMBERSHIP-REPLY-V1";
fn identity<T:Interface>(v:&T)->Option<u64>{Some(v.cast::<IUnknown>().ok()?.as_raw() as u64)}
pub(super) struct Parts {
    pub view:ICoreWebView2,pub controller:ICoreWebView2Controller,pub environment:ICoreWebView2Environment,
    pub current:Box<dyn Fn()->Option<(u64,u64,u64)>>,
}
#[derive(Clone)]
struct Actual {args:ICoreWebView2ContextMenuRequestedEventArgs,items:ICoreWebView2ContextMenuItemCollection,args_id:u64,items_id:u64,view_id:u64}
#[derive(Default)]
pub(super) struct Capture {count:Cell<u32>,actual:RefCell<Option<Actual>>}
impl Capture {
    pub fn begin(&self){
        self.count.set(self.count.get().saturating_add(1));
        // No RefCell borrow spans COM, including release of an old retained request.
        let retired=self.actual.borrow_mut().take();drop(retired);
    }
    pub fn retain(&self,sender:Option<&ICoreWebView2>,args:Option<&ICoreWebView2ContextMenuRequestedEventArgs>,complete:bool){
        if self.count.get()!=1||!complete{return}
        let candidate=(||{let sender=sender?;let args=args?;let items=unsafe{args.MenuItems().ok()?};
            Some(Actual{args:args.clone(),args_id:identity(args)?,items_id:identity(&items)?,items,view_id:identity(sender)?})})();
        *self.actual.borrow_mut()=candidate;
    }
    fn current(&self,expected_view:u64)->Option<Actual>{
        if self.count.get()!=1{return None}let actual=self.actual.try_borrow().ok()?.clone()?;
        let mut handled=windows::core::BOOL::default();let mut editable=handled;
        unsafe{actual.args.Handled(&mut handled).ok()?;actual.args.ContextMenuTarget().ok()?.IsEditable(&mut editable).ok()?;}
        let items=unsafe{actual.args.MenuItems().ok()?};
        (self.count.get()==1&&actual.view_id==expected_view&&identity(&actual.args)==Some(actual.args_id)&&identity(&actual.items)==Some(actual.items_id)&&identity(&items)==Some(actual.items_id)&&!handled.as_bool()&&editable.as_bool()).then_some(actual)
    }
}
#[derive(Clone,Debug,PartialEq,Eq)]
struct Request {phase:u8,nonce:String,popup:usize,child:usize,gpu:u32,created:u64}
fn decimal(s:&str)->Option<u64>{if s.is_empty()||s.len()>20||!s.bytes().all(|b|b.is_ascii_digit()){return None}let n=s.parse::<u64>().ok()?;(n>0&&n.to_string()==s).then_some(n)}
impl Request {
    fn parse(line:&str)->Option<Self>{
        if line.len()>1024||!line.is_ascii(){return None}let f=line.split('|').collect::<Vec<_>>();
        if f.len()!=7||f[0]!=REQUEST||!matches!(f[1],"0"|"1")||f[2].len()!=32||!f[2].bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)){return None}
        let popup=usize::try_from(decimal(f[3])?).ok()?;let child=usize::try_from(decimal(f[4])?).ok()?;
        let gpu=u32::try_from(decimal(f[5])?).ok()?;if popup==child{return None}
        Some(Self{phase:f[1].parse().ok()?,nonce:f[2].into(),popup,child,gpu,created:decimal(f[6])?})
    }
}

// Read/query/synchronize only. No access fallback and no process-name lookup.
struct Process {handle:windows::Win32::Foundation::HANDLE,pid:u32,created:u64,image:String}
impl Process {
    fn open(pid:u32)->Option<Self>{unsafe{
        use windows::Win32::System::Threading::{OpenProcess,PROCESS_QUERY_LIMITED_INFORMATION,PROCESS_SYNCHRONIZE};
        let handle=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION|PROCESS_SYNCHRONIZE,false,pid).ok()?;
        let mut process=Self{handle,pid,created:0,image:String::new()};
        process.created=process.creation()?;process.image=process.image()?;
        if !Path::new(&process.image).file_name()?.to_str()?.eq_ignore_ascii_case("msedgewebview2.exe")||!process.live(){return None}Some(process)
    }}
    fn creation(&self)->Option<u64>{unsafe{
        let(mut created,mut exited,mut kernel,mut user)=(FILETIME::default(),FILETIME::default(),FILETIME::default(),FILETIME::default());
        windows::Win32::System::Threading::GetProcessTimes(self.handle,&mut created,&mut exited,&mut kernel,&mut user).ok()?;
        Some((u64::from(created.dwHighDateTime)<<32)|u64::from(created.dwLowDateTime))
    }}
    fn image(&self)->Option<String>{unsafe{
        let mut text=[0u16;32768];let mut n=text.len() as u32;
        windows::Win32::System::Threading::QueryFullProcessImageNameW(self.handle,windows::Win32::System::Threading::PROCESS_NAME_FORMAT(0),windows::core::PWSTR(text.as_mut_ptr()),&mut n).ok()?;
        String::from_utf16(&text[..n as usize]).ok()
    }}
    fn live(&self)->bool{unsafe{
        use windows::Win32::{System::Threading::{GetProcessId,WaitForSingleObject},Foundation::WAIT_TIMEOUT};
        GetProcessId(self.handle)==self.pid&&WaitForSingleObject(self.handle,0)==WAIT_TIMEOUT&&self.creation()==Some(self.created)&&self.image().is_some_and(|image|image.eq_ignore_ascii_case(&self.image))
    }}
}
impl Drop for Process {fn drop(&mut self){unsafe{let _=windows::Win32::Foundation::CloseHandle(self.handle);}}}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct Native {host_thread:u32,menu_thread:u32,child_thread:u32,focus:usize}
fn pid_thread(h:HWND)->(u32,u32){let mut p=0;let t=unsafe{windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(h,Some(&mut p))};(p,t)}
fn class(h:HWND,expected:&str)->bool{let mut b=[0u16;64];let n=unsafe{windows::Win32::UI::WindowsAndMessaging::GetClassNameW(h,&mut b)};n>0&&n<63&&String::from_utf16_lossy(&b[..n as usize])==expected}
fn native(host:HWND,browser:u32,r:&Request)->Option<Native>{unsafe{
    use windows::Win32::UI::{WindowsAndMessaging::*,Input::KeyboardAndMouse::IsWindowEnabled};
    let popup=HWND(r.popup as _);let child=HWND(r.child as _);
    let (owner,host_thread)=pid_thread(host);let (p,menu_thread)=pid_thread(popup);let (gpu,child_thread)=pid_thread(child);
    if owner!=std::process::id()||host_thread==0||p!=browser||menu_thread==0||gpu!=r.gpu||child_thread==0||!IsWindow(Some(host)).as_bool()||!class(host,"Static")||GetForegroundWindow()!=host{return None}
    // This owned host's fixed caption is checked without writing or switching foreground.
    let mut title=[0u16;64];let n=GetWindowTextW(host,&mut title);if n!=22||String::from_utf16_lossy(&title[..n as usize])!="bowser-extension-probe"{return None}
    let popup_ok=||IsWindow(Some(popup)).as_bool()&&IsWindowVisible(popup).as_bool()&&IsWindowEnabled(popup).as_bool()&&class(popup,"Chrome_WidgetWin_2")&&GetParent(popup).ok()==Some(host)&&GetWindow(popup,GW_OWNER).ok()==Some(host)&&GetAncestor(popup,GA_ROOTOWNER)==host&&GetAncestor(popup,GA_ROOT)==popup;
    // ChildWindowWin creates an input-transparent WS_DISABLED rendering child.
    // Only that structural child is disabled; the owned popup above remains enabled.
    let child_ok=||IsWindow(Some(child)).as_bool()&&IsWindowVisible(child).as_bool()&&!IsWindowEnabled(child).as_bool()&&class(child,"Intermediate D3D Window")&&GetParent(child).ok()==Some(popup)&&GetAncestor(child,GA_ROOT)==popup&&IsChild(popup,child).as_bool();
    let mut a=GUITHREADINFO{cbSize:std::mem::size_of::<GUITHREADINFO>() as u32,..Default::default()};let mut b=a;let mut rect=RECT::default();
    if !popup_ok()||!child_ok()||GetWindowRect(popup,&mut rect).is_err()||rect.right<=rect.left||rect.bottom<=rect.top||GetGUIThreadInfo(host_thread,&mut a).is_err()||GetGUIThreadInfo(menu_thread,&mut b).is_err(){return None}
    let focus=a.hwndFocus;
    if focus.is_invalid()||a.hwndActive!=host||b.hwndActive!=host||focus!=b.hwndFocus||a.hwndCapture!=popup||b.hwndCapture!=popup||pid_thread(focus)!=(browser,menu_thread)||!IsChild(host,focus).as_bool()||GetAncestor(focus,GA_ROOT)!=host{return None}
    if !popup_ok()||!child_ok()||pid_thread(host)!=(owner,host_thread)||pid_thread(popup)!=(p,menu_thread)||pid_thread(child)!=(gpu,child_thread)||GetForegroundWindow()!=host{return None}
    Some(Native{host_thread,menu_thread,child_thread,focus:focus.0 as usize})
}}

pub(super) struct Service {
    parts:Parts,capture:Rc<Capture>,host:HWND,browser:Process,gpu:Option<Process>,view:u64,controller:u64,environment:u64,
    proof:(u64,u64,u64),consumed:u8,previous:Option<Request>,native:Option<Native>,failed:bool,checkpoint:Cell<&'static str>,queries:u8,
}
impl Service {
    pub fn new(parts:Parts,capture:Rc<Capture>,host:HWND,browser:u32)->Option<Self>{
        let proof=(parts.current)()?;if proof.0==0||proof.1==0{return None}
        Some(Self{view:identity(&parts.view)?,controller:identity(&parts.controller)?,environment:identity(&parts.environment)?,parts,capture,host,browser:Process::open(browser)?,gpu:None,proof,consumed:0,previous:None,native:None,failed:false,checkpoint:Cell::new("created"),queries:0})
    }
    pub fn view_identity(&self)->u64{self.view}
    fn fence(&self,r:&Request)->Option<Actual>{
        self.checkpoint.set("current-view-controller-document-browser");
        if (self.parts.current)()!=Some(self.proof)||identity(&self.parts.view)!=Some(self.view)||identity(&self.parts.controller)!=Some(self.controller)||identity(&self.parts.environment)!=Some(self.environment)||!self.browser.live(){return None}
        self.checkpoint.set("actual-retained-context-request");let actual=self.capture.current(self.view)?;
        self.checkpoint.set("native-controller-environment-root-child-focus-capture");
        let mut parent=HWND::default();let mut visible=windows::core::BOOL::default();let mut browser=0;
        unsafe{self.parts.controller.ParentWindow(&mut parent).ok()?;self.parts.controller.IsVisible(&mut visible).ok()?;self.parts.view.BrowserProcessId(&mut browser).ok()?;}
        let environment=unsafe{self.parts.view.cast::<ICoreWebView2_2>().ok()?.Environment().ok()?};
        let gpu=self.gpu.as_ref()?;
        if parent!=self.host||!visible.as_bool()||browser!=self.browser.pid||identity(&environment)!=Some(self.environment)||gpu.pid!=r.gpu||gpu.created!=r.created||!gpu.live()||!gpu.image.eq_ignore_ascii_case(&self.browser.image)||native(self.host,browser,r)!=self.native||self.capture.count.get()!=1||(self.parts.current)()!=Some(self.proof){return None}
        Some(actual)
    }
    pub fn respond(&mut self,line:&str)->Option<String>{
        if self.failed{return None}let result=self.respond_checked(line);if result.is_none(){self.failed=true}result
    }
    fn respond_checked(&mut self,line:&str)->Option<String>{
        self.checkpoint.set("parse-exact-private-request");
        let r=Request::parse(line)?;
        if r.phase!=self.consumed||r.phase>1||r.gpu==self.browser.pid{return None}
        if let Some(previous)=&self.previous {if previous.nonce==r.nonce||previous.popup!=r.popup||previous.child!=r.child||previous.gpu!=r.gpu||previous.created!=r.created{return None}}
        self.checkpoint.set("retain-exact-native-child-process");
        if self.gpu.is_none(){let current=native(self.host,self.browser.pid,&r)?;self.gpu=Some(Process::open(r.gpu)?);self.native=Some(current);}
        let before=self.fence(&r)?;
        self.checkpoint.set("fresh-get-process-infos");self.queries=self.queries.saturating_add(1);
        let processes=unsafe{self.parts.environment.cast::<ICoreWebView2Environment8>().ok()?.GetProcessInfos().ok()?};
        let mut count=0;unsafe{processes.Count(&mut count).ok()?};if count==0||count>64{return None}
        let mut matches=0;
        for i in 0..count {let process=unsafe{processes.GetValueAtIndex(i).ok()?};let mut pid=0;let mut kind=COREWEBVIEW2_PROCESS_KIND(0);unsafe{process.ProcessId(&mut pid).ok()?;process.Kind(&mut kind).ok()?};if u32::try_from(pid).ok()==Some(r.gpu) {if kind.0!=4{return None}matches+=1;}}
        self.checkpoint.set("process-list-read-complete");let after=self.fence(&r)?;
        if matches!=1||before.args_id!=after.args_id||before.items_id!=after.items_id{return None}
        let reply=format!("{REPLY}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|4",r.phase,r.nonce,std::process::id(),self.view,after.args_id,self.controller,self.proof.0,self.proof.1,self.proof.2,self.host.0 as usize,r.popup,r.child,self.browser.pid,self.browser.created,r.gpu,r.created);
        if reply.len()>8192{return None}self.consumed+=1;self.previous=Some(r);self.checkpoint.set("fresh-response-issued");Some(reply)
    }
    fn diagnostic(&self)->String{format!(r#"{{"scope":"bounded-private-service-checkpoints-not-authority","responses":{},"fresh_query_calls":{},"failed":{},"checkpoint":"{}"}}"#,self.consumed,self.queries,self.failed,self.checkpoint.get())}
}

/// Only the exact spawned helper inherits these pipes. No filesystem/port rendezvous or
/// diagnostic input. Reader/writer threads never touch COM, UIA, process or native HWND APIs.
pub(super) struct Pipe {requests:mpsc::Receiver<Option<String>>,replies:mpsc::SyncSender<String>,failed:bool,evidence:std::path::PathBuf,reader:Option<std::thread::JoinHandle<()>>,writer:Option<std::thread::JoinHandle<()>>}
impl Pipe {
    pub fn attach(child:&mut std::process::Child,out:&Path)->Self{
        let mut input=child.stdout.take().expect("private helper stdout");let mut output=child.stdin.take().expect("private helper stdin");
        let (send,requests)=mpsc::sync_channel(3);let (replies,receive)=mpsc::sync_channel::<String>(2);let path=out.join("helper.stdout.log");
        let reader=std::thread::spawn(move||{
            let mut log=Vec::new();let mut line=Vec::new();let mut messages=0;let mut b=[0u8;1];
            while input.read(&mut b).ok()==Some(1){if b[0]==b'\n'{
                if line.last()==Some(&b'\r'){line.pop();}
                if line.starts_with(REQUEST.as_bytes()) {messages+=1;if messages>2||send.try_send(String::from_utf8(line.clone()).ok()).is_err(){let _=send.try_send(None);break}}
                else if log.len()+line.len()+1<=16384{log.extend_from_slice(&line);log.push(b'\n');}else{let _=send.try_send(None);break}
                line.clear();
            }else{line.push(b[0]);if line.len()>1024{let _=send.try_send(None);break}}}
            let _=std::fs::write(path,log);
        });
        let writer=std::thread::spawn(move||{while let Ok(reply)=receive.recv(){if writeln!(output,"{reply}").and_then(|_|output.flush()).is_err(){break}}});
        Self{requests,replies,failed:false,evidence:out.join("private-membership-service.json"),reader:Some(reader),writer:Some(writer)}
    }
    pub fn poll(&mut self,service:&mut Service){
        if self.failed{return}match self.requests.try_recv(){
            Ok(Some(line))=>{let reply=service.respond(&line);let _=std::fs::write(&self.evidence,service.diagnostic());match reply{Some(reply)=>{if self.replies.try_send(reply).is_err(){self.failed=true}},None=>{self.failed=true;let _=self.replies.try_send("BOWSER-MEMBERSHIP-REFUSED".into());}}},
            Ok(None)=>{self.failed=true;let _=self.replies.try_send("BOWSER-MEMBERSHIP-REFUSED".into());},
            Err(mpsc::TryRecvError::Empty|mpsc::TryRecvError::Disconnected)=>{},
        }
    }
    pub fn finish(mut self){drop(self.replies);if let Some(thread)=self.reader.take(){let _=thread.join();}if let Some(thread)=self.writer.take(){let _=thread.join();}}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_membership_request_is_exact_bounded_and_canonical(){
        let line="BOWSER-MEMBERSHIP-V1|0|aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa|20|21|7|70";
        assert_eq!(Request::parse(line).unwrap().gpu,7);
        for bad in [line.replace("|0|","|2|"),line.replace("|20|","|020|"),line.replace("|7|","|0|"),line.replace("|21|","|20|"),format!("{line}|extra"),line.replace('a',"A"),format!("{line}\r"),"x".repeat(1025)] {assert!(Request::parse(&bad).is_none(),"{bad}");}
    }
}
