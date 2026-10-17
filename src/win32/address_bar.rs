//! The window's side of the address bar options: what the toolbar's address field and search field are
//! told, what they ask for, the drop-down rows under the options, and the engine suggestions that
//! arrive late from the network (only after an engine nickname or in the search field).
//!
//! Every function runs on the UI thread except the suggestion fetch, which runs on one short-lived
//! thread and hands its answer back through a queue and `WM_ADDRESS_SUGGEST`.

use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};

use crate::{
    address_input::Options,
    address_settings::Scope,
    address_suggest,
    app::{App, AppRef},
    chrome_host::{
        address_page::{self, ToolbarAction},
        protocol::PanelFields,
    },
    omnibox::SuggestKind,
    search_engines::{Engine, EngineStore},
    win32::html_suggest::{self, Row, Target},
};

/// Engine suggestions arrived (see [`take_answers`]).
/// (WM_APP + 0x5E1 is `WM_CHAIN_STEP`; the integration moved this one so neither shadows the other.)
pub(crate) const WM_ADDRESS_SUGGEST: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x5E3;

/// Most suggestions one answer keeps.
const MAX_ANSWERS: usize = 8;
/// A suggestion request waits this long for the next keystroke before it is sent.
const DEBOUNCE: Duration = Duration::from_millis(160);

/// What a window's open drop-down answers, so a late engine answer can refresh it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Listed {
    /// The address field's list for `text`, edit `seq`.
    Address { seq: u32, text: String },
    /// The search field's list for `text` with engine `engine`.
    Field { seq: u32, text: String, engine: String },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Cache {
    /// `(engine id, query)` the list answers.
    key: (String, String),
    suggestions: Vec<String>,
}

struct Answer {
    key: (String, String),
    suggestions: Vec<String>,
}

static INBOX: Mutex<Vec<Answer>> = Mutex::new(Vec::new());
static GENERATION: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// Per window: the generation of the toolbar page, the last `addr` and `sfield` sent to it.
    static SENT: RefCell<HashMap<isize, (u64, String, String)>> = RefCell::new(HashMap::new());
    /// Per window and per kind of tab shown (normal, private): the index of the engine picked in the
    /// search field's menu (wave-1 fixes: a pick in a normal tab never overrides the private engine).
    static CHOSEN: RefCell<HashMap<(isize, bool), usize>> = RefCell::new(HashMap::new());
    static LISTED: RefCell<HashMap<isize, Listed>> = RefCell::new(HashMap::new());
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
}

fn key(hwnd: HWND) -> isize {
    hwnd.0 as isize
}

/// The registrable domain of the shown page's host, for the unfocused address' emphasis.
fn base_domain(app: &AppRef) -> String {
    let id = App::active_tab_id(app);
    App::tab_committed_url(app, id)
        .and_then(|url| url.host_str().map(|host| crate::blocker::registrable_domain(&host.to_ascii_lowercase()).to_owned()))
        .unwrap_or_default()
}

/// The search field's engine for this window: the one chosen, else the last used when asked to keep
/// it, else the default. An index into `engines`.
fn chosen_engine(hwnd: HWND, app: &AppRef, engines: &EngineStore) -> usize {
    let private = App::active_tab_private(app);
    let picked = CHOSEN.with(|chosen| chosen.borrow().get(&(key(hwnd), private)).copied());
    pick_engine(engines, picked, &App::settings(app).address, private)
}

/// --- wave-1 fixes --- [`chosen_engine`]'s rule: an engine picked in the menu for this kind of tab;
/// else, in a private tab, the engine set for private windows; else the last used when asked to keep
/// it; else the default.
pub(super) fn pick_engine(engines: &EngineStore, picked: Option<usize>, address: &crate::address_settings::AddressSettings, private: bool) -> usize {
    let list = engines.engines();
    if let Some(index) = picked.filter(|index| *index < list.len()) {
        return index;
    }
    let wanted = if private {
        engines.effective(true).id.as_str()
    } else if address.keep_engine && !address.last_engine.is_empty() {
        address.last_engine.as_str()
    } else {
        engines.default_id()
    };
    list.iter().position(|engine| engine.id == wanted).unwrap_or(0)
}

