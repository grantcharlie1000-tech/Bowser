//! Privacy, history and website appearance, the app's side (thread T3). Pure decisions live in
//! `blocker`, `browsing_data`, `history_retention` and `website_look`; this module only reads and
//! changes the app's own state and runs the plans those modules make.

use std::{cell::Cell, cell::RefCell, rc::Rc};

use super::{App, AppRef};
use crate::{
    blocker::{Level, registrable_domain},
    browsing_data::{self, DataKind, Plan, Progress},
    engine::BlockerHandle,
    history_retention,
    settings::Settings,
    website_look,
};

pub use crate::browsing_data::DeleteReport;

impl App {
    /// The blocker the normal tabs use (`None` for benchmark launches).
    pub fn blocker_handle(app: &AppRef) -> Option<BlockerHandle> {
        app.borrow().blocker.clone()
    }

    /// The blocker private tabs use: the normal one, or its own matcher when the normal one is off
    /// (benchmark and custom-profile launches).
    pub fn private_blocker_handle(app: &AppRef) -> Option<BlockerHandle> {
        app.borrow().private_blocker.clone()
    }

    /// Takes the privacy, history and appearance settings into the running app: the blocker's level,
    /// cookie-notice switch and exceptions, the history retention, the website look and the private
    /// theme. Called after any of them is saved. Pages already open keep what they loaded.
    pub fn apply_privacy_settings(app: &AppRef, settings: &Settings) {
        let blocker = {
            let mut state = app.borrow_mut();
            state.settings.blocker_enabled = settings.blocker_enabled;
            state.settings.blocker_level = settings.blocker_level;
            state.settings.blocker_allow = settings.blocker_allow.clone();
            state.settings.cookie_notices = settings.cookie_notices;
            state.settings.history_retention = settings.history_retention;
            state.settings.website_appearance = settings.website_appearance;
            state.settings.force_dark_websites = settings.force_dark_websites;
            let private_theme_changed = state.settings.private_theme != settings.private_theme;
            state.settings.private_theme = settings.private_theme;
            (state.blocker.clone(), private_theme_changed, state.hwnd)
        };
        let (blocker, private_theme_changed, hwnd) = blocker;
        if let Some(blocker) = blocker {
            blocker.borrow_mut().apply_settings(settings);
        }
        history_retention::set_current(settings.history_retention);
        if website_look::set(website_look::WebsiteLook::from_settings(settings)) {
            Self::call_engine(app, |engine| engine.apply_website_look());
        }
        if private_theme_changed {
            // The chrome reads the theme on its next state push.
            let active = Self::active_tab_id(app);
            Self::post_tab_changed(hwnd, active);
        }
    }

    /// The private theme setting, for the chrome while a private tab is shown.
    pub fn private_theme(app: &AppRef) -> crate::settings::PrivateTheme {
        app.borrow().settings.private_theme
    }

    /// The site the active (non-private) tab shows, as (host, registrable site key), when it is a
    /// web page.
    pub fn active_web_site(app: &AppRef) -> Option<(String, String)> {
        let (tab, url) = Self::active_site(app)?;
        if Self::tab_private(app, tab) {
            return None;
        }
        let host = url.host_str()?.to_ascii_lowercase();
        matches!(url.scheme(), "http" | "https").then(|| {
            let site = registrable_domain(&host).to_owned();
            (host, site)
        })
    }

    /// The blocking level and cookie-notice state in force for a document on `host`.
    pub fn blocker_state_for(app: &AppRef, host: &str, private: bool) -> Option<(Level, bool)> {
        let blocker = Self::blocker_handle(app)?;
        let state = blocker.borrow();
        Some((state.level_for(host, private), state.cookie_notices_for(host, private)))
    }

