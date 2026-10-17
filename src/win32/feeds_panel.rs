//! The Feeds panel's host side (thread T13): what the panel's commands do, and the worker threads that
//! fetch. The panel itself is an HTML page in the dialog host (`html_dialog::Kind::Feeds`,
//! `ui/src/panels/feeds.ts`); its wire is `chrome_host::panels::feeds`; the core is `crate::feeds`.
//!
//! Everything runs on the UI thread except the fetches. A worker thread only calls
//! `crate::feeds::{look_at, refresh_one}` and leaves its answer in [`DONE`], then posts the panel
//! window a message; the UI thread takes the answers, changes the store and saves it. The store is
//! loaded from `feeds.txt` once and kept here, so a refresh that finishes while the panel is closed is
//! applied (and saved) the next time the panel opens.
//!
//! Network use is exactly: a preview or subscribe of an address the user typed or picked, a refresh the
//! user pressed or that Feeds' "refresh when opened" setting asks for, and an OPML import's new
//! subscriptions. Nothing runs at startup or on a timer.

use std::{cell::RefCell, sync::Mutex};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};

use crate::{
    app::{App, AppRef},
    chrome_host::panels::feeds::{self as wire, Action, Preview, View},
    feeds::{self, Outcome, discover::FeedLink, store::{AddError, Store}},
};

/// The panel window's message for "a worker left an answer".
pub(crate) const WM_FEEDS_DONE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x5C6;
/// Most feeds one refresh batch fetches.
const MAX_BATCH: usize = 100;

enum Done {
    /// A preview or subscribe: what the address was and what it turned out to be.
    Looked { text: String, subscribe: bool, result: Result<Outcome, String> },
    /// One subscription fetched (or failed).
    Refreshed { id: u32, result: Result<feeds::parse::ParsedFeed, String> },
    /// The batch is over.
    BatchEnd,
}

static DONE: Mutex<Vec<(isize, Done)>> = Mutex::new(Vec::new());

#[derive(Default)]
struct State {
    store: Option<Store>,
    selected: u32,
    preview: Option<Preview>,
    page: Vec<FeedLink>,
    status: String,
    error: String,
    /// Worker jobs not yet answered.
    jobs: u32,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|state| f(&mut state.borrow_mut()))
}

fn profile() -> Option<crate::profile::Profile> {
    crate::profile::active().ok().filter(crate::productivity::profile_allows_productivity)
}

fn path() -> Option<std::path::PathBuf> {
    profile().map(|profile| profile.root().join("feeds.txt"))
}

/// Runs `f` on the store, loading it first when needed. `None` for a guest profile.
fn store<R>(f: impl FnOnce(&mut Store) -> R) -> Option<R> {
    let path = path()?;
    Some(with(|state| f(state.store.get_or_insert_with(|| Store::load(&path)))))
}

fn save() {
    let Some(path) = path() else { return };
    let result = with(|state| state.store.as_ref().map(|store| store.save(&path)));
    if let Some(Err(error)) = result {
        with(|state| state.error = format!("Feeds could not be saved: {error}"));
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

// ---- opening ----

/// Opens the Feeds panel. `page` is the feeds the page in front offers (the address bar's feed button);
/// when there are any the panel shows them first. Without the HTML chrome there is no panel: Bowser says so.
pub(crate) fn open(hwnd: HWND, app: &AppRef, page: Vec<FeedLink>) {
    with(|state| {
        state.page = page;
        state.error.clear();
        state.status.clear();
    });
    if super::html_dialog::html_only(hwnd)
        && let Some(env) = super::window::html_environment(hwnd)
    {
        super::html_dialog::open_feeds(hwnd, app, env);
    } else {
        super::window::show_toast(hwnd, "Feeds need Bowser's own interface, which is not running", "Feeds");
    }
}

/// The panel just became ready: fetch the subscriptions that are older than the user's "refresh when
/// opened" choice. The setting's value `0` fetches nothing.
pub(crate) fn on_ready(app: &AppRef, parent: HWND) {
    let minutes = App::settings(app).feeds_refresh_minutes;
    let ids = store(|store| store.stale(now(), minutes)).unwrap_or_default();
    if !ids.is_empty() {
        start_refresh(parent, ids);
    }
}

// ---- the snapshot ----

pub(crate) fn snapshot_json(app: &AppRef) -> String {
    let minutes = App::settings(app).feeds_refresh_minutes;
    let allowed = profile().is_some();
    let empty = Store::default();
    let json = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if allowed && state.store.is_none() {
            if let Some(path) = path() {
                state.store = Some(Store::load(&path));
            }
        }
        let State { store, selected, preview, page, status, error, jobs } = &*state;
        wire::snapshot_json(&View {
            allowed,
            busy: *jobs > 0,
            refresh_minutes: minutes,
            selected: *selected,
            store: store.as_ref().unwrap_or(&empty),
            preview: preview.as_ref(),
            page,
            status,
            error,
        })
    });
    json
}