/// Tells the toolbar page how the address field and the search field behave, when that changed (or
/// when the page was reloaded: `generation` is the toolbar page's). Called from every chrome sync.
pub(super) fn sync(hwnd: HWND, app: &AppRef, generation: u64) {
    let settings = App::settings(app);
    // "Show the full address" is read wherever an address is turned into text.
    crate::omnibox::set_show_full_address(settings.address.show_full);
    crate::omnibox::set_autocomplete(settings.address.autocomplete);
    let engines = crate::address_runtime::engines();
    let chosen = chosen_engine(hwnd, app, &engines);
    let addr = address_page::addr_json(&settings.address, &base_domain(app));
    let sfield = address_page::sfield_json(&settings.address, &engines, chosen);
    let fresh = SENT.with(|sent| match sent.borrow().get(&key(hwnd)) {
        Some((seen, a, s)) => (*seen != generation, *a != addr, *s != sfield),
        None => (true, true, true),
    });
    if !(fresh.0 || fresh.1 || fresh.2) {
        return;
    }
    let (send_addr, send_field) = (fresh.0 || fresh.1, fresh.0 || fresh.2);
    if send_addr {
        super::window::html_toolbar_post(hwnd, &addr);
    }
    if send_field {
        super::window::html_toolbar_post(hwnd, &sfield);
    }
    SENT.with(|sent| {
        sent.borrow_mut().insert(key(hwnd), (generation, addr, sfield));
    });
}

/// Forgets what a closed window was told.
pub(super) fn forget(hwnd: HWND) {
    SENT.with(|sent| sent.borrow_mut().remove(&key(hwnd)));
    CHOSEN.with(|chosen| chosen.borrow_mut().retain(|(window, _), _| *window != key(hwnd)));
    LISTED.with(|listed| listed.borrow_mut().remove(&key(hwnd)));
}

/// Whether typing opens the drop-down.
pub(super) fn list_on_typing(app: &AppRef) -> bool {
    App::settings(app).address.dropdown_while_typing
}

/// The suggestions an engine answered for `query`, now if they are in hand; otherwise they are asked
/// for (after a short wait for more typing) and `suggestions_ready` runs when they arrive.
fn engine_suggestions(hwnd: HWND, engine: &Engine, query: &str, allowed: bool) -> Vec<String> {
    if !allowed || query.trim().is_empty() || engine.suggest.is_empty() {
        return Vec::new();
    }
    let wanted = (engine.id.clone(), query.trim().to_owned());
    if let Some(hit) = CACHE.with(|cache| Some(cache.borrow()).filter(|cache| cache.key == wanted).map(|cache| cache.suggestions.clone())) {
        return hit;
    }
    let Some(url) = engine.suggest_url(query.trim()) else { return Vec::new() };
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let raw = key(hwnd);
    thread::spawn(move || {
        thread::sleep(DEBOUNCE);
        if GENERATION.load(Ordering::SeqCst) != generation {
            return; // typed on: the newer request supersedes this one
        }
        let Ok(body) = crate::translate::net::get_https(&url, 64 * 1024) else { return };
        let suggestions = address_suggest::parse_suggestions(&String::from_utf8_lossy(&body), MAX_ANSWERS);
        if let Ok(mut inbox) = INBOX.lock() {
            if inbox.len() < 8 {
                inbox.push(Answer { key: wanted, suggestions });
            }
        }
        super::post(HWND(raw as *mut _), WM_ADDRESS_SUGGEST, WPARAM(0), LPARAM(0));
    });
    Vec::new()
}

/// Engine suggestions arrived: keep them, and redraw the list they belong to.
pub(super) fn suggestions_ready(hwnd: HWND) {
    let answers: Vec<Answer> = INBOX.lock().map(|mut inbox| std::mem::take(&mut *inbox)).unwrap_or_default();
    for answer in answers {
        CACHE.with(|cache| *cache.borrow_mut() = Cache { key: answer.key.clone(), suggestions: answer.suggestions });
    }
    let listed = LISTED.with(|listed| listed.borrow().get(&key(hwnd)).cloned());
    match listed {
        Some(Listed::Address { seq, text }) => super::window::html_suggest_refresh(hwnd, seq, &text),
        Some(Listed::Field { seq, text, engine }) => show_field_rows(hwnd, seq, &text, &engine),
        None => {}
    }
}

/// The text whose rows are wanted, with what the window knows about it.
#[derive(Clone, Copy)]
pub(super) struct Query<'a> {
    pub text: &'a str,
    /// The address `text` would navigate to, when it is one.
    pub typed_url: Option<&'a url::Url>,
    /// Open tabs other than the shown one.
    pub tabs: &'a [html_suggest::TabSource],
    pub seq: u32,
    /// Typing alone would not open the list, but the user asked for it.
    pub forced: bool,
}

