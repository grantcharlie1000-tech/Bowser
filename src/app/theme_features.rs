//! What the theme options do to the running app (thread T8): the person's colours in every palette,
//! the accent taken from the page, the light/dark schedule, and the theme file's import and export.
//! The pure rules are in `crate::theme` (colour arithmetic and its contrast guarantees) and
//! `crate::theme_custom` (options, schedule, file checks); this file applies them to `App`.

use std::{cell::RefCell, collections::HashMap};

use super::*;
use crate::{
    settings::Settings,
    theme::{self, Appearance, Palette, Rgba, UserTheme},
    theme_custom,
};

/// What the active page told the chrome about its own colour, per tab. The meta tag wins over the
/// favicon's colour. UI thread only, like the rest of `App`.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct PageAccent {
    pub(super) meta: Option<Rgba>,
    pub(super) icon: Option<Rgba>,
}

impl PageAccent {
    fn colour(self) -> Option<Rgba> {
        self.meta.or(self.icon)
    }
}

thread_local! {
    static PAGE_ACCENTS: RefCell<HashMap<u32, PageAccent>> = RefCell::new(HashMap::new());
}

/// The palette family to install and the schedule to follow, before the window exists: the first
/// paint already has the person's colours and the look the schedule asks for.
pub(super) fn install_launch_theme(settings: &Settings) {
    theme::install_user_theme(theme_custom::user_theme(settings, theme::system_accent_colour(), None));
    win32::set_scheduled_look(settings.themes.scheduled_look(win32::local_minutes()));
}

impl App {
    /// The theme the chrome is drawn with now: the settings' colours with the accent resolved, and
    /// the active page's colour in it while "Get accent colour from the webpage" is on.
    pub fn user_theme(app: &AppRef) -> UserTheme {
        let state = app.borrow();
        let page = if state.settings.themes.accent_from_page {
            let active = state.tab_model.active().0;
            PAGE_ACCENTS.with(|accents| accents.borrow().get(&active).copied()).and_then(PageAccent::colour)
        } else {
            None
        };
        theme_custom::user_theme(&state.settings, theme::system_accent_colour(), page)
    }

    /// The palette an HTML host takes for `look` and the backdrop: Bowser's own with the person's
    /// colours, accent and bar tint in it. The look is the one asked for (a private tab's own look
    /// keeps its family; the background colour applies to its own family only).
    pub fn themed_palette(app: &AppRef, look: Appearance, solid: bool) -> (Palette, Appearance) {
        (theme::themed_for(look, solid, &Self::user_theme(app)), look)
    }

    /// Puts the colours the settings choose (without any page's accent) into every palette read, so
    /// native menus, dialogs, panels and the GDI chrome follow them. Returns whether they changed.
    pub fn refresh_theme_install(app: &AppRef) -> bool {
        let user = {
            let state = app.borrow();
            theme_custom::user_theme(&state.settings, theme::system_accent_colour(), None)
        };
        theme::install_user_theme(user)
    }

    /// Takes the theme options into the running app: the installed palettes, the look the schedule
    /// asks for and its timer, and a redraw of the chrome. Called after any theme setting changes.
    pub fn apply_theme_options(app: &AppRef) {
        let palette_changed = Self::refresh_theme_install(app);
        let (options, hwnd) = {
            let state = app.borrow();
            (state.settings.themes.clone(), state.hwnd)
        };
        let look_changed = win32::set_scheduled_look(options.scheduled_look(win32::local_minutes()));
        win32::set_theme_timer(hwnd, options.schedule == theme_custom::Schedule::Time);
        if palette_changed || look_changed {
            win32::refresh_theme(hwnd);
        }
        let active = Self::active_tab_id(app);
        Self::post_tab_changed(hwnd, active);
    }

    /// The window exists: start the schedule's timer if the theme follows the time of day.
    pub fn theme_window_created(app: &AppRef, hwnd: HWND) {
        let time = app.borrow().settings.themes.schedule == theme_custom::Schedule::Time;
        win32::set_theme_timer(hwnd, time);
    }

