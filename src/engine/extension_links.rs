//! An explicit Bowser-owned navigation aid for the current isolated extension page. This is not
//! chrome.tabs/action emulation: no page-callable channel, new controller or browsing-tab routing.
use std::{cell::{Cell, RefCell}, rc::Rc};
use url::Url;
use webview2_com::{CoTaskMemPWSTR, Microsoft::Web::WebView2::Win32::{ICoreWebView2, ICoreWebView2ExecuteScriptCompletedHandler, ICoreWebView2ExecuteScriptCompletedHandler_Impl}};
use windows::{core::{HSTRING, HRESULT, PCWSTR, PWSTR, implement, w}, Win32::{Foundation::{HWND, POINT, WPARAM, LPARAM}, UI::WindowsAndMessaging::{AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, MB_ICONINFORMATION, MB_OK, MF_STRING, MessageBoxW, PostMessageW, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu}}};
use super::webview2::HostedPagePolicy;
use crate::extensions::json::{self, Value};
const MAX_LINKS: usize = 32;
const MAX_RESULT: usize = 128 * 1024;
const MAX_URL: usize = 2048;
const READ_LINKS: &str = r#"(()=>{const out=[];const anchors=document.getElementsByTagName('a');for(let i=0;i<anchors.length&&i<512&&out.length<32;i++){const a=anchors[i];if(!a.hasAttribute('href'))continue;const href=a.href;if(typeof href!=='string'||href.length>2048)continue;let u;try{u=new URL(href)}catch{continue}if(u.protocol!==location.protocol||u.host!==location.host)continue;out.push({href,label:''})}return out})()"#;