/// Rows for the address field under the options. `history`, `completion` and `tabs` come from the
/// window (they are its state); the rest is read here.
pub(super) fn rows(
    hwnd: HWND,
    app: &AppRef,
    history: &crate::omnibox::History,
    completion: Option<&crate::omnibox::Completion>,
    query: &Query<'_>,
) -> Vec<Row> {
    let Query { text, typed_url, tabs, seq, forced } = *query;
    let settings = App::settings(app);
    let address = &settings.address;
    if !address.dropdown_while_typing && !forced {
        return Vec::new();
    }
    let engines = crate::address_runtime::engines();
    let private = App::active_tab_private(app);
    let typed = crate::address_runtime::typed();
    let marks = html_suggest::marks();
    // Engine suggestions only follow a nickname, only with the source on, and never in a private tab.
    let suggestions = match engines.split_nickname(text) {
        Some((engine, query)) if address.suggest_in_address && address.has_source(crate::address_settings::Source::Search) && !private => {
            engine_suggestions(hwnd, engine, query, true)
        }
        _ => Vec::new(),
    };
    LISTED.with(|listed| listed.borrow_mut().insert(key(hwnd), Listed::Address { seq, text: text.to_owned() }));
    let env = html_suggest::Env { settings: address, engines: &engines, typed_history: &typed, engine_suggestions: &suggestions, private };
    html_suggest::rows_for(history, completion, text, typed_url, tabs, &marks, &env)
}

/// The typed-history list: what was typed before, newest first.
pub(super) fn typed_list(hwnd: HWND, app: &AppRef, seq: u32) {
    let settings = App::settings(app);
    let typed = crate::address_runtime::typed();
    let rows = html_suggest::typed_rows(&typed, &settings.address);
    LISTED.with(|listed| listed.borrow_mut().remove(&key(hwnd)));
    super::window::html_suggest_present(hwnd, app, seq, "", rows);
}

/// Rows for the search field's text: engine suggestions with the field's engine, under the default
/// row that searches for the text as typed.
fn show_field_rows(hwnd: HWND, seq: u32, text: &str, engine_id: &str) {
    let Some(app) = super::window::app_of(hwnd) else { return };
    let engines = crate::address_runtime::engines();
    let settings = App::settings(&app);
    let Some(engine) = engines.find(engine_id) else { return };
    let private = App::active_tab_private(&app);
    let suggestions = engine_suggestions(hwnd, engine, text, field_suggestions_allowed(&settings.address, private));
    let label = address_suggest::engine_label(engine, settings.address.show_nickname);
    let mut rows = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for shown in std::iter::once(text.trim().to_owned()).chain(suggestions) {
        if shown.is_empty() || seen.contains(&shown) {
            continue;
        }
        seen.push(shown.clone());
        rows.push(Row {
            item: crate::chrome_host::protocol::SuggestItem {
                kind: SuggestKind::Search.wire().to_owned(),
                text: shown.clone(),
                title: shown.clone(),
                url: if rows.is_empty() { label.clone() } else { String::new() },
                png: None,
                badge: false,
            },
            target: Target::EngineSearch { engine: engine.id.clone(), query: shown },
            fav_url: String::new(),
        });
        if rows.len() >= usize::from(settings.address.max_results) {
            break;
        }
    }
    LISTED.with(|listed| listed.borrow_mut().insert(key(hwnd), Listed::Field { seq, text: text.to_owned(), engine: engine.id.clone() }));
    super::window::html_suggest_present(hwnd, &app, seq, text, rows);
}