    /// The shield's choice of blocking level for the active tab's site (a private tab has none to
    /// choose). The page is reloaded so the choice applies to what it loads next.
    pub fn choose_site_level(app: &AppRef, level: Level) -> crate::blocker::SiteChoice {
        use crate::blocker::SiteChoice;
        let Some((host, _)) = Self::active_web_site(app) else {
            return SiteChoice::NoSite;
        };
        let Some(blocker) = Self::blocker_handle(app) else {
            return SiteChoice::NoSite;
        };
        let result = blocker.borrow_mut().choose_level_for_page(&host, level);
        if result == SiteChoice::Saved {
            Self::sync_window_title(app);
            Self::reload(app);
        }
        result
    }

    /// The shield's choice of hiding cookie notices on the active tab's site. The page is reloaded
    /// so the notice rules are injected, or left out, on the next load.
    pub fn choose_site_cookie_notices(app: &AppRef, hide: bool) -> crate::blocker::SiteChoice {
        use crate::blocker::SiteChoice;
        let Some((host, _)) = Self::active_web_site(app) else {
            return SiteChoice::NoSite;
        };
        let Some(blocker) = Self::blocker_handle(app) else {
            return SiteChoice::NoSite;
        };
        let result = blocker.borrow_mut().choose_cookie_notices_for_page(&host, hide);
        if result == SiteChoice::Saved {
            Self::reload(app);
        }
        result
    }

    /// Runs a Delete Browsing Data plan. Bowser's own stores (history, saved passwords, site
    /// choices) are handled at once; the browser engine's data is requested in one call and
    /// answers later. `done` runs once, on the UI thread, when everything has finished, with App
    /// not borrowed.
    pub fn delete_browsing_data(app: &AppRef, plan: Plan, done: Box<dyn FnOnce(DeleteReport)>) {
        let progress = Rc::new(RefCell::new(Progress::default()));
        let profile = crate::profile::active().ok();
        let now = history_retention::now_secs();

        if plan.local_history {
            match profile.as_ref().filter(|profile| !profile.is_guest()) {
                // A guest keeps no history list, so there is nothing of Bowser's to delete.
                None => progress.borrow_mut().done.push(DataKind::History),
                Some(profile) => {
                    let result = delete_own_history(&profile.history_file(), profile.root(), plan.range.seconds(), now);
                    match result {
                        Ok(()) => progress.borrow_mut().done.push(DataKind::History),
                        Err(error) => progress.borrow_mut().problems.push(format!("History could not be deleted: {error}")),
                    }
                }
            }
        }

        if plan.passwords {
            // Only the native vault, and only because the plan carries the user's confirmation.
            match crate::passwords::PasswordStore::open_default().and_then(|mut vault| vault.delete_all()) {
                Ok(_) => progress.borrow_mut().done.push(DataKind::Passwords),
                Err(crate::passwords::PasswordError::Disabled) => {
                    progress.borrow_mut().problems.push("Guest windows keep no saved passwords.".to_owned());
                }
                Err(_) => progress.borrow_mut().problems.push("Saved passwords could not be deleted. The vault could not be opened or written.".to_owned()),
            }
        }

        if plan.own_site_settings {
            let (permissions, site_prefs) = {
                let state = app.borrow();
                (state.permissions.clone(), state.site_prefs.clone())
            };
            let mut failure = None;
            if let Some(store) = permissions {
                if let Err(error) = store.borrow_mut().reset_all() {
                    failure = Some(error.to_string());
                }
            }
            if let Some(store) = site_prefs {
                if let Err(error) = store.borrow_mut().clear_all() {
                    failure = Some(error.to_string());
                }
            }
            match failure {
                None => progress.borrow_mut().done.push(DataKind::SiteSettings),
                Some(error) => progress.borrow_mut().problems.push(format!("Site settings could not be deleted: {error}")),
            }
        }

        let engine_kinds = plan.engine.clone();
        let finish = {
            let progress = progress.clone();
            let plan = plan.clone();
            move |engine_result: Option<Result<(), String>>| {
                {
                    let mut progress = progress.borrow_mut();
                    match engine_result {
                        Some(Ok(())) => {
                            for kind in &plan.engine {
                                if let Some(kind) = browsing_data::data_kind_of(*kind) {
                                    if !progress.done.contains(&kind) {
                                        progress.done.push(kind);
                                    }
                                }
                            }
                        }
                        Some(Err(error)) => progress.problems.push(format!("The browser's own data could not be deleted: {error}")),
                        None => {}
                    }
                }
                let report = browsing_data::report(&plan, &progress.borrow());
                done(report);
            }
        };

        if engine_kinds.is_empty() {
            finish(None);
            return;
        }
        let Some(tab) = Self::profile_page_tab(app) else {
            progress
                .borrow_mut()
                .problems
                .push("Open a web page first: Bowser needs one to reach the browser's stored data.".to_owned());
            finish(None);
            return;
        };
        // The engine may answer inside the call, while App is borrowed; that answer waits until the
        // call returns, so `done` never runs with App borrowed.
        let finish = Rc::new(RefCell::new(Some(finish)));
        let in_call = Rc::new(Cell::new(true));
        let early: Rc<RefCell<Option<Result<(), String>>>> = Rc::new(RefCell::new(None));
        let answer = {
            let (finish, in_call, early) = (finish.clone(), in_call.clone(), early.clone());
            Box::new(move |result: Result<(), String>| {
                if in_call.get() {
                    *early.borrow_mut() = Some(result);
                } else if let Some(finish) = finish.borrow_mut().take() {
                    finish(Some(result));
                }
            })
        };
        let started = Self::call_engine(app, |engine| engine.clear_browsing_data_set(tab, &engine_kinds, plan.range.engine(), answer));
        in_call.set(false);
        if started.is_none() {
            if let Some(finish) = finish.borrow_mut().take() {
                progress.borrow_mut().problems.push("The browser engine is not ready.".to_owned());
                finish(None);
            }
            return;
        }
        let waiting = early.borrow_mut().take();
        if let Some(result) = waiting {
            if let Some(finish) = finish.borrow_mut().take() {
                finish(Some(result));
            }
        }
    }

