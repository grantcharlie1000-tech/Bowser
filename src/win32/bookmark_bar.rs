//! The bookmarks bar: a strip under the toolbar (or at the bottom of the window) that lists the
//! bookmarks of one folder and, optionally, tab groups. One HTML host (`View::BookmarkBar`) in a
//! child window of the browser window, created the first time the bar is wanted and kept hidden
//! while it is not.
//!
//! The bar takes its height from the page: [`reserve_px`] is what `window::relayout` subtracts from
//! the viewport, so the page, the split panes and the side panels all end where the bar begins.
//!
//! Every function runs on the UI thread. Page commands are queued from the WebView2 callback and run
//! from the window procedure (`WM_BOOKMARK_BAR_COMMAND`).

use std::{cell::RefCell, collections::{HashMap, HashSet}, rc::Rc, time::SystemTime};

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, IsWindow, RegisterClassW, SetWindowPos, ShowWindow, HWND_TOP,
            SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
        },
    },
    core::{w, PCWSTR},
};

use crate::{
    address_input::Options,
    address_settings::BarPosition,
    app::{App, AppRef},
    bookmarks::{Bookmark, BookmarkKind, BookmarkStore},
    chrome_host::{
        self,
        address_page::{self, BarAction, BarGroup, BarItem},
        host::{ChromeHost, View},
        protocol::{Command, HoverRect, PanelType},
    },
    settings::BookmarksBar,
    tab_groups::{self, DomainKey, TabGroupKey},
    tabs::TabId,
};

/// A bar command is waiting (see [`run_queued`]).
pub(crate) const WM_BOOKMARK_BAR_COMMAND: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x5E2;

const CLASS: PCWSTR = w!("BowserBookmarkBar");
/// Logical height of the bar.
pub(crate) const BAR_HEIGHT_DIP: i32 = 28;
/// Most bookmarks a folder menu lists, and how deep its sub-folders nest.
const MAX_MENU_ITEMS: usize = 60;
const MAX_MENU_DEPTH: u32 = 3;

#[derive(Default)]
struct Runtime {
    container: Option<HWND>,
    host: Option<ChromeHost>,
    requested: bool,
    ready: bool,
    failed: bool,
    shown: Option<RECT>,
    /// Last theme state and snapshot the page accepted.
    state: Option<String>,
    snapshot: Option<String>,
    /// What the last snapshot was built from, so it is rebuilt only when one of them changed.
    built: Option<BuiltFrom>,
    /// Domains of tab groups seen live, for "pin new tab groups".
    seen_groups: Option<HashSet<String>>,
}

#[derive(Clone, PartialEq, Eq)]
struct BuiltFrom {
    stamp: Option<(SystemTime, u64)>,
    folder: u64,
    display: String,
    groups: Vec<(String, String, bool, bool, i32)>,
}

thread_local! {
    static RUNTIMES: RefCell<HashMap<isize, Runtime>> = RefCell::new(HashMap::new());
    static QUEUE: RefCell<Vec<(isize, address_page::BarAction)>> = const { RefCell::new(Vec::new()) };
}

fn with<T>(owner: HWND, f: impl FnOnce(&mut Runtime) -> T) -> Option<T> {
    RUNTIMES.with(|runtimes| {
        let mut runtimes = runtimes.try_borrow_mut().ok()?;
        Some(f(runtimes.entry(owner.0 as isize).or_default()))
    })
}

/// Whether the bar is shown for the active tab: always, or only on a new tab page.
fn wanted(app: &AppRef) -> bool {
    let settings = App::settings(app);
    match settings.bookmarks_bar {
        BookmarksBar::Never => false,
        BookmarksBar::Always => true,
        BookmarksBar::NewTab => App::tab_is_blank(app, App::active_tab_id(app)),
    }
}

/// The page area the bar takes at DPI `dpi`: its height in physical pixels, and whether it sits at
/// the bottom. `(0, false)` while it is not shown.
pub(crate) fn reserve_px(app: &AppRef, dpi: u32) -> (i32, bool) {
    if !chrome_host::html_ui_enabled() || !wanted(app) {
        return (0, false);
    }
    let settings = App::settings(app);
    (BAR_HEIGHT_DIP * dpi.max(96) as i32 / 96, settings.address.bar_position == BarPosition::Bottom)
}