    /// The schedule's timer fired: switch between light and dark when the time says so.
    pub fn theme_tick(app: &AppRef) {
        let (options, hwnd) = {
            let state = app.borrow();
            (state.settings.themes.clone(), state.hwnd)
        };
        if win32::set_scheduled_look(options.scheduled_look(win32::local_minutes())) {
            win32::refresh_theme(hwnd);
            let active = Self::active_tab_id(app);
            Self::post_tab_changed(hwnd, active);
        }
    }

    /// Settings > Appearance: one `key=value` change of the theme options, kept, saved and put to
    /// work at once. False (and nothing changed) when the key or value is not one the page may send.
    pub fn set_theme_option(app: &AppRef, text: &str) -> bool {
        let applied = theme_custom::apply_assignment(&mut app.borrow_mut().settings, text);
        if applied {
            Self::save_settings(app);
            Self::apply_theme_options(app);
        }
        applied
    }

    /// "Reset theme settings": the theme, the accent and every theme option back to the defaults.
    pub fn reset_theme(app: &AppRef) {
        theme_custom::reset(&mut app.borrow_mut().settings);
        Self::save_settings(app);
        Self::apply_theme_options(app);
    }

    /// The text of the theme file for the running settings.
    pub fn export_theme_text(app: &AppRef) -> String {
        theme_custom::export_text(&app.borrow().settings)
    }

    /// Imports a theme file read from disk: checked whole, then applied whole. The error is what the
    /// page shows; nothing changes when there is one.
    pub fn import_theme(app: &AppRef, bytes: &[u8]) -> std::result::Result<(), String> {
        let file = theme_custom::parse_theme_file(bytes).map_err(|error| error.to_string())?;
        theme_custom::apply_theme_file(&mut app.borrow_mut().settings, &file);
        Self::save_settings(app);
        Self::apply_theme_options(app);
        Ok(())
    }

    // ---- the accent from the page ----

    /// A page finished loading in `tab`: when the accent follows the page, reads its `theme-color`
    /// (one attribute, through the page's own canvas; no request, nothing written to the page).
    pub(super) fn sample_page_accent(app: &AppRef, tab: TabId) {
        if !app.borrow().settings.themes.accent_from_page {
            return;
        }
        let weak = Rc::downgrade(app);
        Self::run_reader_script(
            app,
            tab,
            theme_custom::PAGE_COLOUR_SCRIPT.to_owned(),
            Box::new(move |answer| {
                let Some(app) = weak.upgrade() else { return };
                let meta = answer.as_deref().and_then(theme_custom::parse_page_colour);
                Self::set_page_accent(&app, tab, |accent| accent.meta = meta);
            }),
        );
    }

    /// A tab's favicon changed: when the accent follows the page, its colour is the fallback for a
    /// page without a `theme-color`. The favicon is already in memory; nothing is fetched.
    pub(super) fn note_favicon_colour(app: &AppRef, tab: TabId) {
        if !app.borrow().settings.themes.accent_from_page {
            return;
        }
        let icon = Self::tab_favicon(app, tab)
            .and_then(|bytes| crate::favicon::decode(&bytes, 16))
            .and_then(|bitmap| theme::dominant_colour(&bitmap.bgra_premultiplied));
        Self::set_page_accent(app, tab, |accent| accent.icon = icon);
    }

    pub(super) fn set_page_accent(app: &AppRef, tab: TabId, change: impl FnOnce(&mut PageAccent)) {
        let (before, after, active, hwnd) = {
            let state = app.borrow();
            let live: Vec<u32> = state.tabs.iter().map(|tab| tab.id.0).collect();
            let (before, after) = PAGE_ACCENTS.with(|accents| {
                let mut accents = accents.borrow_mut();
                accents.retain(|id, _| live.contains(id));
                let entry = accents.entry(tab.0).or_default();
                let before = entry.colour();
                change(entry);
                (before, entry.colour())
            });
            (before, after, state.tab_model.active() == tab, state.hwnd)
        };
        if before != after && active {
            Self::post_tab_changed(hwnd, tab);
        }
    }

    /// The colour the active tab's page gave the chrome, for tests and the settings page.
    #[cfg(test)]
    pub(crate) fn page_accent(tab: TabId) -> Option<Rgba> {
        PAGE_ACCENTS.with(|accents| accents.borrow().get(&tab.0).copied()).and_then(PageAccent::colour)
    }
}