    /// Requests the blocker stopped in all this session, for the Privacy section.
    pub fn session_blocked(app: &AppRef) -> u64 {
        app.borrow().privacy_session_blocked
    }
}

/// Deletes Bowser's own history for Delete Browsing Data: the visits in the range (`None`: all of
/// them) and the address bar's typed history (T2's `typed-history.txt`, which keeps no times, so any
/// range forgets it all rather than leave typed text behind).
fn delete_own_history(history: &std::path::Path, root: &std::path::Path, seconds: Option<u64>, now: u64) -> std::io::Result<()> {
    match seconds {
        None => crate::productivity::clear_history(history)?,
        Some(seconds) => {
            crate::productivity::delete_history_range(history, now.saturating_sub(seconds), u64::MAX)?;
        }
    }
    crate::address_runtime::clear_typed_in(root)
}

#[cfg(test)]
mod tests {
    use crate::address_suggest::TypedHistory;

    #[test]
    fn deleting_history_also_forgets_what_was_typed_in_the_address_bar() {
        for seconds in [None, Some(3600)] {
            let root = std::env::temp_dir().join(format!("bowser-delete-typed-{}-{}", std::process::id(), seconds.is_some()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            let mut typed = TypedHistory::default();
            typed.record("secret search", true);
            typed.record("example.com/private", false);
            typed.save(&root).unwrap();
            assert_eq!(TypedHistory::load(&root).len(), 2);
            let history = root.join("history.tsv");
            std::fs::write(&history, b"").unwrap();
            super::delete_own_history(&history, &root, seconds, 10_000).unwrap();
            assert!(TypedHistory::load(&root).is_empty(), "range {seconds:?}: the typed list is empty");
            let _ = std::fs::remove_dir_all(&root);
        }
    }
}