/// Runs a toolbar command of the `addr` family.
pub(super) fn on_toolbar_command(hwnd: HWND, f: &PanelFields) {
    let Some(app) = super::window::app_of(hwnd) else { return };
    let Some(action) = address_page::toolbar_command(f) else { return };
    match action {
        ToolbarAction::Submit { text, how } => {
            super::html_hover::suggest_hide(hwnd);
            super::window::remember_typed_url(hwnd, &app, &text);
            let options = Options { new_tab: how == 1, expand: how == 2, ..Options::default() };
            App::submit_with(&app, &text, options);
            App::focus_page(&app);
        }
        ToolbarAction::Typed { seq } => typed_list(hwnd, &app, seq),
        ToolbarAction::Suggest { text, seq } => super::window::html_suggest_forced(hwnd, &app, seq, &text),
        ToolbarAction::Qr => show_qr(hwnd, &app),
        ToolbarAction::Search { text, engine, new_tab } => {
            super::html_hover::suggest_hide(hwnd);
            let engines = crate::address_runtime::engines();
            let Some(engine) = engines.engines().get(engine) else { return };
            let options = Options { new_tab, from_search_field: true, engine: Some(engine.id.as_str()), ..Options::default() };
            App::submit_with(&app, &text, options);
            App::focus_page(&app);
        }
        ToolbarAction::SearchEdit { text, engine, seq } => {
            let engines = crate::address_runtime::engines();
            let Some(engine) = engines.engines().get(engine) else { return };
            if text.trim().is_empty() {
                super::html_hover::suggest_close(hwnd);
                return;
            }
            show_field_rows(hwnd, seq, &text, &engine.id.clone());
        }
        ToolbarAction::Engines { rect } => show_engine_menu(hwnd, &app, rect),
    }
}

/// The engine list under the search field's button: pick one to search with it from now on.
fn show_engine_menu(hwnd: HWND, app: &AppRef, rect: crate::chrome_host::protocol::HoverRect) {
    use super::menu::{Entry, Icon, Item, Menu, Placement};
    let Some(anchor) = super::window::toolbar_anchor(hwnd, rect) else { return };
    let engines = crate::address_runtime::engines();
    let settings = App::settings(app);
    let chosen = chosen_engine(hwnd, app, &engines);
    let mut entries: Vec<Entry> = engines
        .engines()
        .iter()
        .enumerate()
        .take(address_page::MAX_FIELD_ENGINES)
        .map(|(index, engine)| {
            let label = address_suggest::engine_label(engine, settings.address.show_nickname);
            Entry::Item(Item::new(index + 1, label, Icon::None).glyph("search").checked(index == chosen))
        })
        .collect();
    entries.push(Entry::Separator);
    entries.push(Entry::Item(Item::new(0, "Manage search engines", Icon::Gear).glyph("gear")));
    let raw = key(hwnd);
    super::menu::show(
        hwnd,
        anchor,
        Placement::BelowLeft,
        Menu { header: None, entries, footer: String::new() },
        "search-engines",
        move |app, picked| {
            let Some(picked) = picked else { return };
            let hwnd = HWND(raw as *mut _);
            if picked.id == 0 {
                super::settings_page::open(hwnd, app, Some(crate::internal_page::Section::Search), false);
                return;
            }
            let index = picked.id - 1;
            let engines = crate::address_runtime::engines();
            let Some(engine) = engines.engines().get(index) else { return };
            let private = App::active_tab_private(app);
            CHOSEN.with(|chosen| chosen.borrow_mut().insert((key(hwnd), private), index));
            // A private pick is not remembered for normal tabs.
            if remembers_pick(App::settings(app).address.keep_engine, private) {
                remember_engine(app, &engine.id);
            }
            super::window::html_sync_now(hwnd);
        },
    );
}

/// --- wave-1 fixes --- the search field asks its engine for suggestions only with "Suggestions in the
/// search field" on, and never in a private tab.
fn field_suggestions_allowed(address: &crate::address_settings::AddressSettings, private: bool) -> bool {
    address.suggest_in_field && !private
}

/// --- wave-1 fixes --- a pick in the engine menu is remembered ("Keep the last selected engine") only
/// from a normal tab.
fn remembers_pick(keep: bool, private: bool) -> bool {
    keep && !private
}

/// Saves the engine the search field last used (when "keep last selected engine" is on).
fn remember_engine(app: &AppRef, id: &str) {
    let Ok(profile) = crate::profile::active() else { return };
    let path = profile.settings_file();
    let Ok(mut settings) = crate::settings::read_strict(&path) else { return };
    if settings.address.last_engine == id || !settings.address.set("search_last_engine", id) {
        return;
    }
    if crate::settings::save(&path, &settings).is_ok() {
        App::apply_live_settings(app, &settings);
    }
}

/// Picking a drop-down row of the kinds this module added. `true` when handled here.
pub(super) fn run_target(hwnd: HWND, app: &AppRef, target: &Target) -> bool {
    match target {
        Target::Fill(text) => {
            super::window::set_toolbar_text(hwnd, text);
            true
        }
        Target::EngineSearch { engine, query } => {
            let new_tab = false;
            let options = Options { new_tab, from_search_field: true, engine: Some(engine.as_str()), ..Options::default() };
            App::submit_with(app, query, options);
            App::focus_page(app);
            true
        }
        _ => false,
    }
}

