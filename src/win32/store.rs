//! UI-thread Store state and a bounded background preparation worker. Website WebViews have no
//! handler here. Only the trusted Bowser settings host can approve a matching one-shot review.
use std::{cell::RefCell, collections::HashMap, path::PathBuf, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use crate::{app::{App, AppRef}, chrome_host::{json::Json, protocol::PanelFields}, internal_page::InternalPage, store};

#[derive(Default)]
struct State {
    request: String, id: String, name: String, version: String, phase: &'static str,
    error: String, restart: bool, profile: PathBuf,
    cancelled: Option<Arc<AtomicBool>>, review: Option<store::Review>,
}
thread_local! { static STATES: RefCell<HashMap<isize, State>> = RefCell::new(HashMap::new()); }
struct Completed { owner: isize, request: String, outcome: Result<store::Review, String> }
static COMPLETED: Mutex<Vec<Completed>> = Mutex::new(Vec::new());

fn with<T>(owner: HWND, f: impl FnOnce(&mut State) -> T) -> T {
    STATES.with(|states| f(states.borrow_mut().entry(owner.0 as isize).or_default()))
}
fn changed(owner: HWND) { super::settings_page::store_changed(owner); }
fn error(owner: HWND, text: impl Into<String>) {
    with(owner, |state| { state.phase = "error"; state.error = text.into().chars().take(1500).map(|c| if c.is_control() { ' ' } else { c }).collect(); state.review = None; });
    changed(owner);
}

pub(crate) fn open(owner: HWND, app: &AppRef, reuse_blank: bool) {
    if !super::settings_page::usable(owner) { super::show_error_message("The Bowser Store needs Bowser's HTML interface. Restart with the default HTML interface."); return; }
    let _ = App::open_internal_page(app, InternalPage::Store, reuse_blank);
    changed(owner);
}

pub(crate) fn close(owner: HWND) {
    STATES.with(|states| { if let Some(state) = states.borrow_mut().remove(&(owner.0 as isize)) { if let Some(cancelled) = state.cancelled { cancelled.store(true, Ordering::Release); } } });
    COMPLETED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).retain(|item| item.owner != owner.0 as isize);
}

pub(crate) fn start(owner: HWND, app: &AppRef, id: &str) {
    if !super::settings_page::usable(owner) { return error(owner, "The Store requires the HTML interface. Nothing was downloaded."); }
    if crate::crx_fetch::parse_extension_id(id).is_err() { return error(owner, "That extension ID is not valid."); }
    if with(owner, |state| matches!(state.phase, "downloading" | "review" | "installing")) { return; }
    let Ok(profile) = crate::profile::active() else { return error(owner, "The profile is not ready."); };
    if profile.is_guest() { return error(owner, "Use a persistent normal profile to install extensions."); }
    let runtime = match crate::engine::WebView2Engine::availability() { Ok(runtime) => runtime, Err(reason) => return error(owner, reason.0) };
    if !crate::engine::WebView2Engine::extension_runtime_is_supported(&runtime) {
        return error(owner, "This WebView2 runtime cannot run supported extensions. Update the WebView2 runtime before installing; restarting alone will not fix this. Nothing was installed.");
    }
    let version = runtime.split_whitespace().next().unwrap_or_default().to_owned();
    let request = store::request_id();
    let cancelled = Arc::new(AtomicBool::new(false));
    let restart = crate::extensions::read_records(&profile.extensions_file()).is_ok_and(|records| records.is_empty());
    with(owner, |state| *state = State { request: request.clone(), id: id.to_owned(), name: store::entry(id).map_or_else(String::new, |entry| entry.name.to_owned()), phase: "downloading", restart, profile: profile.root().to_owned(), cancelled: Some(cancelled.clone()), ..State::default() });
    open(owner, app, false);
    let raw = owner.0 as isize;
    let worker_id = id.to_owned();
    let spawned = std::thread::Builder::new().name("bowser-store-download".into()).spawn(move || {
        let outcome = store::prepare(&worker_id, &version, &profile.extensions_dir(), &profile.extensions_file(), &cancelled);
        // Never keep a prepared copy for a cancelled/closed window waiting for another event.
        let mut completed = COMPLETED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if cancelled.load(Ordering::Acquire) { return; }
        completed.push(Completed { owner: raw, request, outcome });
        drop(completed);
        super::post(HWND(raw as *mut _), super::WM_SETTINGS_COMMAND, WPARAM(0), LPARAM(0));
    });
    if spawned.is_err() { error(owner, "The extension download could not be started."); }
}

pub(crate) fn apply_completed(owner: HWND) {
    let mine = {
        let mut waiting = COMPLETED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mine, rest): (Vec<_>, Vec<_>) = waiting.drain(..).partition(|item| item.owner == owner.0 as isize);
        *waiting = rest; mine
    };
    for item in mine {
        let current = with(owner, |state| state.request == item.request && state.phase == "downloading" && state.cancelled.as_ref().is_some_and(|cancelled| !cancelled.load(Ordering::Acquire)));
        if !current { continue; }
        match item.outcome {
            Ok(review) => {
                with(owner, |state| { state.name = review.name.clone(); state.version = review.prepared.inspection().manifest.version.clone(); state.phase = "review"; state.review = Some(review); });
                // Never shorten a permission review to fit the bridge. Over-budget fails closed.
                if snapshot(owner, true).len() > 240 * 1024 { error(owner, "The full permission review is too large to show safely. Nothing was installed."); }
                changed(owner);
            }
            Err(reason) => error(owner, reason),
        }
    }
}

