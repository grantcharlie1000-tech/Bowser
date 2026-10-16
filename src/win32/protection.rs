//! The Privacy section's blocking, cookie-notice, history and website-appearance controls, the
//! window side (thread T3): reads the app into a `protection` snapshot and runs the commands the
//! page sends. The wire and every check are in `chrome_host::panels::protection`; the rules are in
//! `blocker`, `browsing_data`, `history_retention` and `website_look`.
//!
//! Runs on the UI thread. The one background job is fetching a list the user typed an address for:
//! its worker only calls `blocker::fetch` and leaves the answer in [`FETCHED`] for the UI thread to
//! take (`apply_fetched`), so no shared state is touched off the UI thread.

use std::{
    cell::{Cell, RefCell},
    sync::Mutex,
};

use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::IsWindow,
};

use crate::{
    app::{App, AppRef},
    blocker::{
        Level,
        fetch,
        sources::{self, Origin, Registry, SourceKind},
    },
    browsing_data,
    chrome_host::panels::protection::{Action, CommandResult, CurrentSite, SourceRow, View, apply_to_settings},
    settings::{self, Settings},
};

thread_local! {
    /// How the last command went, kept until the next one so a refreshed snapshot still shows it.
    static RESULT: RefCell<Option<CommandResult>> = const { RefCell::new(None) };
    static SEQ: Cell<u64> = const { Cell::new(0) };
    /// A list fetch is running.
    static BUSY: Cell<bool> = const { Cell::new(false) };
}

/// What a fetch worker found, waiting for the UI thread.
struct Fetched {
    owner: isize,
    kind: SourceKind,
    url: String,
    outcome: Result<String, String>,
}

static FETCHED: Mutex<Vec<Fetched>> = Mutex::new(Vec::new());

fn set_result(ok: bool, text: impl Into<String>) {
    let seq = SEQ.with(|seq| {
        seq.set(seq.get() + 1);
        seq.get()
    });
    RESULT.with(|slot| *slot.borrow_mut() = Some(CommandResult { seq, ok, text: text.into() }));
}

/// Reads the app and the profile into the snapshot's data.
pub(crate) fn gather(app: &AppRef) -> View {
    let profile = crate::profile::active().ok();
    let read = profile.as_ref().map(|profile| settings::read_strict(&profile.settings_file()));
    let readable = matches!(read, Some(Ok(_)));
    let current_settings = match read {
        Some(Ok(settings)) => settings,
        _ => Settings::default(),
    };
    let blocker = App::blocker_handle(app);
    let current = App::active_web_site(app).and_then(|(host, _)| {
        let (level, cookies) = App::blocker_state_for(app, &host, false)?;
        Some(CurrentSite { host, level, cookies, private: false })
    });
    let (sites, sources) = match (&blocker, &profile) {
        (Some(blocker), Some(profile)) => {
            let state = blocker.borrow();
            let rows = Registry::load(&profile.blocker_dir())
                .sources()
                .iter()
                .map(|source| SourceRow {
                    id: source.id.clone(),
                    name: source.name.clone(),
                    kind: source.kind,
                    enabled: source.enabled,
                    origin: source.origin.key(),
                    url: match &source.origin {
                        Origin::Url(url) => url.clone(),
                        _ => String::new(),
                    },
                    rules: state.source_rule_count(&source.id),
                    removable: source.removable(),
                })
                .collect();
            (state.site_rules(), rows)
        }
        _ => (Vec::new(), Vec::new()),
    };
    View {
        level: Level::from_settings(current_settings.blocker_enabled, current_settings.blocker_level),
        cookie_notices: current_settings.cookie_notices,
        retention: current_settings.history_retention,
        website_appearance: current_settings.website_appearance,
        force_dark: current_settings.force_dark_websites,
        private_theme: current_settings.private_theme,
        readable,
        guest: profile.as_ref().is_some_and(crate::profile::Profile::is_guest),
        current,
        sites,
        allow: current_settings.blocker_allow.clone(),
        sources,
        blocked_page: App::active_blocked_count(app),
        blocked_session: App::session_blocked(app),
        result: RESULT.with(|slot| slot.borrow().clone()),
        busy: BUSY.with(Cell::get),
    }
}

/// Reads `settings.txt`, applies `change`, saves it and tells the running app.
fn update_settings(app: &AppRef, change: impl FnOnce(&mut Settings)) -> Result<(), String> {
    let profile = crate::profile::active().map_err(|_| "The profile is not ready.".to_owned())?;
    let path = profile.settings_file();
    let mut current = settings::read_strict(&path)
        .map_err(|_| "settings.txt could not be read, so nothing was saved. Open the settings file to fix it.".to_owned())?;
    change(&mut current);
    settings::save(&path, &current).map_err(|error| format!("Settings could not be saved: {error}. Nothing was saved."))?;
    App::apply_privacy_settings(app, &current);
    Ok(())
}