// ---- commands ----

/// Runs one validated panel command. The caller sends a fresh snapshot afterwards.
pub(crate) fn handle(app: &AppRef, parent: HWND, action: Action) {
    if profile().is_none() {
        return;
    }
    with(|state| state.error.clear());
    match action {
        Action::Select(id) => with(|state| {
            state.selected = id;
            state.status.clear();
        }),
        Action::ClosePreview => with(|state| {
            state.preview = None;
            state.status.clear();
        }),
        Action::Preview(text) => start_look(parent, text, false),
        Action::Subscribe(text) => subscribe(parent, text),
        Action::Unsubscribe(id) => {
            let removed = store(|store| store.remove(id)).unwrap_or(false);
            if removed {
                with(|state| {
                    if state.selected == id {
                        state.selected = 0;
                    }
                    state.status = "Unsubscribed.".into();
                });
                save();
            }
        }
        Action::Refresh(id) => {
            let ids = match id {
                Some(id) => vec![id],
                None => store(|store| store.feeds.iter().map(|feed| feed.id).collect()).unwrap_or_default(),
            };
            start_refresh(parent, ids);
        }
        Action::Read { feed, item, read } => {
            if store(|store| store.mark(feed, item, read)).unwrap_or(false) {
                save();
            }
        }
        Action::ReadAll(id) => {
            if store(|store| store.mark_all(id)).unwrap_or(0) > 0 {
                save();
            }
        }
        Action::Open { feed, item } => open_item(app, feed, item, false),
        Action::Play { feed, item } => open_item(app, feed, item, true),
        Action::Import => import(parent),
    }
}

/// Opens an item (or plays its episode) in a new tab, and marks it read. The address comes from the stored
/// item, never from the page.
fn open_item(app: &AppRef, feed: u32, item: u32, play: bool) {
    let target = store(|store| {
        let feed = store.feed(feed)?;
        let item = feed.items.iter().find(|stored| stored.hash == item)?;
        let target = if play { feeds::play_target(item) } else { feeds::open_target(&feed.url, item) };
        Some(target)
    })
    .flatten()
    .flatten();
    let Some(target) = target else {
        with(|state| state.error = if play { "This item has no episode to play.".into() } else { "This item has no link.".into() });
        return;
    };
    let Some(url) = url::Url::parse(&target).ok().filter(|url| matches!(url.scheme(), "http" | "https")) else { return };
    if store(|store| store.mark(feed, item, true)).unwrap_or(false) {
        save();
    }
    App::open_in_new_tab(app, &url);
}

fn subscribe(parent: HWND, text: String) {
    // The previewed feed is already read: subscribe to what the user saw, without fetching again.
    let previewed = with(|state| {
        state.preview.as_ref().filter(|preview| preview.candidates.is_empty() && same_address(&preview.url, &text)).cloned()
    });
    if let Some(preview) = previewed {
        let parsed = feeds::parse::ParsedFeed { title: preview.title.clone(), site: preview.site.clone(), items: preview.items.clone() };
        add_feed(&preview.url, &parsed);
        return;
    }
    start_look(parent, text, true);
}

fn same_address(a: &str, b: &str) -> bool {
    let norm = |text: &str| feeds::fetch::parse_url(text).map(|url| url.to_string()).unwrap_or_default();
    let (a, b) = (norm(a), norm(b));
    !a.is_empty() && a == b
}

