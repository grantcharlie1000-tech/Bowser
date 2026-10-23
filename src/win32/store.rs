//! UI-thread Store state and a bounded background preparation worker. Website WebViews have no
//! handler here. Only the trusted Bowser settings host can approve a matching one-shot review.
use std::{cell::RefCell, collections::HashMap, path::PathBuf, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use crate::{app::{App, AppRef}, chrome_host::{json::Json, protocol::PanelFields}, internal_page::InternalPage, store};

#[derive(Default)]
struct State {
    request: String, id: String, name: String, version: String, phase: &'static str,
    error: String, restart: bool, profile: PathBuf,
    /// The verified package's icon data URL; sent only in the phases after verification (see `snapshot`).
    icon: String,
    cancelled: Option<Arc<AtomicBool>>, review: Option<store::Review>,
}
const MAX_SNAPSHOT_BYTES: usize = 240 * 1024;
thread_local! { static STATES: RefCell<HashMap<isize, State>> = RefCell::new(HashMap::new()); }
struct Completed { owner: isize, request: String, outcome: Result<store::Review, String> }
static COMPLETED: Mutex<Vec<Completed>> = Mutex::new(Vec::new());

fn with<T>(owner: HWND, f: impl FnOnce(&mut State) -> T) -> T {
    STATES.with(|states| f(states.borrow_mut().entry(owner.0 as isize).or_default()))
}
fn changed(owner: HWND) { super::settings_page::store_changed(owner); }
fn set_error(state: &mut State, text: String) {
    state.phase = "error"; state.error = text.chars().take(1500).map(|c| if c.is_control() { ' ' } else { c }).collect(); state.review = None; state.icon.clear();
}
fn error(owner: HWND, text: impl Into<String>) {
    with(owner, |state| set_error(state, text.into()));
    changed(owner);
}
/// A cancelled request keeps nothing of the package, including its icon.
fn set_cancelled(state: &mut State) {
    if let Some(cancelled) = &state.cancelled { cancelled.store(true, Ordering::Release); }
    state.review = None; state.icon.clear(); state.phase = "cancelled"; state.error.clear();
}

/// Discovery is Google's real website in an ordinary browsing tab. It receives no Bowser bridge.
pub(crate) fn open(_owner: HWND, app: &AppRef, reuse_blank: bool) {
    if !reuse_blank { App::new_tab(app); }
    if let Ok(url) = url::Url::parse(store::BROWSE_URL) { App::navigate_current(app, &url); }
}

/// Only a native install request opens the trusted package/permission review, not discovery.
fn open_review(owner: HWND, app: &AppRef) {
    if !super::settings_page::usable(owner) { super::show_error_message("The Bowser Store needs Bowser's HTML interface. Restart with the default HTML interface."); return; }
    let _ = App::open_internal_page(app, InternalPage::Store, false);
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
    let Some(permit) = store::acquire_download_permit() else {
        return error(owner, "Other extension downloads are finishing. Wait a moment and try again. Nothing was installed.");
    };
    let request = store::request_id();
    let cancelled = Arc::new(AtomicBool::new(false));
    let restart = crate::extensions::read_records(&profile.extensions_file()).is_ok_and(|records| records.is_empty());
    with(owner, |state| *state = State { request: request.clone(), id: id.to_owned(), name: store::entry(id).map_or_else(String::new, |entry| entry.name.to_owned()), phase: "downloading", restart, profile: profile.root().to_owned(), cancelled: Some(cancelled.clone()), ..State::default() });
    open_review(owner, app);
    let raw = owner.0 as isize;
    let worker_id = id.to_owned();
    let spawned = std::thread::Builder::new().name("bowser-store-download".into()).spawn(move || {
        let _permit = permit;
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
                with(owner, |state| { let mut review = review; state.name = review.name.clone(); state.version = review.prepared.inspection().manifest.version.clone(); state.phase = "review"; state.icon = std::mem::take(&mut review.icon); state.review = Some(review); });
                // The icon is decoration: if it alone pushes the snapshot over budget, drop it, never the review.
                if snapshot(owner, true).len() > MAX_SNAPSHOT_BYTES { with(owner, |state| state.icon.clear()); }
                // Never shorten a permission review to fit the bridge. Over-budget fails closed.
                if snapshot(owner, true).len() > MAX_SNAPSHOT_BYTES { error(owner, "The full permission review is too large to show safely. Nothing was installed."); }
                changed(owner);
            }
            Err(reason) => error(owner, reason),
        }
    }
}