/// `viewport` after the bar took its share (called by `window::relayout`).
pub(crate) fn reserved_viewport(viewport: crate::chrome::layout::Rect, reserve: (i32, bool)) -> crate::chrome::layout::Rect {
    let (height, bottom) = reserve;
    if height <= 0 {
        return viewport;
    }
    let height = height.min((viewport.height() - 1).max(0));
    if bottom {
        crate::chrome::layout::Rect::new(viewport.left, viewport.top, viewport.right, viewport.bottom - height)
    } else {
        crate::chrome::layout::Rect::new(viewport.left, viewport.top + height, viewport.right, viewport.bottom)
    }
}

// ----- the host -----

fn register_class() -> bool {
    thread_local! { static REGISTERED: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) }; }
    if let Some(done) = REGISTERED.with(|slot| slot.get()) {
        return done;
    }
    let instance = unsafe { GetModuleHandleW(None) }.map(|module| module.into()).unwrap_or_default();
    let class = WNDCLASSW { lpfnWndProc: Some(container_proc), hInstance: instance, lpszClassName: CLASS, ..Default::default() };
    let ok = unsafe { RegisterClassW(&class) } != 0;
    REGISTERED.with(|slot| slot.set(Some(ok)));
    ok
}

unsafe extern "system" fn container_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

fn fail(owner: HWND) {
    let _ = with(owner, |rt| rt.failed = true);
}

