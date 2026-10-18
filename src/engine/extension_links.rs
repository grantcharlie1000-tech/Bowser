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
fn notice(hwnd: HWND, text: &str) { unsafe { let _ = MessageBoxW(Some(hwnd), &HSTRING::from(text), w!("Extension page links"), MB_OK | MB_ICONINFORMATION); } }
struct Busy(Rc<Cell<bool>>);
impl Drop for Busy { fn drop(&mut self) { self.0.set(false); } }

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
    let links = match &pending.content { Content::Links(links) => links, Content::Notice(text) => { notice(hwnd, text); return; } };
    let Ok(menu) = (unsafe { CreatePopupMenu() }) else { notice(hwnd, "The native Page links menu could not be created."); return; };
    for (index, link) in links.iter().enumerate() {
        let label = HSTRING::from(link.label.replace('&', "&&"));
        if unsafe { AppendMenuW(menu, MF_STRING, index + 1, &label) }.is_err() {
            unsafe { let _ = DestroyMenu(menu); } return;
        }
    }
    let mut point = POINT::default();
    let chosen = unsafe {
        if GetCursorPos(&mut point).is_err() { let _ = DestroyMenu(menu); return; }
        let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, point.x, point.y, Some(0), hwnd, None);
        let _ = DestroyMenu(menu); chosen.0 as usize
    };
    if !current() { return; }
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
