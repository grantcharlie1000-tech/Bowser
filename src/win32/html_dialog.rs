//! Lazy HTML hosts layered over owned browser chrome windows.

use std::{cell::RefCell, collections::HashMap, rc::{Rc, Weak}, time::{SystemTime, UNIX_EPOCH}};
use windows::Win32::{Foundation::{HWND, RECT}, UI::WindowsAndMessaging::{DestroyWindow, GetClientRect}};
use crate::{
    app::AppRef,
    chrome_host::{host::{Accel, AccelHandler, ChromeEnv, ChromeHost, View}, protocol::{self, Command, PanelFields, PanelType}},
    profile,
    win32::{notes_panel, show_error_message},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind { Notes, Settings, Privacy }
impl Kind {
    fn view(self) -> View { match self { Self::Notes => View::DialogNotes, Self::Settings => View::DialogSettings, Self::Privacy => View::DialogPrivacy } }
    fn token(self) -> &'static str { match self { Self::Notes => "notes", Self::Settings => "settings", Self::Privacy => "privacy" } }
    fn title(self) -> &'static str { match self { Self::Notes => "Notes and Tasks", Self::Settings => "Settings", Self::Privacy => "Privacy dashboard" } }
}

struct Runtime { app: AppRef, owner: HWND, parent: HWND, kind: Kind, host: Option<ChromeHost>, ready: bool, closed: bool }
thread_local! { static RUNTIMES: RefCell<HashMap<isize, Rc<RefCell<Runtime>>>> = RefCell::new(HashMap::new()); }

pub(crate) fn attach(app: &AppRef, owner: HWND, parent: HWND, env: ChromeEnv, kind: Kind) {
    let runtime = Rc::new(RefCell::new(Runtime { app: app.clone(), owner, parent, kind, host: None, ready: false, closed: false }));
    RUNTIMES.with(|map| { map.borrow_mut().insert(parent.0 as isize, runtime.clone()); });
    let mut bounds = RECT::default();
    unsafe { let _ = GetClientRect(parent, &mut bounds); }
    let weak: Weak<RefCell<Runtime>> = Rc::downgrade(&runtime);
    let commands: Rc<dyn Fn(Command)> = Rc::new(move |command| if let Some(state) = weak.upgrade() { handle(&state, command); });
    let failed_weak = Rc::downgrade(&runtime);
    let failed: Rc<dyn Fn()> = Rc::new(move || if let Some(state) = failed_weak.upgrade() { close_runtime(&state); });
    let accel_owner = owner;
    let accel: AccelHandler = Rc::new(move |key: Accel| super::window::html_panel_accelerator(accel_owner, key));
    let ready_weak = Rc::downgrade(&runtime);
    let view = kind.view();
    if let Err(error) = ChromeHost::create(&env, parent, bounds, view, commands, failed, Some(accel), move |result| match result {
        Ok(host) => if let Some(state) = ready_weak.upgrade() {
            if let Ok(mut runtime) = state.try_borrow_mut() {
                if runtime.closed { host.close(); return; }
                runtime.host = Some(host.clone());
                let _ = host.set_visible(true);
            } else { host.close(); }
        } else { host.close(); },
        Err(_) => if let Some(state) = ready_weak.upgrade() { close_runtime(&state); },
    }) { close_runtime(&runtime); show_error_message(&format!("{} could not be opened in HTML: {error}", kind.title())); }
}

pub(crate) fn open_privacy(owner: HWND, app: &AppRef, env: ChromeEnv) {
    if RUNTIMES.with(|map| map.borrow().contains_key(&(owner.0 as isize))) { return; }
    attach(app, owner, owner, env, Kind::Privacy);
}

pub(crate) fn resize(parent: HWND) {
    let runtime = RUNTIMES.with(|map| map.borrow().get(&(parent.0 as isize)).cloned());
    let Some(runtime) = runtime else { return; };
    let (host, closed) = runtime.try_borrow().map(|state| (state.host.clone(), state.closed)).unwrap_or((None, true));
    if closed { return; }
    if let Some(host) = host {
        let mut bounds = RECT::default(); unsafe { let _ = GetClientRect(parent, &mut bounds); }
        if host.set_bounds(bounds).is_err() { close_runtime(&runtime); }
    }
}

pub(crate) fn close(parent: HWND) {
    if let Some(runtime) = RUNTIMES.with(|map| map.borrow_mut().remove(&(parent.0 as isize))) { close_runtime(&runtime); }
}

fn close_runtime(runtime: &Rc<RefCell<Runtime>>) {
    let (parent, host) = runtime.try_borrow_mut().ok().map(|mut state| { state.closed = true; (state.parent, state.host.take()) }).unwrap_or((HWND::default(), None));
    RUNTIMES.with(|map| { map.borrow_mut().remove(&(parent.0 as isize)); });
    if let Some(host) = host { host.close(); }
}

fn handle(runtime: &Rc<RefCell<Runtime>>, command: Command) {
    match command {
        Command::Ready => {
            if let Ok(mut state) = runtime.try_borrow_mut() { state.ready = true; }
            send_current(runtime);
        }
        Command::Panel { kind: PanelType::Panel, f } if empty_fields(&f, false) && f.a == "dismiss" => {
            let (parent, kind) = { let state = runtime.borrow(); (state.parent, state.kind) };
            if kind == Kind::Privacy {
                RUNTIMES.with(|map| { map.borrow_mut().remove(&(parent.0 as isize)); });
                close_runtime(runtime);
            } else { unsafe { let _ = DestroyWindow(parent); } }
        }
        Command::Panel { kind: PanelType::Notes, f } if runtime.borrow().kind == Kind::Notes => handle_notes(runtime, f),
        Command::Panel { kind: PanelType::Settings, f } if runtime.borrow().kind == Kind::Settings => handle_settings(runtime, f),
        Command::Panel { kind: PanelType::Privacy, f } if runtime.borrow().kind == Kind::Privacy => handle_privacy(runtime, f),
        _ => {}
    }
}

fn send_current(runtime: &Rc<RefCell<Runtime>>) {
    let (host, ready, owner, kind) = match runtime.try_borrow() { Ok(state) => (state.host.clone(), state.ready, state.owner, state.kind), Err(_) => return };
    let Some(host) = host.filter(|_| ready) else { return; };
    let _ = host.post_json(&super::window::html_panel_state(owner));
    if let Some(message) = protocol::panelshow_json(kind.token()) { let _ = host.post_json(&message); }
    let snapshot = match kind { Kind::Notes => notes_snapshot(), Kind::Settings => settings_snapshot(), Kind::Privacy => privacy_snapshot(runtime) };
    if let Some(snapshot) = snapshot { let _ = host.post_json(&snapshot); }
}

fn privacy_snapshot(runtime: &Rc<RefCell<Runtime>>) -> Option<String> {
    let app = runtime.try_borrow().ok()?.app.clone();
    let profile = profile::active().ok();
    let snapshot = super::privacy_panel::gather(&app, profile.as_ref(), None);
    protocol::snap_json("privacy", |out| {
        out.string("profile", &snapshot.profile); out.bool("guest", snapshot.guest);
        out.object("stats", |stats| {
            stats.uint("allTime", snapshot.stats.all_time_total()); stats.uint("session", snapshot.session_total);
            stats.uint("currentSite", snapshot.site_total);
            stats.array("topSites", |items| for (host, count) in &snapshot.top_sites { items.object(|item| { item.string("host", host); item.uint("count", *count); }); });
            stats.array("days", |items| for (day, count) in &snapshot.days { items.object(|item| { item.string("day", day); item.uint("count", *count); }); });
        });
        if let Some(site) = &snapshot.host { out.string("site", site); } else { out.null("site"); }
        out.string("security", match snapshot.security { crate::site_security::SiteSecurity::Secure => "secure", crate::site_security::SiteSecurity::NotSecure => "notsecure", crate::site_security::SiteSecurity::CertificateProblem => "certproblem", crate::site_security::SiteSecurity::Internal => "internal", crate::site_security::SiteSecurity::Unknown => "unknown", crate::site_security::SiteSecurity::NoSite => "none" });
        if let Some(blocking) = snapshot.blocking { out.bool("blocking", blocking); } else { out.null("blocking"); }
        if let Some(tracking) = snapshot.tracking { out.string("tracking", tracking); } else { out.null("tracking"); }
        if let Some(enabled) = snapshot.smart_screen { out.bool("smartScreen", enabled); } else { out.null("smartScreen"); }
        out.bool("blockerSetting", snapshot.blocker_setting); out.bool("sleeping", snapshot.sleeping); out.uint("sleepAfter", snapshot.sleep_after);
        out.array("permissions", |items| for (origin, permission, decision) in &snapshot.permissions { items.object(|item| {
            item.string("origin", origin); item.string("permission", permission.label());
            item.string("decision", match decision { crate::permissions::Decision::Allow => "allow", crate::permissions::Decision::Block => "block" });
        }); });
        if let Some(status) = &snapshot.status { out.string("status", status); } else { out.null("status"); }
    })
}

fn handle_privacy(runtime: &Rc<RefCell<Runtime>>, f: PanelFields) {
    let (app, owner) = match runtime.try_borrow() { Ok(state) => (state.app.clone(), state.owner), Err(_) => return };
    let no_extra = f.parent.is_none() && f.before.is_none() && f.on.is_none() && f.text.is_none() && f.url.is_none();
    let id = match f.a.as_str() {
        "clear-stats" if empty_fields(&f, false) => Some(8_800),
        "permission" if no_extra && f.id.is_some() && f.n.is_some_and(|choice| (0..=2).contains(&choice)) => {
            let index = f.id.unwrap_or_default(); let choice = f.n.unwrap_or_default() as usize;
            let Some(id) = index.checked_mul(3).and_then(|base| base.checked_add(choice as u64)).and_then(|value| value.checked_add(10_000)) else { return; }; usize::try_from(id).ok()
        }
        "clear-data" if no_extra && f.id.is_some_and(|kind| kind <= 2) && f.n.is_some_and(|range| (0..=3).contains(&range)) => {
            Some(8_900 + f.id.unwrap_or_default() as usize * 4 + f.n.unwrap_or_default() as usize)
        }
        _ => None,
    };
    let Some(id) = id else { return; };
    let profile = profile::active().ok();
    let snapshot = super::privacy_panel::gather(&app, profile.as_ref(), None);
    let _ = super::privacy_panel::apply(owner, &app, &snapshot, id);
    send_current(runtime);
}

fn notes_snapshot() -> Option<String> {
    let profile = profile::active().ok()?;
    let notes = notes_panel::html_notes(&profile);
    let tasks = notes_panel::html_tasks(&profile);
    protocol::snap_json("notes", |out| {
        out.array("notes", |items| for note in &notes { items.object(|item| { item.uint("id", note.id); item.string("title", &note.title); item.string("body", &note.body); }); });
        out.array("tasks", |items| for task in &tasks { items.object(|item| { item.uint("id", task.id); item.string("text", &task.text); item.bool("complete", task.complete); }); });
    })
}

fn settings_snapshot() -> Option<String> {
    let profile = profile::active().ok()?;
    let settings = crate::settings::read_strict(&profile.settings_file()).ok()?;
    let fallback = if crate::navigation::SearchProvider::from_config(None, &profile.search_provider_file()).is_default() { "default" } else { "custom" };
    protocol::snap_json("settings", |out| {
        out.string("tabLayout", match settings.tab_layout { crate::settings::TabLayout::Horizontal => "horizontal", crate::settings::TabLayout::Vertical => "vertical" }); out.uint("sidebarWidth", u64::from(settings.sidebar_width));
        out.bool("sleepEnabled", settings.sleep_enabled); out.uint("sleepAfterSecs", settings.sleep_after_secs);
        out.bool("blockerEnabled", settings.blocker_enabled); out.array("blockerAllow", |items| for site in &settings.blocker_allow { items.string(site); });
        if let Some(engine) = settings.search_engine { out.string("searchEngine", engine.key()); } else { out.null("searchEngine"); }
        out.string("searchFallback", fallback); out.bool("automaticTabGrouping", settings.automatic_tab_grouping);
    })
}

fn handle_notes(runtime: &Rc<RefCell<Runtime>>, f: PanelFields) {
    let Ok(profile) = profile::active() else { return; };
    let mut notes = notes_panel::html_notes(&profile);
    let mut tasks = notes_panel::html_tasks(&profile);
    let result = match f.a.as_str() {
        "add-note" if empty_fields(&f, false) => { let id = next_id(notes.iter().map(|n| n.id)); notes.push(crate::productivity::Note { id, title: "New note".into(), body: String::new() }); notes_panel::save_html_notes(&profile, &notes) }
        "save-title" if f.parent.is_none() && f.before.is_none() && f.n.is_none() && f.on.is_none() && f.url.is_none() && f.id.is_some() && f.text.as_ref().is_some_and(|s| s.chars().count() <= 2048) => {
            let Some(note) = notes.iter_mut().find(|item| item.id == f.id.unwrap_or_default()) else { return; }; note.title = f.text.unwrap_or_default(); notes_panel::save_html_notes(&profile, &notes)
        }
        "save-body" if f.parent.is_none() && f.before.is_none() && f.n.is_none() && f.on.is_none() && f.url.is_none() && f.id.is_some() && f.text.as_ref().is_some_and(|s| s.chars().count() <= 2048) => {
            let Some(note) = notes.iter_mut().find(|item| item.id == f.id.unwrap_or_default()) else { return; }; note.body = f.text.unwrap_or_default(); notes_panel::save_html_notes(&profile, &notes)
        }
        "delete-note" if id_only(&f) => { notes.retain(|item| item.id != f.id.unwrap_or_default()); notes_panel::save_html_notes(&profile, &notes) }
        "add-task" if empty_fields(&f, false) => { let id = next_id(tasks.iter().map(|task| task.id)); tasks.push(crate::productivity::Task { id, complete: false, text: "New task".into() }); notes_panel::save_html_tasks(&profile, &tasks) }
        "save-task" if f.parent.is_none() && f.before.is_none() && f.n.is_none() && f.on.is_none() && f.url.is_none() && f.id.is_some() && f.text.as_ref().is_some_and(|s| s.chars().count() <= 2048) => {
            let Some(task) = tasks.iter_mut().find(|item| item.id == f.id.unwrap_or_default()) else { return; }; task.text = f.text.unwrap_or_default(); notes_panel::save_html_tasks(&profile, &tasks)
        }
        "delete-task" if id_only(&f) => { tasks.retain(|item| item.id != f.id.unwrap_or_default()); notes_panel::save_html_tasks(&profile, &tasks) }
        "toggle-task" if f.parent.is_none() && f.before.is_none() && f.n.is_none() && f.text.is_none() && f.url.is_none() && f.id.is_some() && f.on.is_some() => {
            let Some(task) = tasks.iter_mut().find(|item| item.id == f.id.unwrap_or_default()) else { return; }; task.complete = f.on.unwrap_or(false); notes_panel::save_html_tasks(&profile, &tasks)
        }
        _ => return,
    };
    if let Err(error) = result { show_error_message(&format!("Notes could not be saved: {error}")); }
    send_current(runtime);
}

fn handle_settings(runtime: &Rc<RefCell<Runtime>>, f: PanelFields) {
    let (app, owner) = match runtime.try_borrow() { Ok(state) => (state.app.clone(), state.owner), Err(_) => return };
    let Ok(profile) = profile::active() else { return; };
    let result = match f.a.as_str() {
        "reset" if empty_fields(&f, false) => {
            if !crate::win32::ask_yes_no_owned(owner, "Reset all settings to their defaults? Comments and other lines in settings.txt are kept.") { return; }
            crate::settings::save(&profile.settings_file(), &crate::settings::Settings::default()).map(|()| { crate::app::App::apply_tab_layout_settings(&app, crate::settings::TabLayout::Horizontal, crate::settings::DEFAULT_SIDEBAR_WIDTH); })
        }
        "save" if f.id.is_none() && f.parent.is_none() && f.before.is_none() && f.n.is_none() && f.on.is_none() && f.url.is_none() && f.text.is_some() => {
            let Ok(mut settings) = crate::settings::read_strict(&profile.settings_file()) else { return; };
            if !apply_settings_text(&mut settings, f.text.as_deref().unwrap_or("")) { return; }
            crate::settings::save(&profile.settings_file(), &settings).map(|()| crate::app::App::apply_tab_layout_settings(&app, settings.tab_layout, settings.sidebar_width))
        }
        _ => return,
    };
    if let Err(error) = result { show_error_message(&format!("Settings could not be saved: {error}. Nothing was saved.")); }
    send_current(runtime);
}

fn apply_settings_text(settings: &mut crate::settings::Settings, text: &str) -> bool {
    if text.len() > 2048 { return false; }
    let mut seen = std::collections::HashSet::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { return false; };
        if !seen.insert(key) { return false; }
        match key {
            "tab_layout" => settings.tab_layout = match value { "horizontal" => crate::settings::TabLayout::Horizontal, "vertical" => crate::settings::TabLayout::Vertical, _ => return false },
            "sidebar_width" => { let Ok(width) = value.parse::<u16>() else { return false; }; if !(crate::settings::MIN_SIDEBAR_WIDTH..=crate::settings::MAX_SIDEBAR_WIDTH).contains(&width) { return false; } settings.sidebar_width = width; }
            "sleep_enabled" => settings.sleep_enabled = match value { "true" => true, "false" => false, _ => return false },
            "sleep_after_secs" => { let Ok(value) = crate::settings::parse_sleep_after_secs(value) else { return false; }; settings.sleep_after_secs = value; }
            "blocker_enabled" => settings.blocker_enabled = match value { "true" => true, "false" => false, _ => return false },
            "blocker_allow" => { let Ok(value) = crate::settings::parse_blocker_allow(value) else { return false; }; settings.blocker_allow = value; }
            "automatic_tab_grouping" => settings.automatic_tab_grouping = match value { "true" => true, "false" => false, _ => return false },
            "search_engine" => { let Some(value) = crate::navigation::SearchEngine::parse(value) else { return false; }; settings.search_engine = Some(value); }
            _ => return false,
        }
    }
    ["tab_layout", "sidebar_width", "sleep_enabled", "sleep_after_secs", "blocker_enabled", "blocker_allow", "automatic_tab_grouping"].iter().all(|key| seen.contains(key))
}