fn add_feed(url: &str, parsed: &feeds::parse::ParsedFeed) {
    let title = parsed.title.clone();
    let added = store(|store| match store.add(url, &title) {
        Ok(id) => {
            store.merge(id, parsed, now());
            Ok(id)
        }
        Err(error) => Err(error),
    });
    match added {
        Some(Ok(id)) => {
            with(|state| {
                state.preview = None;
                state.selected = id;
                state.status = "Subscribed.".into();
            });
            save();
        }
        Some(Err(AddError::Duplicate(id))) => with(|state| {
            state.selected = id;
            state.preview = None;
            state.status = "You are already subscribed to that feed.".into();
        }),
        Some(Err(AddError::TooMany)) => with(|state| state.error = format!("Bowser keeps at most {} feeds.", feeds::store::MAX_FEEDS)),
        None => {}
    }
}

// ---- workers ----

fn post_done(parent: isize, done: Done) {
    DONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((parent, done));
    super::post(HWND(parent as *mut _), WM_FEEDS_DONE, WPARAM(0), LPARAM(0));
}

fn start_look(parent: HWND, text: String, subscribe: bool) {
    // Refuse a bad address here, so the panel says why at once and no thread starts.
    if let Err(error) = feeds::fetch::parse_url(&text) {
        with(|state| state.error = error.to_string());
        return;
    }
    with(|state| {
        state.jobs += 1;
        state.status = if subscribe { "Looking for the feed…".into() } else { "Loading a preview…".into() };
    });
    let raw = parent.0 as isize;
    let worker_text = text.clone();
    let spawned = std::thread::Builder::new().name("bowser-feed-look".into()).spawn(move || {
        let result = feeds::look_at(&worker_text);
        post_done(raw, Done::Looked { text: worker_text, subscribe, result });
    });
    if spawned.is_err() {
        with(|state| {
            state.jobs = state.jobs.saturating_sub(1);
            state.error = "The feed could not be fetched.".into();
        });
    }
}

fn start_refresh(parent: HWND, ids: Vec<u32>) {
    let jobs: Vec<(u32, String)> = store(|store| ids.iter().filter_map(|id| store.feed(*id).map(|feed| (feed.id, feed.url.clone()))).take(MAX_BATCH).collect()).unwrap_or_default();
    if jobs.is_empty() {
        return;
    }
    if with(|state| state.jobs > 0) {
        with(|state| state.status = "Wait for the current fetch to finish.".into());
        return;
    }
    with(|state| {
        state.jobs += 1;
        state.status = if jobs.len() == 1 { "Refreshing…".into() } else { format!("Refreshing {} feeds…", jobs.len()) };
    });
    let raw = parent.0 as isize;
    let spawned = std::thread::Builder::new().name("bowser-feed-refresh".into()).spawn(move || {
        for (id, url) in jobs {
            let result = feeds::refresh_one(&url);
            post_done(raw, Done::Refreshed { id, result });
        }
        post_done(raw, Done::BatchEnd);
    });
    if spawned.is_err() {
        with(|state| {
            state.jobs = state.jobs.saturating_sub(1);
            state.error = "The feeds could not be fetched.".into();
        });
    }
}

/// Takes the answers the workers left for `parent` and applies them. True when anything changed (the
/// caller sends a fresh snapshot).
pub(crate) fn apply_done(parent: HWND) -> bool {
    let mine: Vec<Done> = {
        let mut queue = DONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mine, rest): (Vec<_>, Vec<_>) = queue.drain(..).partition(|(owner, _)| *owner == parent.0 as isize);
        *queue = rest;
        mine.into_iter().map(|(_, done)| done).collect()
    };
    let any = !mine.is_empty();
    let mut changed_store = false;
    for done in mine {
        match done {
            Done::Looked { text, subscribe, result } => {
                with(|state| state.jobs = state.jobs.saturating_sub(1));
                match result {
                    Ok(Outcome::Feed { url, parsed }) if subscribe => add_feed(&url, &parsed),
                    Ok(Outcome::Feed { url, parsed }) => with(|state| {
                        state.status.clear();
                        state.preview = Some(Preview { url, title: parsed.title, site: parsed.site, items: parsed.items, candidates: Vec::new() });
                    }),
                    Ok(Outcome::Candidates(links)) => with(|state| {
                        state.status = if links.len() == 1 { "That page offers a feed. Open it to see what is in it.".into() } else { format!("That page offers {} feeds. Pick one.", links.len()) };
                        state.preview = Some(Preview { url: text, candidates: links, ..Preview::default() });
                    }),
                    Err(error) => with(|state| {
                        state.status.clear();
                        state.error = error;
                    }),
                }
                changed_store = true;
            }
            Done::Refreshed { id, result } => {
                match result {
                    Ok(parsed) => {
                        let _ = store(|store| store.merge(id, &parsed, now()));
                    }
                    Err(error) => {
                        let _ = store(|store| store.set_error(id, &error));
                    }
                }
                changed_store = true;
            }
            Done::BatchEnd => {
                let failed = store(|store| store.feeds.iter().filter(|feed| !feed.error.is_empty()).count()).unwrap_or(0);
                with(|state| {
                    state.jobs = state.jobs.saturating_sub(1);
                    state.status = match failed {
                        0 => "Up to date.".into(),
                        1 => "Updated. One feed could not be fetched.".into(),
                        n => format!("Updated. {n} feeds could not be fetched."),
                    };
                });
            }
        }
    }
    if changed_store {
        save();
    }
    any
}