fn answer(result: Result<(), String>) {
    if let Err(error) = result {
        set_result(false, error);
    }
}

/// Runs one validated command.
pub(crate) fn run(app: &AppRef, owner: HWND, action: Action) {
    let profile = crate::profile::active().ok();
    let dir = profile.as_ref().map(|profile| profile.blocker_dir());
    // The commands that are saved settings: the level, the cookie switch, the retention, the
    // website look and the private theme.
    let mut scratch = Settings::default();
    if apply_to_settings(&action, &mut scratch) {
        let result = update_settings(app, |settings| {
            apply_to_settings(&action, settings);
        });
        if result.is_ok() && matches!(action, Action::Retention(_)) {
            // A shorter period takes effect now, off the UI thread.
            if let Some(profile) = profile.filter(|profile| !profile.is_guest()) {
                let path = profile.history_file();
                let _ = std::thread::Builder::new().name("history-retention".into()).spawn(move || {
                    let _ = crate::history_retention::prune_now(&path);
                });
            }
        }
        return answer(result);
    }
    match action {
        // Handled above: they are saved settings.
        Action::Level(_)
        | Action::CookieNotices(_)
        | Action::Retention(_)
        | Action::WebsiteAppearance(_)
        | Action::ForceDark(_)
        | Action::PrivateTheme(_) => {}
        Action::SiteLevel { site, level } => {
            let Some(blocker) = App::blocker_handle(app) else { return };
            if !blocker.borrow_mut().set_site_level(&site, level) {
                set_result(false, "That site could not be saved. The list may be full, or it is not a site.");
            }
        }
        Action::SiteCookies { site, hide } => {
            let Some(blocker) = App::blocker_handle(app) else { return };
            if !blocker.borrow_mut().set_site_cookies(&site, hide) {
                set_result(false, "That site could not be saved. The list may be full, or it is not a site.");
            }
        }
        Action::SiteRemove(site) => {
            if let Some(blocker) = App::blocker_handle(app) {
                blocker.borrow_mut().remove_site_rule(&site);
            }
        }
        Action::SourceToggle { id, on } => {
            let Some(dir) = dir else { return };
            let mut registry = Registry::load(&dir);
            if !registry.set_enabled(&id, on) || registry.save(&dir).is_err() {
                return set_result(false, "That list could not be changed.");
            }
            rebuild(app, &dir);
        }
        Action::SourceRemove(id) => {
            let Some(dir) = dir else { return };
            match Registry::load(&dir).remove(&dir, &id) {
                Ok(true) => rebuild(app, &dir),
                Ok(false) => set_result(false, "Bowser's own lists cannot be removed. Switch one off instead."),
                Err(error) => set_result(false, format!("That list could not be removed: {error}")),
            }
        }
        Action::SourceAddFile { kind } => {
            let Some(dir) = dir else { return };
            let Some(path) = pick_list_file(owner) else { return };
            add_from_file(app, &dir, kind, &path);
        }
        Action::SourceAddUrl { url, kind } => {
            let parsed = match fetch::parse_https_url(&url) {
                Ok(parsed) => parsed,
                Err(error) => return set_result(false, error.to_string()),
            };
            start_fetch(owner, kind, parsed.to_string());
        }
        Action::SourceUpdate(id) => {
            let Some(dir) = dir else { return };
            let registry = Registry::load(&dir);
            match registry.get(&id) {
                Some(source) => match &source.origin {
                    Origin::Url(url) => start_fetch(owner, source.kind, url.clone()),
                    _ => set_result(false, "Only a list added from an address can be updated."),
                },
                None => set_result(false, "That list is not there any more."),
            }
        }
        Action::DeleteData { selection, range, logins_confirmed } => {
            let plan = match browsing_data::plan(selection, range, logins_confirmed) {
                Ok(plan) => plan,
                Err(error) => return set_result(false, error.to_string()),
            };
            // Saved passwords are the one kind that is also asked about natively. The page's
            // confirmation is only a flag it sends; this is a question that only the user at the
            // keyboard can answer, so a page cannot delete the vault on its own say-so.
            if plan.passwords
                && !super::ask_yes_no("Delete every saved password in this profile? Passwords have no dates, so all of them go, whatever period you chose. This cannot be undone.")
            {
                return set_result(false, "Nothing was deleted. Your saved passwords were kept.");
            }
            let raw = owner.0 as isize;
            App::delete_browsing_data(
                app,
                plan,
                Box::new(move |report| {
                    set_result(report.ok, report.text);
                    let owner = HWND(raw as *mut _);
                    if unsafe { IsWindow(Some(owner)) }.as_bool() {
                        super::settings_page::refresh(owner);
                    }
                }),
            );
        }
    }
}