fn ensure_host(owner: HWND) {
    let start = with(owner, |rt| {
        if rt.requested || rt.failed {
            return false;
        }
        rt.requested = true;
        true
    })
    .unwrap_or(false);
    if !start {
        return;
    }
    let Some(env) = super::window::html_environment(owner) else {
        let _ = with(owner, |rt| rt.requested = false);
        return;
    };
    if !register_class() {
        return fail(owner);
    }
    let instance = unsafe { GetModuleHandleW(None) }.map(|module| module.into()).ok();
    let container = unsafe { CreateWindowExW(Default::default(), CLASS, w!(""), WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS, 0, 0, 1, 1, Some(owner), None, instance, None) };
    let Ok(container) = container else { return fail(owner) };
    unsafe {
        let _ = SetWindowPos(container, Some(HWND_TOP), 0, 0, 1, 1, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
    let _ = with(owner, |rt| rt.container = Some(container));
    let raw = owner.0 as isize;
    let on_command: Rc<dyn Fn(Command)> = Rc::new(move |command| {
        let owner = HWND(raw as *mut _);
        match command {
            Command::Ready => {
                let _ = with(owner, |rt| {
                    rt.ready = true;
                    rt.state = None;
                    rt.snapshot = None;
                    rt.built = None;
                });
                super::window::split_ui_invalidate(owner);
            }
            Command::Panel { kind: PanelType::Address, f } => {
                if let Some(action) = address_page::bar_command(&f) {
                    queue(owner, action);
                }
            }
            _ => {}
        }
    });
    let on_failed: Rc<dyn Fn()> = Rc::new(move || fail(HWND(raw as *mut _)));
    let created = ChromeHost::create(&env, container, RECT { left: 0, top: 0, right: 1, bottom: 1 }, View::BookmarkBar, on_command, on_failed, None, move |result| {
        let owner = HWND(raw as *mut _);
        match result {
            Ok(host) => {
                let keep = with(owner, |rt| {
                    if rt.failed {
                        return false;
                    }
                    rt.host = Some(host.clone());
                    true
                })
                .unwrap_or(false);
                if !keep {
                    host.close();
                }
                super::window::split_ui_invalidate(owner);
            }
            Err(_) => fail(owner),
        }
    });
    if created.is_err() {
        fail(owner);
    }
}

fn queue(owner: HWND, action: BarAction) {
    let full = QUEUE.with(|queue| {
        let mut queue = queue.borrow_mut();
        if queue.len() >= 16 {
            return true;
        }
        queue.push((owner.0 as isize, action));
        false
    });
    if !full {
        super::post(owner, WM_BOOKMARK_BAR_COMMAND, WPARAM(0), LPARAM(0));
    }
}

/// Closes the host (window closing).
pub(crate) fn close(owner: HWND) {
    let removed = RUNTIMES.with(|runtimes| runtimes.try_borrow_mut().ok().and_then(|mut all| all.remove(&(owner.0 as isize))));
    if let Some(runtime) = removed {
        if let Some(host) = runtime.host {
            host.close();
        }
        if let Some(container) = runtime.container {
            unsafe {
                if IsWindow(Some(container)).as_bool() {
                    let _ = DestroyWindow(container);
                }
            }
        }
    }
}

// ----- the content -----

fn live_groups(app: &AppRef) -> Vec<(String, Vec<TabId>)> {
    let inputs = App::tab_group_inputs(app);
    let prefs = App::tab_group_preferences(app);
    let keys = tab_groups::group_keys(&inputs, &prefs);
    let mut seen: Vec<TabGroupKey> = Vec::new();
    let mut out = Vec::new();
    for key in keys.iter().flatten() {
        if let TabGroupKey::Domain(domain) = key {
            if seen.contains(key) {
                continue;
            }
            seen.push(key.clone());
            let members: Vec<TabId> = tab_groups::group_members(&keys, key).into_iter().filter_map(|index| inputs.get(index).map(|input| input.id)).collect();
            out.push((domain.as_str().to_owned(), members));
        }
    }
    out
}

/// "Pin new tab groups": a group seen for the first time (after the bar has looked once) is pinned.
fn pin_new_groups(owner: HWND, app: &AppRef, live: &[(String, Vec<TabId>)]) {
    let settings = App::settings(app);
    let now: HashSet<String> = live.iter().map(|(domain, _)| domain.clone()).collect();
    let fresh: Vec<String> = with(owner, |rt| {
        let fresh = match &rt.seen_groups {
            // The first look only learns what is already open.
            None => Vec::new(),
            Some(before) => live.iter().map(|(domain, _)| domain.clone()).filter(|domain| !before.contains(domain)).collect(),
        };
        rt.seen_groups = Some(now);
        fresh
    })
    .unwrap_or_default();
    if let Some(pinned) = pins_after(settings.pin_new_tab_groups, fresh, &settings.address.bar_groups) {
        save_pinned(app, &pinned);
    }
}

/// --- wave-1 fixes --- "Pin new tab groups": the pinned list once the groups seen for the first time
/// join it (at most 32), or `None` when the option is off or nothing new appeared.
fn pins_after(enabled: bool, fresh: Vec<String>, pinned: &[String]) -> Option<Vec<String>> {
    if !enabled || fresh.is_empty() {
        return None;
    }
    let mut pinned = pinned.to_vec();
    for domain in fresh {
        if !pinned.contains(&domain) && pinned.len() < 32 {
            pinned.push(domain);
        }
    }
    Some(pinned)
}

fn save_pinned(app: &AppRef, pinned: &[String]) {
    let Ok(profile) = crate::profile::active() else { return };
    let path = profile.settings_file();
    let Ok(mut settings) = crate::settings::read_strict(&path) else { return };
    if settings.address.bar_groups == pinned || !settings.address.set("bar_groups", &pinned.join(",")) {
        return;
    }
    if crate::settings::save(&path, &settings).is_ok() {
        App::apply_live_settings(app, &settings);
    }
}

fn group_chips(app: &AppRef, live: &[(String, Vec<TabId>)]) -> Vec<BarGroup> {
    let settings = App::settings(app);
    if !settings.bookmarks_bar_tab_groups {
        return Vec::new();
    }
    let prefs = App::tab_group_preferences(app);
    let chip = |domain: &str, pinned: bool| {
        let key = DomainKey::parse(domain);
        BarGroup {
            key: domain.to_owned(),
            name: key
                .as_ref()
                .map(|key| prefs.name(key).map_or_else(|| crate::tab_organisation::suggested_name(key), str::to_owned))
                .unwrap_or_else(|| domain.to_owned()),
            pinned,
            live: live.iter().any(|(live_domain, _)| live_domain == domain),
            colour: key.as_ref().and_then(|key| prefs.colour(key)).map_or(-1, i32::from),
        }
    };
    let mut chips: Vec<BarGroup> = settings.address.bar_groups.iter().map(|domain| chip(domain, true)).collect();
    for (domain, _) in live {
        if !settings.address.bar_groups.contains(domain) {
            chips.push(chip(domain, false));
        }
    }
    chips
}

fn favicon_for(url: &str) -> String {
    let host = url::Url::parse(url).ok().and_then(|url| url.host_str().map(str::to_owned));
    host.and_then(|host| crate::favicon::load_favicon_bytes(&host).ok().flatten())
        .and_then(|png| chrome_host::protocol::favicon_uri(&png))
        .filter(|uri| uri.len() <= 8 * 1024)
        .unwrap_or_default()
}

fn bar_items(store: &BookmarkStore, folder: u64) -> Vec<Bookmark> {
    let parent = (folder != 0 && store.items().iter().any(|item| item.id == folder && matches!(item.kind, BookmarkKind::Folder))).then_some(folder);
    store.children(parent).into_iter().cloned().collect()
}

fn snapshot_for(app: &AppRef, owner: HWND) -> Option<(String, BuiltFrom)> {
    let settings = App::settings(app);
    let profile = crate::profile::active().ok()?;
    let path = profile.root().join("bookmarks-v2.txt");
    let stamp = std::fs::metadata(&path).ok().and_then(|meta| Some((meta.modified().ok()?, meta.len())));
    let live = live_groups(app);
    pin_new_groups(owner, app, &live);
    let groups = group_chips(app, &live);
    let built = BuiltFrom {
        stamp,
        folder: settings.address.bar_folder,
        display: settings.address.bar_display.key().to_owned(),
        groups: groups.iter().map(|g| (g.key.clone(), g.name.clone(), g.pinned, g.live, g.colour)).collect(),
    };
    let unchanged = with(owner, |rt| rt.built.as_ref() == Some(&built) && rt.snapshot.is_some()).unwrap_or(false);
    if unchanged {
        return None;
    }
    let store = if crate::productivity::profile_allows_productivity(&profile) { BookmarkStore::load(profile.root()).unwrap_or_default() } else { BookmarkStore::new() };
    let items: Vec<BarItem> = bar_items(&store, settings.address.bar_folder)
        .into_iter()
        .map(|item| match item.kind {
            BookmarkKind::Folder => BarItem { id: item.id, folder: true, title: item.title, url: String::new(), fav: String::new() },
            BookmarkKind::Link { url, .. } => BarItem { id: item.id, folder: false, title: item.title, fav: favicon_for(&url), url },
        })
        .collect();
    Some((address_page::bar_snapshot_json(settings.address.bar_display.key(), &items, &groups), built))
}

/// Brings the bar in line with the settings and the tabs: reserves (or frees) its room, creates and
/// places its host, and sends it its content when that changed. Called from every chrome sync, so it
/// is cheap when nothing changed.
pub(crate) fn sync(owner: HWND, app: &AppRef) {
    if !chrome_host::html_ui_enabled() {
        return;
    }
    let dpi = super::window::split_px(owner, 96).max(96) as u32;
    let reserve = reserve_px(app, dpi);
    if super::window::apply_bar_reserve(owner, app, reserve) {
        return; // the relayout runs this again with the room in place
    }
    if reserve.0 == 0 {
        hide(owner);
        return;
    }
    ensure_host(owner);
    let Some((host, container, ready)) = with(owner, |rt| (rt.host.clone(), rt.container, rt.ready && !rt.failed)) else { return };
    let (Some(host), Some(container), true) = (host, container, ready) else { return };
    let Some(viewport) = super::window::page_viewport(owner) else { return };
    let rect = if reserve.1 { RECT { left: viewport.left, top: viewport.bottom, right: viewport.right, bottom: viewport.bottom + reserve.0 } } else { RECT { left: viewport.left, top: viewport.top - reserve.0, right: viewport.right, bottom: viewport.top } };
    let state = super::window::split_host_state(owner, App::settings_accent(app));
    if with(owner, |rt| rt.state.as_deref() != Some(state.as_str())).unwrap_or(false) && host.post_json(&state).is_ok() {
        let _ = with(owner, |rt| rt.state = Some(state));
    }
    if let Some((snapshot, built)) = snapshot_for(app, owner) {
        if with(owner, |rt| rt.snapshot.as_deref() != Some(snapshot.as_str())).unwrap_or(true) && host.post_json(&snapshot).is_ok() {
            let _ = with(owner, |rt| {
                rt.snapshot = Some(snapshot);
                rt.built = Some(built);
            });
        } else {
            let _ = with(owner, |rt| rt.built = Some(built));
        }
    }
    if with(owner, |rt| rt.shown != Some(rect)).unwrap_or(false) {
        let (width, height) = ((rect.right - rect.left).max(1), (rect.bottom - rect.top).max(1));
        unsafe {
            let _ = SetWindowPos(container, Some(HWND_TOP), rect.left, rect.top, width, height, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
        let _ = host.set_bounds(RECT { left: 0, top: 0, right: width, bottom: height });
        let _ = host.set_visible(true);
        let _ = with(owner, |rt| rt.shown = Some(rect));
    }
}

fn hide(owner: HWND) {
    let Some((host, container, shown)) = with(owner, |rt| (rt.host.clone(), rt.container, rt.shown.take())) else { return };
    if shown.is_none() {
        return;
    }
    if let Some(host) = host {
        let _ = host.set_visible(false);
    }
    if let Some(container) = container {
        unsafe {
            let _ = ShowWindow(container, SW_HIDE);
        }
    }
}

// ----- commands -----

/// Runs the queued bar commands for `owner` (from `WM_BOOKMARK_BAR_COMMAND`).
pub(crate) fn run_queued(owner: HWND, app: &AppRef) {
    let commands: Vec<BarAction> = QUEUE.with(|queue| {
        let mut queue = queue.borrow_mut();
        let (mine, rest): (Vec<_>, Vec<_>) = queue.drain(..).partition(|(id, _)| *id == owner.0 as isize);
        *queue = rest;
        mine.into_iter().map(|(_, action)| action).collect()
    });
    for action in commands {
        run(owner, app, action);
    }
}

fn store_now() -> Option<(crate::profile::Profile, BookmarkStore)> {
    let profile = crate::profile::active().ok()?;
    let store = BookmarkStore::load(profile.root()).ok()?;
    Some((profile, store))
}

fn url_of(store: &BookmarkStore, id: u64) -> Option<String> {
    store.items().iter().find(|item| item.id == id).and_then(|item| match &item.kind {
        BookmarkKind::Link { url, .. } => Some(url.clone()),
        BookmarkKind::Folder => None,
    })
}

/// --- wave-1 fixes --- "Open bookmarks in a new tab": a bookmark opens in a new tab when asked (middle
/// click, the menu) or when the option is on.
fn opens_in_new_tab(asked: bool, address: &crate::address_settings::AddressSettings) -> bool {
    asked || address.bookmark_new_tab
}

fn open_url(owner: HWND, app: &AppRef, url: &str, new_tab: bool) {
    let new_tab = opens_in_new_tab(new_tab, &App::settings(app).address);
    App::submit_with(app, url, Options { new_tab, ..Options::default() });
    App::focus_page(app);
    super::window::split_ui_invalidate(owner);
}

fn cursor() -> POINT {
    let mut point = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut point);
    }
    point
}

/// Menu item ids: a bookmark is its id; "open all" in folder `f` is `OPEN_ALL | f`; a group action is
/// `GROUP | n`.
const OPEN_ALL: usize = 1 << 40;
const NEW_TAB: usize = 1 << 41;
const COPY: usize = 1 << 42;
const DELETE: usize = 1 << 43;

fn folder_entries(store: &BookmarkStore, folder: u64, depth: u32) -> Vec<super::menu::Entry> {
    use super::menu::{Entry, Icon, Item};
    let mut entries = Vec::new();
    let children = store.children(Some(folder));
    for item in children.iter().take(MAX_MENU_ITEMS) {
        match &item.kind {
            BookmarkKind::Link { url, .. } => {
                let mut entry = Item::new(item.id as usize, item.title.clone(), Icon::None).glyph("bookmark");
                if let Ok(png) = crate::favicon::load_favicon_bytes(url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_default().as_str()) {
                    entry = entry.favicon_png(png.map(Rc::new));
                }
                entries.push(Entry::Item(entry));
            }
            BookmarkKind::Folder => {
                let nested = if depth < MAX_MENU_DEPTH { folder_entries(store, item.id, depth + 1) } else { Vec::new() };
                let mut entry = Item::new(item.id as usize, item.title.clone(), Icon::None).glyph("folder");
                if !nested.is_empty() {
                    entry = entry.submenu_with(nested);
                } else {
                    entry = entry.disabled();
                }
                entries.push(Entry::Item(entry));
            }
        }
    }
    if entries.is_empty() {
        entries.push(Entry::Item(Item::new(0, "Empty", Icon::None).disabled()));
    }
    let links = crate::chrome_host::panels::bookmarks::links_under(store.items(), folder).len();
    if links > 1 || (links == 1 && depth == 0) {
        entries.push(Entry::Separator);
        entries.push(Entry::Item(Item::new(OPEN_ALL | folder as usize, "Open all in tabs", Icon::None).glyph("external")));
    }
    entries
}

fn show_folder(owner: HWND, app: &AppRef, id: u64, anchor: POINT) {
    use super::menu::{Menu, Placement};
    let Some((_, store)) = store_now() else { return };
    let entries = folder_entries(&store, id, 0);
    let raw = owner.0 as isize;
    super::menu::show(owner, anchor, Placement::BelowLeft, Menu { header: None, entries, footer: String::new() }, "bookmark-folder", move |app, picked| {
        let Some(picked) = picked else { return };
        pick(HWND(raw as *mut _), app, picked.id);
    });
    let _ = app;
}

/// A pick from a folder, overflow or context menu.
fn pick(owner: HWND, app: &AppRef, id: usize) {
    let Some((profile, mut store)) = store_now() else { return };
    if id & OPEN_ALL != 0 {
        let folder = (id & (OPEN_ALL - 1)) as u64;
        let urls = crate::chrome_host::panels::bookmarks::links_under(store.items(), folder);
        super::address_bar::open_many(owner, app, &urls);
        return;
    }
    if id & (NEW_TAB | COPY | DELETE) != 0 {
        let target = (id & (NEW_TAB - 1)) as u64;
        match (id & (NEW_TAB | COPY | DELETE), url_of(&store, target)) {
            (NEW_TAB, Some(url)) => open_url(owner, app, &url, true),
            (COPY, Some(url)) => {
                let _ = super::window::copy_text_to_clipboard(owner, &url);
            }
            (DELETE, _) => {
                store.delete(target);
                let mut extras = crate::bookmark_extras::Extras::load(profile.root());
                extras.remove(target);
                if store.save(profile.root()).is_ok() {
                    let _ = extras.save(profile.root());
                    super::post(owner, super::WM_RAIL_CHANGED, WPARAM(0), LPARAM(0));
                }
            }
            _ => {}
        }
        return;
    }
    if let Some(url) = url_of(&store, id as u64) {
        open_url(owner, app, &url, false);
    }
}

fn run(owner: HWND, app: &AppRef, action: BarAction) {
    use super::menu::{Entry, Icon, Item, Menu, Placement};
    match action {
        BarAction::Open { id, new_tab } => {
            if let Some((_, store)) = store_now() {
                if let Some(url) = url_of(&store, id) {
                    open_url(owner, app, &url, new_tab);
                }
            }
        }
        BarAction::Folder { id, rect } => {
            if let Some(anchor) = anchor(owner, rect) {
                show_folder(owner, app, id, anchor);
            }
        }
        BarAction::Context { id } => {
            let Some((_, store)) = store_now() else { return };
            let Some(item) = store.items().iter().find(|item| item.id == id) else { return };
            let mut entries = Vec::new();
            match &item.kind {
                BookmarkKind::Link { .. } => {
                    entries.push(Entry::Item(Item::new(id as usize, "Open", Icon::None).glyph("enter")));
                    entries.push(Entry::Item(Item::new(NEW_TAB | id as usize, "Open in a new tab", Icon::None).glyph("external")));
                    entries.push(Entry::Item(Item::new(COPY | id as usize, "Copy address", Icon::None).glyph("copy")));
                    entries.push(Entry::Separator);
                }
                BookmarkKind::Folder => {
                    entries.push(Entry::Item(Item::new(OPEN_ALL | id as usize, "Open all in tabs", Icon::None).glyph("external").disabled_if(crate::chrome_host::panels::bookmarks::links_under(store.items(), id).is_empty())));
                    entries.push(Entry::Separator);
                }
            }
            entries.push(Entry::Item(Item::new(DELETE | id as usize, "Delete", Icon::None).glyph("trash")));
            let raw = owner.0 as isize;
            super::menu::show(owner, cursor(), Placement::BelowLeft, Menu { header: None, entries, footer: String::new() }, "bookmark-item", move |app, picked| {
                if let Some(picked) = picked {
                    pick(HWND(raw as *mut _), app, picked.id);
                }
            });
        }
        BarAction::Group { key } => {
            let live = live_groups(app);
            match live.iter().find(|(domain, _)| *domain == key) {
                Some((_, members)) => {
                    // The shown member of the group when one is, else its first.
                    let active = App::active_tab_id(app);
                    let target = members.iter().copied().find(|id| *id == active).or_else(|| members.first().copied());
                    if let Some(id) = target {
                        let _ = App::select_tab(app, id);
                    }
                }
                None => App::submit_with(app, &format!("https://{key}/"), Options { new_tab: true, ..Options::default() }),
            }
            super::window::split_ui_invalidate(owner);
        }
        BarAction::GroupContext { key } => {
            let settings = App::settings(app);
            let pinned = settings.address.bar_groups.contains(&key);
            let entries = vec![Entry::Item(Item::new(1, if pinned { "Unpin from the bookmarks bar" } else { "Pin to the bookmarks bar" }, Icon::None).glyph("pin"))];
            let raw = owner.0 as isize;
            super::menu::show(owner, cursor(), Placement::BelowLeft, Menu { header: None, entries, footer: String::new() }, "bookmark-group", move |app, picked| {
                if picked.is_none() {
                    return;
                }
                let mut list = App::settings(app).address.bar_groups.clone();
                if let Some(at) = list.iter().position(|domain| *domain == key) {
                    list.remove(at);
                } else if list.len() < 32 {
                    list.push(key.clone());
                }
                save_pinned(app, &list);
                super::window::split_ui_invalidate(HWND(raw as *mut _));
            });
        }
        BarAction::Overflow { first, rect } => {
            let Some((_, store)) = store_now() else { return };
            let settings = App::settings(app);
            let items = bar_items(&store, settings.address.bar_folder);
            let mut entries = Vec::new();
            for item in items.iter().skip(first).take(MAX_MENU_ITEMS) {
                match &item.kind {
                    BookmarkKind::Link { .. } => entries.push(Entry::Item(Item::new(item.id as usize, item.title.clone(), Icon::None).glyph("bookmark"))),
                    BookmarkKind::Folder => {
                        let nested = folder_entries(&store, item.id, 1);
                        entries.push(Entry::Item(Item::new(item.id as usize, item.title.clone(), Icon::None).glyph("folder").submenu_with(nested)));
                    }
                }
            }
            if entries.is_empty() {
                return;
            }
            let Some(anchor) = anchor(owner, rect) else { return };
            let raw = owner.0 as isize;
            super::menu::show(owner, anchor, Placement::BelowLeft, Menu { header: None, entries, footer: String::new() }, "bookmark-overflow", move |app, picked| {
                if let Some(picked) = picked {
                    pick(HWND(raw as *mut _), app, picked.id);
                }
            });
        }
    }
}

/// The screen point a menu under the bar's element at `rect` (the bar's CSS pixels) hangs from.
fn anchor(owner: HWND, rect: HoverRect) -> Option<POINT> {
    let host = with(owner, |rt| rt.host.clone()).flatten()?;
    let screen = host.screen_bounds()?;
    let scale = super::window::split_px(owner, 96) as f32 / 96.0;
    let element = super::html_hover::anchor_to_screen(screen, scale, rect);
    Some(POINT { x: element.left, y: element.bottom + (2.0 * scale).round() as i32 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::layout::Rect;

    // --- wave-1 fixes --- what the bar's options do.
    #[test]
    fn the_bar_shows_as_its_option_says() {
        let app = crate::app::test_app();
        for (mode, shown) in [(BookmarksBar::Never, false), (BookmarksBar::Always, true), (BookmarksBar::NewTab, true)] {
            App::apply_live_settings(&app, &crate::settings::Settings { bookmarks_bar: mode, ..crate::settings::Settings::default() });
            assert_eq!(wanted(&app), shown, "{mode:?} on a new tab page");
        }
        crate::app::test_show_page(&app, App::active_tab_id(&app), "https://a.example/");
        assert!(!wanted(&app), "new tab page only: hidden on a page");
        App::apply_live_settings(&app, &crate::settings::Settings { bookmarks_bar: BookmarksBar::Always, ..crate::settings::Settings::default() });
        assert!(wanted(&app), "always: shown on a page too");
    }

    #[test]
    fn tab_group_chips_and_pinning_follow_their_options() {
        let app = crate::app::test_app();
        let live = vec![("a.example".to_owned(), vec![App::active_tab_id(&app)])];
        App::apply_live_settings(&app, &crate::settings::Settings::default());
        assert!(group_chips(&app, &live).is_empty(), "off by default: no group chips");
        App::apply_live_settings(&app, &crate::settings::Settings { bookmarks_bar_tab_groups: true, ..crate::settings::Settings::default() });
        let chips = group_chips(&app, &live);
        assert_eq!(chips.iter().map(|chip| chip.key.as_str()).collect::<Vec<_>>(), ["a.example"]);
        assert!(chips[0].live && !chips[0].pinned);
        let pinned = vec!["old.example".to_owned()];
        assert_eq!(pins_after(false, vec!["new.example".into()], &pinned), None, "off: nothing is pinned");
        assert_eq!(pins_after(true, Vec::new(), &pinned), None, "nothing new");
        assert_eq!(pins_after(true, vec!["new.example".into(), "old.example".into()], &pinned), Some(vec!["old.example".to_owned(), "new.example".to_owned()]));
    }

    #[test]
    fn a_bookmark_opens_in_a_new_tab_when_asked_or_when_the_option_says() {
        let mut address = crate::address_settings::AddressSettings { bookmark_new_tab: false, ..Default::default() };
        assert!(!opens_in_new_tab(false, &address));
        assert!(opens_in_new_tab(true, &address), "a middle click still opens a new tab");
        address.bookmark_new_tab = true;
        assert!(opens_in_new_tab(false, &address));
    }

    #[test]
    fn the_bar_takes_its_height_from_the_top_or_the_bottom_of_the_page() {
        let viewport = Rect::new(40, 100, 1240, 700);
        assert_eq!(reserved_viewport(viewport, (0, false)), viewport);
        assert_eq!(reserved_viewport(viewport, (28, false)), Rect::new(40, 128, 1240, 700));
        assert_eq!(reserved_viewport(viewport, (28, true)), Rect::new(40, 100, 1240, 672));
        // A window with no room never gives the bar the whole page.
        let tiny = Rect::new(0, 0, 100, 20);
        assert_eq!(reserved_viewport(tiny, (28, false)).height(), 1);
    }

    #[test]
    fn menu_ids_keep_a_bookmark_and_its_action_apart() {
        for flag in [OPEN_ALL, NEW_TAB, COPY, DELETE] {
            let id = flag | 123_456usize;
            assert_eq!((id & (flag - 1)) as u64, 123_456);
            assert_ne!(id & flag, 0);
        }
        assert_eq!(OPEN_ALL & (NEW_TAB | COPY | DELETE), 0);
    }

    #[test]
    fn a_folder_menu_lists_its_items_and_offers_open_all() {
        let mut store = BookmarkStore::new();
        let reading = store.add_folder(None, "Reading").unwrap();
        store.add_bookmark(Some(reading), "A", "https://a.example/").unwrap();
        store.add_bookmark(Some(reading), "B", "https://b.example/").unwrap();
        let entries = folder_entries(&store, reading, 0);
        assert_eq!(entries.len(), 4, "two items, a separator, open all");
        assert!(matches!(&entries[3], super::super::menu::Entry::Item(item) if item.label == "Open all in tabs" && item.id & OPEN_ALL != 0));
        let empty = store.add_folder(None, "Empty").unwrap();
        assert_eq!(folder_entries(&store, empty, 0).len(), 1, "an empty folder says so");
    }

    #[test]
    fn the_bar_shows_the_chosen_folder_or_the_top_level() {
        let mut store = BookmarkStore::new();
        let folder = store.add_folder(None, "Bar").unwrap();
        store.add_bookmark(Some(folder), "In", "https://in.example/").unwrap();
        store.add_bookmark(None, "Out", "https://out.example/").unwrap();
        assert_eq!(bar_items(&store, folder).iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), ["In"]);
        assert_eq!(bar_items(&store, 0).iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), ["Bar", "Out"]);
        assert_eq!(bar_items(&store, 999).len(), 2, "a folder that no longer exists shows the top level");
    }
}