/// Avoid the convenience callback's unbounded PCWSTR->String copy before enforcing our cap.
unsafe fn bounded_text(value: PCWSTR, cap: usize) -> Option<String> {
    if value.is_null() { return None; }
    let length = (0..=cap).find(|index| unsafe { *value.0.add(*index) } == 0)?;
    let text = String::from_utf16(unsafe { std::slice::from_raw_parts(value.0, length) }).ok()?;
    (text.len() <= cap).then_some(text)
}
type ReadCompletion = Box<dyn FnOnce(windows::core::Result<()>, Option<String>) -> windows::core::Result<()>>;
#[implement(ICoreWebView2ExecuteScriptCompletedHandler)]
struct BoundedCompletion(RefCell<Option<ReadCompletion>>);
impl ICoreWebView2ExecuteScriptCompletedHandler_Impl for BoundedCompletion_Impl {
    fn Invoke(&self, status: HRESULT, value: &PCWSTR) -> windows::core::Result<()> {
        let callback = self.0.borrow_mut().take();
        if let Some(callback) = callback { callback(status.ok(), unsafe { bounded_text(*value, MAX_RESULT) })?; }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Link { destination: Url, label: String }
fn clean_label(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_control() && !crate::extensions::is_invisible_format(*ch)).take(80).collect::<String>().trim().to_owned()
}
fn parse(text: &str, policy: &HostedPagePolicy) -> Result<Vec<Link>, ()> {
    if text.len() > MAX_RESULT { return Err(()); }
    let Some(Value::Array(values)) = json::parse(text) else { return Err(()); };
    if values.len() > MAX_LINKS { return Err(()); }
    let mut links = Vec::new();
    for value in values {
        let Value::Object(fields) = value else { return Err(()); };
        if fields.len() != 2 { return Err(()); }
        let (Some(Value::String(href)), Some(Value::String(label))) = (json::get(&fields, "href"), json::get(&fields, "label")) else { return Err(()); };
        if href.len() > MAX_URL || label.len() > 512 || label.chars().count() > 128 { return Err(()); }
        let Ok(destination) = Url::parse(href) else { continue; };
        if !policy.allow_new_window(&destination) || links.iter().any(|link: &Link| link.destination == destination) { continue; }
        // Display neither query nor fragment. The exact parsed destination remains bound to its
        // native menu item internally, so navigation never reconstructs/guesses a manager path.
        let filename = clean_label(destination.path().rsplit('/').next().unwrap_or("page"));
        let label = if filename.is_empty() { "Extension page".to_owned() } else { filename };
        links.push(Link { destination, label });
    }
    Ok(links)
}
fn source(webview: &ICoreWebView2) -> Option<Url> {
    let mut raw = PWSTR::null(); unsafe { webview.Source(&mut raw) }.ok()?;
    let allocation = CoTaskMemPWSTR::from(raw);
    let text = unsafe { bounded_text(PCWSTR(raw.0), MAX_URL) };
    drop(allocation); text.and_then(|text| Url::parse(&text).ok())
}
fn still_current(webview: &ICoreWebView2, alive: &Cell<bool>, navigation: &Navigation, captured: u64, original: &Url, policy: &HostedPagePolicy) -> bool {
    navigation.current(alive.get(), captured) && source(webview).as_ref() == Some(original) && navigation.current(alive.get(), captured) && policy.allow_new_window(original)
}
fn notice(hwnd: HWND, text: &str) { #[cfg(test)] let _watchdog = ModalWatchdog::new(hwnd, ModalKind::Notice); unsafe { let _ = MessageBoxW(Some(hwnd), &HSTRING::from(text), w!("Extension page links"), MB_OK | MB_ICONINFORMATION); } }
struct Busy(Rc<Cell<bool>>);
impl Drop for Busy { fn drop(&mut self) { self.0.set(false); } }

// Fixed Rust-only disposable runtime controls. No production message, page bridge or configurable
// script endpoint is registered. The real native timer performs a real same-URL WebView Reload.
#[cfg(test)]
pub(super) const PROBE_RELOAD_TIMER: usize = 0x4a1;
#[cfg(test)]
thread_local! {
    static RELOAD_NEXT_MENU: Cell<bool> = const { Cell::new(false) };
    static RELOAD_PERFORMED: Cell<bool> = const { Cell::new(false) };
    static STALE_SELECTION_REFUSED: Cell<bool> = const { Cell::new(false) };
    static EMPTY_NOTICE_SHOWN: Cell<bool> = const { Cell::new(false) };
}
#[cfg(test)]
pub(super) fn arm_probe_reload() { RELOAD_NEXT_MENU.with(|flag| flag.set(true)); }
#[cfg(test)]
pub(super) fn probe_reload_result(success: bool) { RELOAD_PERFORMED.with(|flag| flag.set(success)); }
#[cfg(test)]
pub(super) fn probe_results() -> (bool, bool, bool) {
    (RELOAD_PERFORMED.with(Cell::get), STALE_SELECTION_REFUSED.with(Cell::get), EMPTY_NOTICE_SHOWN.with(Cell::get))
}

// A Rust/STA pump deadline cannot interrupt DispatchMessage inside a real native modal loop.
// Unique, test-only timer IDs and exact currently-armed ownership reject stale timer messages.
#[cfg(test)]
#[derive(Clone, Copy, PartialEq)]
enum ModalKind { Menu, Notice }
#[cfg(test)]
thread_local! {
    static MODAL_TIMED_OUT: Cell<bool> = const { Cell::new(false) };
    static MODAL_NEXT_ID: Cell<usize> = const { Cell::new(0x5a00_0000) };
    static ACTIVE_MODAL: RefCell<Option<(HWND,usize,ModalKind,std::time::Instant)>> = const { RefCell::new(None) };
}
#[cfg(test)]
struct ModalWatchdog(HWND,usize);
#[cfg(test)]
impl ModalWatchdog {
    fn new(hwnd: HWND, kind: ModalKind) -> Self {
        let id=MODAL_NEXT_ID.with(|next| { let id=next.get(); next.set(id.checked_add(1).filter(|id|*id<0x5b00_0000).expect("test timer ID space exhausted")); id });
        ACTIVE_MODAL.with(|active|assert!(active.borrow().is_none(),"test modal watchdog cannot replace another owner"));
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(60);
        let timer = unsafe { windows::Win32::UI::WindowsAndMessaging::SetTimer(Some(hwnd), id, 250, None) };
        assert_ne!(timer,0,"test-only native modal watchdog was not installed");
        ACTIVE_MODAL.with(|active|*active.borrow_mut()=Some((hwnd,id,kind,deadline))); Self(hwnd,id)
    }
}
#[cfg(test)]
impl Drop for ModalWatchdog {
    fn drop(&mut self) {
        ACTIVE_MODAL.with(|active| { let matches=active.borrow().as_ref().is_some_and(|(hwnd,id,_,_)|*hwnd==self.0&&*id==self.1); if matches { active.borrow_mut().take(); } });
        unsafe { let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(self.0),self.1); }
    }
}
#[cfg(test)]
pub(super) fn probe_modal_timer(hwnd: HWND,id:usize) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{EndMenu, GetClassNameW, GetGUIThreadInfo, GetLastActivePopup, GetWindow, GetWindowTextW, GUITHREADINFO, GUI_POPUPMENUMODE, GW_OWNER, KillTimer, PostMessageW, WM_CLOSE};
    if !(0x5a00_0000..0x5b00_0000).contains(&id) { return false; }
    let active=ACTIVE_MODAL.with(|active|*active.borrow());
    let Some((owner,armed_id,kind,deadline))=active.filter(|(owner,armed_id,_,_)|*owner==hwnd&&*armed_id==id) else { return true; };
    if std::time::Instant::now()<deadline { return true; }
    MODAL_TIMED_OUT.with(|flag|flag.set(true));
    unsafe {
        let _ = KillTimer(Some(owner),armed_id);
        if kind==ModalKind::Menu {
            let mut info=GUITHREADINFO{cbSize:std::mem::size_of::<GUITHREADINFO>() as u32,..Default::default()};
            if GetGUIThreadInfo(windows::Win32::System::Threading::GetCurrentThreadId(),&mut info).is_ok() && info.hwndMenuOwner==owner && info.flags.contains(GUI_POPUPMENUMODE) { let _ = EndMenu(); }
        } else {
            let popup = GetLastActivePopup(owner); let mut class=[0u16;64]; let mut title=[0u16;64];
            let class_len=GetClassNameW(popup,&mut class).max(0) as usize;
            let title_len=GetWindowTextW(popup,&mut title).max(0) as usize;
            if popup!=owner && GetWindow(popup,GW_OWNER).ok()==Some(owner)
                && String::from_utf16_lossy(&class[..class_len])=="#32770"
                && String::from_utf16_lossy(&title[..title_len])=="Extension page links" {
                let _ = PostMessageW(Some(popup),WM_CLOSE,WPARAM(0),LPARAM(0));
            }
        }
    }
    println!("EXTENSION_PAGE_LINKS native_modal_watchdog_expired=true acceptance_failed=true"); true
}
#[cfg(test)]
pub(super) fn assert_no_modal_timeout() { assert!(!MODAL_TIMED_OUT.with(Cell::get),"actual native modal input timed out; cancellation is not acceptance"); }