/// Rebuilds the blocker's rules from the sources on disk.
fn rebuild(app: &AppRef, dir: &std::path::Path) {
    if let Some(blocker) = App::blocker_handle(app) {
        blocker.borrow_mut().reload_sources(dir);
    }
}

/// Adds the list in a local file the user picked. Reading is bounded and the text is checked like
/// a fetched one.
fn add_from_file(app: &AppRef, dir: &std::path::Path, kind: SourceKind, path: &std::path::Path) {
    use std::io::Read;
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let mut bytes = Vec::new();
    let read = std::fs::File::open(path).and_then(|file| file.take(fetch::MAX_LIST_BYTES as u64 + 1).read_to_end(&mut bytes));
    if read.is_err() {
        return set_result(false, "That file could not be read.");
    }
    let text = match fetch::validate_list(&bytes, kind == SourceKind::Cookies) {
        Ok(text) => text,
        Err(error) => return set_result(false, error.to_string()),
    };
    commit(app, dir, kind, Origin::File, &name, &text);
}

/// Stores a validated list and rebuilds, then says how many rules it holds.
fn commit(app: &AppRef, dir: &std::path::Path, kind: SourceKind, origin: Origin, name: &str, text: &str) {
    let mut registry = Registry::load(dir);
    match registry.add(dir, name, kind, origin, text) {
        Ok(id) => {
            rebuild(app, dir);
            let count = App::blocker_handle(app).map_or(0, |blocker| blocker.borrow().source_rule_count(&id));
            set_result(true, format!("Added {}: {count} rules.", sources::clean_name(name)));
        }
        Err(error) => set_result(false, error.to_string()),
    }
}

/// Fetches a list on a worker thread; `apply_fetched` takes the answer on the UI thread.
fn start_fetch(owner: HWND, kind: SourceKind, url: String) {
    if BUSY.with(Cell::get) {
        return set_result(false, "A list is still being fetched. Wait for it to finish.");
    }
    BUSY.with(|busy| busy.set(true));
    let raw = owner.0 as isize;
    let worker_url = url.clone();
    let spawned = std::thread::Builder::new().name("bowser-list-fetch".into()).spawn(move || {
        let outcome = fetch::parse_https_url(&worker_url)
            .and_then(|parsed| fetch::fetch_list(&parsed))
            .map_err(|error| error.to_string())
            .and_then(|bytes| fetch::validate_list(&bytes, kind == SourceKind::Cookies).map_err(|error| error.to_string()));
        FETCHED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(Fetched { owner: raw, kind, url: worker_url, outcome });
        super::post(HWND(raw as *mut _), super::WM_SETTINGS_COMMAND, WPARAM(0), LPARAM(0));
    });
    if spawned.is_err() {
        BUSY.with(|busy| busy.set(false));
        set_result(false, "The list could not be fetched.");
    }
}

/// Takes the answers fetch workers have left and stores them. Called from `WM_SETTINGS_COMMAND`
/// before the page's own commands. Returns whether anything was taken.
pub(crate) fn apply_fetched(app: &AppRef, owner: HWND) -> bool {
    let taken = {
        let mut waiting = FETCHED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mine, rest): (Vec<_>, Vec<_>) = waiting.drain(..).partition(|fetched| fetched.owner == owner.0 as isize);
        *waiting = rest;
        mine
    };
    let any = !taken.is_empty();
    for fetched in taken {
        BUSY.with(|busy| busy.set(false));
        let Some(dir) = crate::profile::active().ok().map(|profile| profile.blocker_dir()) else {
            set_result(false, "The profile is not ready.");
            continue;
        };
        match fetched.outcome {
            Ok(text) => {
                let name = url::Url::parse(&fetched.url)
                    .ok()
                    .map(|url| url.host_str().unwrap_or("list").to_owned())
                    .unwrap_or_else(|| "list".to_owned());
                commit(app, &dir, fetched.kind, Origin::Url(fetched.url), &name, &text);
            }
            Err(error) => set_result(false, error),
        }
    }
    any
}

/// File picker for one filter list on this computer. `None` when cancelled or when the dialog fails.
fn pick_list_file(owner: HWND) -> Option<std::path::PathBuf> {
    use windows::{
        Win32::{
            System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree},
            UI::Shell::{
                Common::COMDLG_FILTERSPEC, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FileOpenDialog, IFileOpenDialog,
                SIGDN_FILESYSPATH,
            },
        },
        core::w,
    };
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        dialog.SetOptions(FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST).ok()?;
        dialog.SetTitle(w!("Choose a filter list on this computer")).ok()?;
        let filters = [
            COMDLG_FILTERSPEC { pszName: w!("Filter lists (*.txt;*.list)"), pszSpec: w!("*.txt;*.list") },
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