pub(crate) fn snapshot(owner: HWND, shown: bool) -> String {
    with(owner, |state| crate::chrome_host::protocol::snap_json("store", |out| {
        out.bool("shown", shown);
        out.array("catalogue", |items| for entry in store::CATALOGUE { items.object(|item| {
            item.string("id", entry.id); item.string("name", entry.name); item.string("description", entry.description);
            item.string("publisher", entry.publisher); item.string("category", entry.category);
            item.string("source", entry.source); item.string("homepage", entry.homepage);
            item.string("provenance", "Manually curated from the linked publisher and Chrome Web Store listing. Not an all-store search or availability guarantee.");
            item.string("checked", entry.checked); item.string("compatibility", "untested");
            item.string("compatibilityNote", "No version-specific functionality verdict yet. Installation alone does not prove compatibility.");
        }); });
        out.string("phase", if state.phase.is_empty() { "idle" } else { state.phase });
        out.string("requestId", &state.request); out.string("extensionId", &state.id); out.string("name", &state.name); out.string("version", &state.version);
        out.string("error", &state.error); out.bool("restart", state.restart);
        let manifest = state.review.as_ref().map(|review| &review.prepared.inspection().manifest);
        for (key, values) in [
            ("permissions", manifest.map(|m| &m.permissions)), ("hostPermissions", manifest.map(|m| &m.host_permissions)),
            ("contentScripts", manifest.map(|m| &m.content_script_matches)), ("optionalPermissions", manifest.map(|m| &m.optional_permissions)),
            ("optionalHostPermissions", manifest.map(|m| &m.optional_host_permissions)),
        ] { out.array(key, |items| if let Some(values) = values { for value in values { items.string(value); } }); }
    }).expect("store is a panel token"))
}

/// Exact-field validation happens before dispatch. No URL supplied by a website is accepted here.
pub(crate) fn run(owner: HWND, app: &AppRef, fields: &PanelFields) -> bool {
    if !fields.a.starts_with("store") { return false; }
    if fields.id.is_some() || fields.parent.is_some() || fields.before.is_some() || fields.n.is_some() || fields.on.is_some() || fields.url.is_some() { return true; }
    match fields.a.as_str() {
        "storeopen" if fields.text.is_none() => open(owner, app, false),
        "storebrowse" if fields.text.is_none() => open_web(app, store::BROWSE_URL),
        "storeinstall" => if let Some(id) = fields.text.as_deref() { start(owner, app, id); },
        "storeurl" => match fields.text.as_deref().and_then(|text| crate::crx_fetch::parse_store_url(text).ok()) {
            Some(id) => start(owner, app, &id), None => error(owner, "Enter a valid HTTPS Chrome Web Store extension listing URL."),
        },
        "storelisting" => if let Some(id) = fields.text.as_deref().and_then(|id| crate::crx_fetch::parse_extension_id(id).ok()) { open_web(app, &format!("https://chromewebstore.google.com/detail/{id}")); },
        "storesource" => if let Some(entry) = fields.text.as_deref().and_then(store::entry) { open_web(app, entry.homepage); },
        "storecancel" => {
            if let Some(request) = fields.text.as_deref().filter(|id| store::valid_request_id(id)) {
                with(owner, |state| if state.request == request && matches!(state.phase, "downloading" | "review") {
                    if let Some(cancelled) = &state.cancelled { cancelled.store(true, Ordering::Release); }
                    state.review = None; state.phase = "cancelled"; state.error.clear();
                }); changed(owner);
            }
        }
        "storeapprove" => {
            let Some(request) = fields.text.as_deref().filter(|id| store::valid_request_id(id)) else { return true; };
            let profile = crate::profile::active().ok();
            let review = with(owner, |state| {
                if state.phase != "review" || state.request != request || profile.as_ref().is_none_or(|profile| profile.root() != state.profile) { return None; }
                state.phase = "installing"; state.review.take()
            });
            if let (Some(review), Some(profile)) = (review, profile) {
                let result = review.prepared.commit_named(&review.name).map(Some).map_err(|error| error.to_string());
                match super::window::finish_install(app, &profile, result) {
                    Ok(true) => { with(owner, |state| { state.phase = "ready"; state.error.clear(); }); changed(owner); }
                    Ok(false) => error(owner, "No extension was installed."),
                    Err(reason) => error(owner, reason),
                }
            }
        }
        _ => {}
    }
    true
}
fn open_web(app: &AppRef, text: &str) {
    if let Ok(url) = url::Url::parse(text) { App::new_tab(app); App::navigate_current(app, &url); }
}

pub(crate) fn listing_button_json() -> String {
    Json::object(|out| { out.int("v", 1); out.string("t", "extbtn"); out.bool("on", true); out.uint("n", 0); out.string("name", "Add to Bowser"); out.bool("store", true); })
}