pub(super) const SHOW_MESSAGE: u32 = 0x8000 + 0x4a0;
#[derive(Clone, Default)]
pub(super) struct Navigation { pub(super) epoch: Rc<Cell<u64>>, pub(super) in_flight: Rc<Cell<bool>>, pub(super) navigation_id: Rc<Cell<Option<u64>>> }
impl Navigation {
    pub(super) fn invalidate(&self) { self.epoch.set(self.epoch.get().saturating_add(1)); }
    fn current(&self, alive: bool, captured: u64) -> bool { alive && !self.in_flight.get() && captured != u64::MAX && self.epoch.get() == captured }
}
enum Content { Links(Vec<Link>), Notice(&'static str) }
pub(super) struct PendingMenu {
    view: ICoreWebView2, policy: Rc<HostedPagePolicy>, active: Rc<Cell<bool>>,
    navigation: Navigation, captured: u64, original: Url, content: Content, _busy: Busy,
}
pub(super) type MenuQueue = Rc<RefCell<Option<PendingMenu>>>;

pub(super) fn open_pending(hwnd: HWND, queue: MenuQueue) {
    let pending = queue.borrow_mut().take();
    let Some(pending) = pending else { return; };
    let current = || still_current(&pending.view, &pending.active, &pending.navigation, pending.captured, &pending.original, &pending.policy);
    if !current() { return; }
    #[cfg(test)] println!("EXTENSION_PAGE_LINKS deferred_native_dispatch=true current_document=true");
    let links = match &pending.content { Content::Links(links) => links, Content::Notice(text) => {
        #[cfg(test)] if *text == "No eligible same-extension page links are present on this page." { EMPTY_NOTICE_SHOWN.with(|flag| flag.set(true)); }
        notice(hwnd, text); return;
    } };
    let Ok(menu) = (unsafe { CreatePopupMenu() }) else { notice(hwnd, "The native Page links menu could not be created."); return; };
    for (index, link) in links.iter().enumerate() {
        let label = HSTRING::from(link.label.replace('&', "&&"));
        if unsafe { AppendMenuW(menu, MF_STRING, index + 1, &label) }.is_err() {
            unsafe { let _ = DestroyMenu(menu); } return;
        }
    }
    #[cfg(test)] if RELOAD_NEXT_MENU.with(|flag| flag.replace(false)) {
        let timer = unsafe { windows::Win32::UI::WindowsAndMessaging::SetTimer(Some(hwnd), PROBE_RELOAD_TIMER, 500, None) };
        assert_ne!(timer, 0, "real reload control timer was not installed");
    }
    let mut point = POINT::default();
    let chosen = unsafe {
        if GetCursorPos(&mut point).is_err() { let _ = DestroyMenu(menu); return; }
        #[cfg(test)] let watchdog = ModalWatchdog::new(hwnd,ModalKind::Menu);
        let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, point.x, point.y, Some(0), hwnd, None);
        #[cfg(test)] drop(watchdog);
        let _ = DestroyMenu(menu); chosen.0 as usize
    };
    if !current() {
        #[cfg(test)] if chosen != 0 { STALE_SELECTION_REFUSED.with(|flag| flag.set(true)); println!("EXTENSION_PAGE_LINKS stale_native_selection_refused=true"); }
        return;
    }
    if let Some(link) = chosen.checked_sub(1).and_then(|index| links.get(index)).filter(|link| pending.policy.allow_new_window(&link.destination)) {
        #[cfg(test)] println!("EXTENSION_PAGE_LINKS actual_native_menu_selection=true destination_policy_valid=true");
        let address = CoTaskMemPWSTR::from(link.destination.as_str());
        if unsafe { pending.view.Navigate(*address.as_ref().as_pcwstr()) }.is_err() && pending.active.get() { notice(hwnd, "The selected extension page could not be opened."); }
    }
}