/// Opens every address in `urls` in its own new tab, after asking when there are as many as
/// "confirm when opening N or more bookmarks" says.
pub(super) fn open_many(owner: HWND, app: &AppRef, urls: &[String]) {
    if urls.is_empty() {
        return;
    }
    if App::settings(app).address.confirm_needed(urls.len())
        && !super::ask_yes_no_owned(owner, &format!("Open {} bookmarks in new tabs?", urls.len()))
    {
        return;
    }
    for url in urls {
        App::submit_with(app, url, Options { new_tab: true, ..Options::default() });
    }
}

/// An `@bookmarks` / `@history` / `@tabs` nickname submitted with Enter: the first result of that
/// scope opens (or its tab is shown).
pub(super) fn scoped_submit(hwnd: HWND, app: &AppRef, scope: Scope, rest: &str) {
    let settings = App::settings(app);
    let Some(word) = settings.address.scope_word(scope) else { return };
    let rows = super::window::html_suggest_rows_for(hwnd, app, &format!("{word} {rest}"));
    if let Some(row) = rows.first() {
        super::window::html_suggest_run(hwnd, row.target.clone());
    }
}

fn show_qr(hwnd: HWND, app: &AppRef) {
    let id = App::active_tab_id(app);
    let Some(url) = App::tab_committed_url(app, id) else { return };
    super::address_dialog::show_qr(hwnd, app, url.as_str());
}

// --- wave-1 fixes ---
#[cfg(test)]
mod wave1_tests {
    use super::*;

    /// T2/T3: the search field in a private tab searches with the engine set for private windows,
    /// whatever the last-used engine or a pick made in a normal tab says; the chosen and remembered
    /// engine still lead in normal tabs.
    #[test]
    fn the_search_field_uses_the_private_engine_in_a_private_tab() {
        let mut engines = EngineStore::default();
        let ids: Vec<String> = engines.engines().iter().map(|engine| engine.id.clone()).collect();
        assert!(ids.len() >= 3, "{ids:?}");
        engines.set_default(&ids[0]).unwrap();
        engines.set_private(&ids[2]).unwrap();
        let mut address = crate::address_settings::AddressSettings { keep_engine: true, last_engine: ids[1].clone(), ..Default::default() };
        assert_eq!(pick_engine(&engines, None, &address, false), 1, "normal: the remembered engine");
        assert_eq!(pick_engine(&engines, None, &address, true), 2, "private: the private engine, not the remembered one");
        assert_eq!(pick_engine(&engines, Some(0), &address, true), 0, "a pick made in a private tab still wins there");
        address.keep_engine = false;
        assert_eq!(pick_engine(&engines, None, &address, false), 0, "not kept: the default");
        assert_eq!(pick_engine(&engines, Some(99), &address, false), 0, "a pick that is gone falls back");
    }

    #[test]
    fn the_search_field_suggests_and_remembers_only_where_the_options_allow() {
        let mut address = crate::address_settings::AddressSettings { suggest_in_field: true, ..Default::default() };
        assert!(field_suggestions_allowed(&address, false));
        assert!(!field_suggestions_allowed(&address, true), "never in a private tab");
        address.suggest_in_field = false;
        assert!(!field_suggestions_allowed(&address, false));
        assert!(remembers_pick(true, false));
        assert!(!remembers_pick(true, true), "a private pick is not remembered");
        assert!(!remembers_pick(false, false));
    }

    /// The address drop-down: off while typing, nothing is listed unless the user asked for the list.
    #[test]
    fn the_address_drop_down_lists_nothing_while_typing_when_the_option_is_off() {
        let app = crate::app::test_app();
        let mut settings = crate::settings::Settings::default();
        settings.address.dropdown_while_typing = false;
        App::apply_live_settings(&app, &settings);
        let history = crate::omnibox::History::default();
        let query = |forced| Query { text: "rust", typed_url: None, tabs: &[], seq: 1, forced };
        assert!(rows(HWND::default(), &app, &history, None, &query(false)).is_empty());
        assert!(!rows(HWND::default(), &app, &history, None, &query(true)).is_empty(), "asked for: the list shows");
        settings.address.dropdown_while_typing = true;
        App::apply_live_settings(&app, &settings);
        assert!(!rows(HWND::default(), &app, &history, None, &query(false)).is_empty());
    }
}