/// The icon belongs to the verified package, so it shows only once one exists (review, installing,
/// ready). In every other phase, including a new request, error and cancel, it is empty.
fn visible_icon(state: &State) -> &str {
    if matches!(state.phase, "review" | "installing" | "ready") { &state.icon } else { "" }
}

pub(crate) fn snapshot(owner: HWND, shown: bool) -> String {
    with(owner, |state| crate::chrome_host::protocol::snap_json("store", |out| {
        out.bool("shown", shown);
        out.array("catalogue", |items| for entry in store::CATALOGUE { items.object(|item| {
            item.string("id", entry.id); item.string("name", entry.name); item.string("description", entry.description);
            item.string("publisher", entry.publisher); item.string("category", entry.category);
            item.string("source", entry.source); item.string("homepage", entry.homepage);
            item.string("provenance", "Manually curated from the linked publisher and Chrome Web Store listing. Not an all-store search or availability guarantee.");
            let (compatibility, note) = store::catalogue_evidence(entry.id);
            item.string("checked", entry.checked); item.string("compatibility", compatibility);
            item.string("compatibilityNote", note);
        }); });
        out.string("phase", if state.phase.is_empty() { "idle" } else { state.phase });
        out.string("requestId", &state.request); out.string("extensionId", &state.id); out.string("name", &state.name); out.string("version", &state.version);
        out.string("error", &state.error); out.bool("restart", state.restart);
        out.string("icon", visible_icon(state));
        let manifest = state.review.as_ref().map(|review| &review.prepared.inspection().manifest);
        for (key, values) in [
            ("permissions", manifest.map(|m| &m.permissions)), ("hostPermissions", manifest.map(|m| &m.host_permissions)),
            ("contentScripts", manifest.map(|m| &m.content_script_matches)),
            ("contentScriptDeclarations", manifest.map(|m| &m.content_script_declarations)), ("optionalPermissions", manifest.map(|m| &m.optional_permissions)),
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
                with(owner, |state| if state.request == request && matches!(state.phase, "downloading" | "review") { set_cancelled(state); }); changed(owner);
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
                let result = review.commit().map(Some);
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

// Store-installer downloads the engine cancelled, as (window, extension id), until the window
// procedure answers them. Bounded: a page that fires them in a loop cannot grow it.
thread_local! { static DOWNLOAD_REQUESTS: RefCell<Vec<(isize, String)>> = const { RefCell::new(Vec::new()) }; }
const MAX_DOWNLOAD_REQUESTS: usize = 4;

/// The Web Store's own "Add to Chrome" download was cancelled (`engine::download`); this is called
/// from inside WebView2's `DownloadStarting`, so it only queues and posts. The answer comes from
/// [`apply_download_requests`] in the window procedure.
pub(crate) fn request_from_download(owner: HWND, id: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{GA_ROOT, GetAncestor};
    if owner.0.is_null() || crate::crx_fetch::parse_extension_id(id).is_err() { return; }
    let root = unsafe { GetAncestor(owner, GA_ROOT) };
    let root = if root.0.is_null() { owner } else { root };
    DOWNLOAD_REQUESTS.with(|queue| {
        let mut queue = queue.borrow_mut();
        let entry = (root.0 as isize, id.to_owned());
        if queue.contains(&entry) { return; }
        if queue.len() >= MAX_DOWNLOAD_REQUESTS { queue.remove(0); }
        queue.push(entry);
    });
    super::post(root, super::WM_SETTINGS_COMMAND, WPARAM(0), LPARAM(0));
}

/// Whether a cancelled Store-installer download may start Bowser's install flow: only for the id of
/// the Store listing the user is on right now, exactly what the "Add to Bowser" button would install.
/// A website cannot name any other extension through this path.
fn matches_active_listing(listing: Option<&str>, requested: &str) -> bool {
    listing.is_some_and(|listing| listing == requested)
}

/// Answers the queued requests for `owner` (from `WM_SETTINGS_COMMAND`). A request for the extension
/// on the active listing goes through [`start`] (same checks, same permission review, no automatic
/// install); any other gets a passive toast pointing at the real button.
pub(crate) fn apply_download_requests(owner: HWND, app: &AppRef) {
    let mine: Vec<String> = DOWNLOAD_REQUESTS.with(|queue| {
        let mut queue = queue.borrow_mut();
        let (mine, rest): (Vec<_>, Vec<_>) = queue.drain(..).partition(|(window, _)| *window == owner.0 as isize);
        *queue = rest;
        mine.into_iter().map(|(_, id)| id).collect()
    });
    for id in mine {
        let listing = super::window::current_store_listing(app);
        if matches_active_listing(listing.as_deref(), &id) {
            start(owner, app, &id);
        } else {
            super::window::show_toast(owner, "Chrome's installer can't run in Bowser. Open the extension's Chrome Web Store page and use Add to Bowser.", "");
        }
    }
}

pub(crate) fn listing_button_json() -> String {
    Json::object(|out| { out.int("v", 1); out.string("t", "extbtn"); out.bool("on", true); out.uint("n", 0); out.string("name", "Add to Bowser"); out.bool("store", true); })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "aomkpefnllinimbhddlfhelelngakbbn";

    const ICON: &str = "data:image/png;base64,iVBORw0KGgo=";

    #[test]
    fn the_icon_shows_only_in_the_phases_after_the_package_is_verified() {
        let owner = HWND(0x1c0 as *mut _);
        for (phase, shown) in [("", false), ("idle", false), ("downloading", false), ("review", true), ("installing", true), ("ready", true), ("error", false), ("cancelled", false)] {
            with(owner, |state| *state = State { phase, icon: ICON.into(), ..State::default() });
            let json = snapshot(owner, true);
            assert_eq!(json.contains(&format!(r#""icon":"{ICON}""#)), shown, "{phase:?}");
            if !shown { assert!(json.contains(r#""icon":"""#), "{phase:?}: {json}"); }
        }
        close(owner);
    }

    #[test]
    fn a_previous_icon_cannot_survive_a_new_request_an_error_or_a_cancel() {
        let owner = HWND(0x1c1 as *mut _);
        let reviewing = || State { request: "store-1-1".into(), phase: "review", icon: ICON.into(), cancelled: Some(Arc::new(AtomicBool::new(false))), ..State::default() };
        with(owner, |state| { *state = reviewing(); set_error(state, "No.".into()); });
        assert!(with(owner, |state| state.icon.is_empty()) && snapshot(owner, true).contains(r#""icon":"""#));
        with(owner, |state| { *state = reviewing(); set_cancelled(state); });
        assert!(with(owner, |state| state.icon.is_empty() && state.phase == "cancelled") && snapshot(owner, true).contains(r#""icon":"""#));
        // `start` replaces the whole state, so a new request begins with no icon at all.
        assert!(State::default().icon.is_empty());
        let production = &include_str!("store.rs")[..include_str!("store.rs").find("#[cfg(test)]").unwrap()];
        let start = production.split("pub(crate) fn start(").nth(1).unwrap().split("pub(crate) fn apply_completed").next().unwrap();
        assert!(start.contains("*state = State {") && start.contains("..State::default() }"));
        close(owner);
    }

    #[test]
    fn the_icon_is_decoration_it_is_dropped_before_the_review_is_refused() {
        let source = include_str!("store.rs");
        let apply = source.split("pub(crate) fn apply_completed(").nth(1).unwrap().split("fn visible_icon").next().unwrap();
        let drop_icon = apply.find("state.icon.clear()").unwrap();
        let refuse = apply.find("The full permission review is too large").unwrap();
        assert!(drop_icon < refuse, "the icon must be shed before the review fails closed");
    }

    #[test]
    fn a_cancelled_installer_download_only_routes_for_the_listing_the_user_is_on() {
        assert!(matches_active_listing(Some(ID), ID));
        assert!(!matches_active_listing(None, ID), "no Store listing is open");
        assert!(!matches_active_listing(Some("ddkjiahejlhfcafbddmgiahcphecmpfh"), ID), "a page cannot name another extension");
        assert!(!matches_active_listing(Some(ID), &ID.to_uppercase()));
    }

    #[test]
    fn the_download_request_queue_ignores_bad_ids_and_a_null_window() {
        use windows::Win32::Foundation::HWND;
        request_from_download(HWND::default(), ID);
        request_from_download(HWND(1 as *mut _), "not-an-id");
        assert!(DOWNLOAD_REQUESTS.with(|queue| queue.borrow().is_empty()));
    }

    #[test]
    fn a_cancelled_download_never_installs_without_the_existing_consent_path() {
        let source = include_str!("store.rs");
        let production = &source[..source.find("#[cfg(test)]").unwrap()];
        let apply = production.split("pub(crate) fn apply_download_requests(").nth(1).unwrap().split("pub(crate) fn listing_button_json").next().unwrap();
        // The only things it may call are the listing check, `start` (which stops at the review) and a toast.
        assert!(apply.contains("current_store_listing(app)") && apply.contains("start(owner, app, &id)") && apply.contains("show_toast("));
        for forbidden in ["commit(", "storeapprove", "prepare(", "download_crx"] {
            assert!(!apply.contains(forbidden), "{forbidden}");
        }
    }
}
