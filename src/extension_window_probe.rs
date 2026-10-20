//! Test-only observation of existing native window-request handlers. No routing/authorization.
use std::{cell::RefCell, sync::atomic::{AtomicU64, Ordering}};
use url::Url;
use webview2_com::{CoTaskMemPWSTR, Microsoft::Web::WebView2::Win32::*};
use windows::core::{BOOL, Interface, PWSTR};
thread_local! { static REGISTERED_TOTAL: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; static REGISTERED: RefCell<Vec<(u64, &'static str)>> = const { RefCell::new(Vec::new()) }; static COUNTS: RefCell<std::collections::BTreeMap<&'static str, usize>> = const { RefCell::new(std::collections::BTreeMap::new()) }; static EXPECTED: RefCell<Option<Url>> = const { RefCell::new(None) }; static INTERVAL: std::cell::Cell<&'static str> = const { std::cell::Cell::new("startup-no-input") }; }
pub(crate) fn interval(value:&'static str){
    let thread=unsafe{windows::Win32::System::Threading::GetCurrentThreadId()};
    let previous=INTERVAL.with(std::cell::Cell::get);
    let registered=REGISTERED.with(|items|items.borrow().clone());
    let total=REGISTERED_TOTAL.with(std::cell::Cell::get);
    let counts=COUNTS.with(|counts|std::mem::take(&mut *counts.borrow_mut()));
    println!("NATIVE_WINDOW_INTERVAL phase=end native_thread={thread} scope=current-STA-only interval={previous} observed_counts={counts:?} registered_observers_first64={registered:?} total_registrations={total} coverage=observed-registrations-not-live-inventory-no-uninstantiated-host-claim");
    INTERVAL.with(|current|current.set(value));
    println!("NATIVE_WINDOW_INTERVAL phase=start native_thread={thread} scope=current-STA-only interval={value} registered_observers={registered:?}");
}
pub(crate) fn expected_destination(url: Option<Url>) { EXPECTED.with(|value| *value.borrow_mut() = url); }
fn native_text(raw: PWSTR, cap: usize) -> Option<String> {
    let allocation = CoTaskMemPWSTR::from(raw);
    let value = if raw.is_null() { None } else { unsafe {
        (0..=cap).find(|index| *raw.0.add(*index)==0)
            .and_then(|length| String::from_utf16(std::slice::from_raw_parts(raw.0,length)).ok())
    }};
    drop(allocation); value
}
fn uri_label(raw: PWSTR) -> String {
    let parsed = native_text(raw,4096).and_then(|text| Url::parse(&text).ok());
    let Some(url) = parsed else { return "unreadable_or_oversized".to_owned(); };
    let origin_kind = match (url.scheme(),url.host_str()) {
        ("chrome-extension",Some(id)) if crate::crx_fetch::parse_extension_id(id).is_ok() => "extension-origin",
        ("http"|"https",Some("127.0.0.1"|"localhost"|"[::1]")) => "controlled-loopback-origin",
        ("bowser-ui",_) => "privileged-ui-origin",
        _ => "other-origin-redacted",
    };
    let (exact_known_destination,same_expected_extension_origin) = EXPECTED.with(|expected| {
        let expected=expected.borrow(); let same_origin=expected.as_ref().is_some_and(|expected|
            crate::extensions::is_page_url(&url) && url.scheme()==expected.scheme() && url.host_str()==expected.host_str() && url.port()==expected.port());
        (expected.as_ref()==Some(&url),same_origin)
    });
    format!("scheme={} origin_kind={origin_kind} same_expected_extension_origin={same_expected_extension_origin} exact_known_destination={exact_known_destination}",url.scheme())
}
fn hr<T>(result: &windows::core::Result<T>) -> String { match result { Ok(_) => "ok".to_owned(),Err(error)=>format!("0x{:08x}",error.code().0 as u32) } }
#[derive(Clone)]
pub(crate) struct Observer { id:u64, role:&'static str, profile:String, private:Option<bool>, profile_status:String, thread:u32 }
impl Observer {
    pub(crate) fn prepare(role:&'static str, view:&ICoreWebView2) -> Self {
        static NEXT:AtomicU64=AtomicU64::new(1);
        let mut profile_name="unavailable".to_owned(); let mut private=None;
        let cast_result=view.cast::<ICoreWebView2_13>(); let cast_status=hr(&cast_result);
        let mut get_status="not-called".to_owned(); let mut name_status="not-called".to_owned(); let mut private_status="not-called".to_owned();
        if let Ok(view)=cast_result {
            let profile_result=unsafe{view.Profile()}; get_status=hr(&profile_result);
            if let Ok(profile)=profile_result {
                let mut raw=PWSTR::null(); let name_result=unsafe{profile.ProfileName(&mut raw)}; name_status=hr(&name_result);
                let text=native_text(raw,128); if name_result.is_ok(){profile_name=text.unwrap_or_else(||"unreadable".to_owned());}
                let mut flag=BOOL::default(); let private_result=unsafe{profile.IsInPrivateModeEnabled(&mut flag)}; private_status=hr(&private_result);
                if private_result.is_ok(){private=Some(flag.as_bool());}
            }
        }
        let profile_status=format!("cast={cast_status} get={get_status} name={name_status} private={private_status}");
        Self{id:NEXT.fetch_add(1,Ordering::Relaxed),role,profile:profile_name,private,profile_status,thread:unsafe{windows::Win32::System::Threading::GetCurrentThreadId()}}
    }
    pub(crate) fn installed(&self){
        REGISTERED_TOTAL.with(|total|total.set(total.get().saturating_add(1)));
        REGISTERED.with(|items|{let mut items=items.borrow_mut();if items.len()<64{items.push((self.id,self.role));}});
        println!("NATIVE_WINDOW_OBSERVER registered=true native_thread={} id={} role={} native_profile={:?} native_in_private={:?} profile_hresults=[{}]",self.thread,self.id,self.role,self.profile,self.private,self.profile_status);
    }
    pub(crate) fn enter(&self,sender:Option<&ICoreWebView2>,args:&ICoreWebView2NewWindowRequestedEventArgs)->u64{
        static NEXT:AtomicU64=AtomicU64::new(1);let request=NEXT.fetch_add(1,Ordering::Relaxed);
        COUNTS.with(|counts|{let mut counts=counts.borrow_mut();let count=counts.entry(self.role).or_default();*count=count.saturating_add(1);});
        let mut raw=PWSTR::null();let target_result=unsafe{args.Uri(&mut raw)};let target=uri_label(raw);
        let mut initiated=BOOL::default();let gesture_result=unsafe{args.IsUserInitiated(&mut initiated)};
        let mut source=PWSTR::null();let source_result=sender.map(|view|unsafe{view.Source(&mut source)});let source=uri_label(source);
        let frame_result=args.cast::<ICoreWebView2NewWindowRequestedEventArgs3>().and_then(|args|unsafe{args.OriginalSourceFrameInfo()});
        let mut frame_source=PWSTR::null();let frame_uri_result=frame_result.as_ref().ok().map(|frame|unsafe{frame.Source(&mut frame_source)});let frame_source=uri_label(frame_source);
        let thread=unsafe{windows::Win32::System::Threading::GetCurrentThreadId()};
        let interval=INTERVAL.with(std::cell::Cell::get);
        println!("NATIVE_WINDOW_REQUEST phase=entry native_thread={thread} registration_thread={} scope=current-STA-only observed_interval={interval} request={request} observer={} role={} target_hresult={} target=[{target}] receiving_source_hresult={:?} receiving_source=[{source}] gesture_hresult={} native_user_initiated={} original_frame_hresult={} original_frame_uri_hresult={:?} original_frame_source=[{frame_source}]",self.thread,self.id,self.role,hr(&target_result),source_result.as_ref().map(hr),hr(&gesture_result),initiated.as_bool(),hr(&frame_result),frame_uri_result.as_ref().map(hr));
        request
    }
    pub(crate) fn finish(&self,request:u64,args:&ICoreWebView2NewWindowRequestedEventArgs,decision:&str,callback_present:bool){
        let mut handled=BOOL::default();let result=unsafe{args.Handled(&mut handled)};
        println!("NATIVE_WINDOW_REQUEST phase=policy_decision request={request} observer={} role={} decision={decision} callback_present={callback_present} handled_hresult={} native_handled={}",self.id,self.role,hr(&result),handled.as_bool());
    }
}