// ---- OPML ----

fn import(parent: HWND) {
    let Some(path) = pick_opml(parent) else { return };
    let bytes = match std::fs::metadata(&path) {
        Ok(meta) if meta.len() <= feeds::xml::MAX_DOCUMENT_BYTES as u64 => std::fs::read(&path),
        Ok(_) => {
            with(|state| state.error = "That file is larger than Bowser reads (3 MB).".into());
            return;
        }
        Err(error) => Err(error),
    };
    let listed = match bytes.map_err(|_| "That file could not be read.").and_then(|bytes| feeds::parse::parse_opml(&bytes).map_err(|_| "That is not an OPML file.")) {
        Ok(listed) => listed,
        Err(message) => {
            with(|state| state.error = message.into());
            return;
        }
    };
    let mut added = Vec::new();
    let (mut skipped, mut full) = (0usize, false);
    for feed in &listed {
        // Every address is checked as a typed one is, so nothing the file names can be a local or odd address.
        let Ok(url) = feeds::fetch::parse_url(&feed.url) else {
            skipped += 1;
            continue;
        };
        match store(|store| store.add(url.as_str(), &feed.title)) {
            Some(Ok(id)) => added.push(id),
            Some(Err(AddError::Duplicate(_))) => skipped += 1,
            Some(Err(AddError::TooMany)) => {
                full = true;
                break;
            }
            None => return,
        }
    }
    if !added.is_empty() {
        save();
    }
    with(|state| {
        state.status = format!(
            "Imported {} feed{}{}.",
            added.len(),
            if added.len() == 1 { "" } else { "s" },
            if skipped > 0 { format!(", {skipped} skipped (already subscribed or not a web address)") } else { String::new() }
        );
        if full {
            state.error = format!("Bowser keeps at most {} feeds; the rest were left out.", feeds::store::MAX_FEEDS);
        }
    });
    // The new subscriptions are empty until fetched: fetch them now (the user asked for them).
    if !added.is_empty() {
        start_refresh(parent, added);
    }
}

/// The native file picker for an OPML file. `None` when cancelled or when the dialog fails.
fn pick_opml(owner: HWND) -> Option<std::path::PathBuf> {
    use windows::{
        Win32::{
            System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree},
            UI::Shell::{
                Common::COMDLG_FILTERSPEC, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
            },
        },
        core::w,
    };
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        dialog.SetOptions(FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST).ok()?;
        dialog.SetTitle(w!("Choose an OPML file")).ok()?;
        let filters = [
            COMDLG_FILTERSPEC { pszName: w!("OPML files (*.opml;*.xml)"), pszSpec: w!("*.opml;*.xml") },
            COMDLG_FILTERSPEC { pszName: w!("All files (*.*)"), pszSpec: w!("*.*") },
        ];
        dialog.SetFileTypes(&filters).ok()?;
        dialog.Show(Some(owner)).ok()?;
        let item = dialog.GetResult().ok()?;
        let display = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = display.to_string().ok().map(std::path::PathBuf::from);
        CoTaskMemFree(Some(display.0.cast_const().cast()));
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_spellings_of_one_address_are_the_same() {
        assert!(same_address("https://a.example/feed", "a.example/feed"));
        assert!(same_address("https://a.example/feed", " https://a.example/feed "));
        assert!(!same_address("https://a.example/feed", "https://a.example/other"));
        assert!(!same_address("", ""));
        assert!(!same_address("http://127.0.0.1/x", "http://127.0.0.1/x"), "an address that is refused matches nothing");
    }
}