fn empty_fields(f: &PanelFields, allow_text: bool) -> bool { f.id.is_none() && f.parent.is_none() && f.before.is_none() && f.n.is_none() && f.on.is_none() && f.url.is_none() && (allow_text || f.text.is_none()) }
fn id_only(f: &PanelFields) -> bool { f.id.is_some() && f.parent.is_none() && f.before.is_none() && f.n.is_none() && f.on.is_none() && f.text.is_none() && f.url.is_none() }
fn next_id(existing: impl Iterator<Item = u64>) -> u64 { existing.max().unwrap_or(0).saturating_add(1).max(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_micros() as u64) }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_payload_updates_only_known_values_and_requires_every_control() {
        let mut settings = crate::settings::Settings::default();
        let valid = "tab_layout=vertical\nsidebar_width=300\nsleep_enabled=false\nsleep_after_secs=900\nblocker_enabled=true\nblocker_allow=example.com\nautomatic_tab_grouping=false";
        assert!(apply_settings_text(&mut settings, valid));
        assert_eq!(settings.tab_layout, crate::settings::TabLayout::Vertical);
        assert_eq!(settings.sidebar_width, 300);
        assert!(!settings.sleep_enabled);
        assert!(!settings.automatic_tab_grouping);
        assert!(!apply_settings_text(&mut settings, "tab_layout=wrong"));
        assert!(!apply_settings_text(&mut settings, &format!("{valid}\nunknown=x")));
        assert!(!apply_settings_text(&mut settings, &format!("{valid}\nblocker_enabled=false")));
    }
}