pub(super) fn show(hwnd: HWND, webview: ICoreWebView2, policy: Rc<HostedPagePolicy>, alive: Rc<Cell<bool>>, navigation: Navigation, busy: Rc<Cell<bool>>, queued: MenuQueue) {
    if !alive.get() || busy.replace(true) { return; }
    let Some(original) = source(&webview).filter(|url| policy.allow_new_window(url)) else {
        busy.set(false); if alive.get() { notice(hwnd, "Wait for this extension page to finish opening, then try Page links again."); } return;
    };
    #[cfg(test)] println!("EXTENSION_PAGE_LINKS native_user_command=true");
    let captured = navigation.epoch.get();
    if navigation.in_flight.get() { busy.set(false); if alive.get() { notice(hwnd, "Wait for this extension page to finish navigating, then try Page links again."); } return; }
    if captured == u64::MAX { busy.set(false); if alive.get() { notice(hwnd, "This page changed too often. Close and reopen its extension window."); } return; }
    let view = webview.clone(); let active = alive.clone(); let work = busy.clone();
    let handler: ICoreWebView2ExecuteScriptCompletedHandler = BoundedCompletion(RefCell::new(Some(Box::new(move |status, value| {
        let busy_guard = Busy(work);
        if !still_current(&view, &active, &navigation, captured, &original, &policy) { return Ok(()); }
        let content = if status.is_err() { Content::Notice("Links could not be read from this extension page. Nothing was opened.") }
        else { match value.as_deref().and_then(|text| parse(text, &policy).ok()) {
            Some(links) if !links.is_empty() => Content::Links(links),
            Some(_) => Content::Notice("No eligible same-extension page links are present on this page."),
            None => Content::Notice("This extension page returned invalid or oversized link information. Nothing was opened."),
        } };
        // Do not enter a modal native menu from a COM completion: an accessibility invoker may
        // still be waiting for its command to return. Queue only these validated Rust-owned
        // values, then post a private zero-payload window notification after the callback ends.
        if queued.borrow().is_some() { return Ok(()); }
        *queued.borrow_mut() = Some(PendingMenu { view, policy, active, navigation, captured, original, content, _busy: busy_guard });
        if unsafe { PostMessageW(Some(hwnd), SHOW_MESSAGE, WPARAM(0), LPARAM(0)) }.is_err() {
            drop(queued.borrow_mut().take());
        }
        Ok(())
    })))).into();
    let script = CoTaskMemPWSTR::from(READ_LINKS);
    if unsafe { webview.ExecuteScript(*script.as_ref().as_pcwstr(), &handler) }.is_err() {
        busy.set(false); if alive.get() { notice(hwnd, "Links could not be requested. Nothing was opened."); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn policy() -> HostedPagePolicy {
        let url = Url::parse("chrome-extension://abcdefghijklmnopabcdefghijklmnop/popup.html").unwrap();
        let policy = HostedPagePolicy::new(&url).unwrap(); assert!(policy.allow_navigation(&url)); policy
    }
    #[test]
    fn native_page_links_reject_same_url_document_replacement_and_inflight_navigation() {
        let navigation = Navigation::default();
        assert!(navigation.current(true, 0));
        navigation.in_flight.set(true); navigation.invalidate();
        assert!(!navigation.current(true, 1), "already-started reload must not capture the old document");
        navigation.invalidate(); // top-level ContentLoading, including a same-URL document
        navigation.in_flight.set(false); navigation.invalidate(); // NavigationCompleted
        assert!(!navigation.current(true, 0)); assert!(!navigation.current(true, 1));
        assert!(navigation.current(true, 3)); assert!(!navigation.current(false, 3));
        navigation.epoch.set(u64::MAX); assert!(!navigation.current(true, u64::MAX));
    }
    #[test]
    fn native_page_links_preserve_actual_query_and_fragment_without_displaying_them() {
        let links = parse(r#"[{"href":"chrome-extension://abcdefghijklmnopabcdefghijklmnop/manage.html?state=private-token#secret","label":"m"}]"#, &policy()).unwrap();
        assert_eq!(links[0].label, "manage.html"); assert_eq!(links[0].destination.query(), Some("state=private-token")); assert_eq!(links[0].destination.fragment(), Some("secret"));
        let links = parse(r#"[{"href":"chrome-extension://abcdefghijklmnopabcdefghijklmnop/manage.html?state=private-token#secret","label":"chrome-extension://abcdefghijklmnopabcdefghijklmnop/manage.html?state=private-token#secret"}]"#, &policy()).unwrap();
        assert_eq!(links[0].label, "manage.html", "page-supplied URL labels must never reveal query/fragment values");
    }
    #[test]
    fn native_page_links_refuse_external_ui_other_extension_and_bad_shapes() {
        for href in ["https://example.test/", "bowser-ui://app/index.html", "file:///C:/private", "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/manage.html", "javascript:alert(1)"] {
            let text = format!("[{{\"href\":{},\"label\":\"external\"}}]", crate::chrome_host::protocol::json_string(href)); assert!(parse(&text, &policy()).unwrap().is_empty());
        }
        for text in ["null", "{}", "[1]", r#"[{"href":42,"label":"x"}]"#, r#"[{"href":"x","label":"x","other":true}]"#] { assert!(parse(text, &policy()).is_err()); }
        assert!(parse(&"x".repeat(MAX_RESULT + 1), &policy()).is_err());
    }
    #[test]
    fn native_page_links_enforce_count_size_and_clean_labels() {
        let item = r#"{"href":"chrome-extension://abcdefghijklmnopabcdefghijklmnop/manage.html","label":"Manage"}"#;
        assert!(parse(&format!("[{}]", vec![item; MAX_LINKS + 1].join(",")), &policy()).is_err());
        assert_eq!(clean_label("\u{202e}Manage\n\u{200b}"), "Manage");
        assert_eq!(parse(&format!("[{item},{item}]"), &policy()).unwrap().len(), 1);
    }
    #[test]
    fn native_page_links_bound_native_string_copy_before_parsing() {
        let text: Vec<u16> = "bounded".encode_utf16().chain(Some(0)).collect();
        assert_eq!(unsafe { bounded_text(PCWSTR(text.as_ptr()), 7) }.as_deref(), Some("bounded"));
        assert!(unsafe { bounded_text(PCWSTR(text.as_ptr()), 6) }.is_none());
        assert!(unsafe { bounded_text(PCWSTR::null(), 7) }.is_none());
        let invalid = [0xd800, 0]; assert!(unsafe { bounded_text(PCWSTR(invalid.as_ptr()), 7) }.is_none());
    }

}
