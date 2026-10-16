//! What the Tabs options, closed-tab history, periodic reload and tab selection do to the running
//! app. The pure rules are in `crate::tab_options` and `crate::tab_state`; this file applies them to
//! `App`, so `app.rs` itself only calls in from the places a tab is made, shown, titled or closed.

use super::*;
use crate::tab_options::{self, CloseKind, MuteMode, NewTabTarget, PinnedClose};
use crate::tab_state::{unix_now, PeriodicReload, TabExtra};
use crate::tabs::Placement;

/// The placement the "New tab position" settings add up to.
pub(super) fn placement(settings: &crate::settings::Settings) -> Placement {
    match tab_options::new_tab_position(settings.new_tab_beside_active, settings.tabs.new_tab_related) {
        tab_options::NewTabPosition::Last => Placement::Last,
        tab_options::NewTabPosition::Active => Placement::AfterActive,
        tab_options::NewTabPosition::Related => Placement::AfterRelated,
    }
}

/// Puts a tab the model just placed at the same place in the active workspace, whose order is the
/// strip's order (`Workspace::add_tab` appends). The tab goes right of the nearest earlier tab of
/// the model that is in this workspace.
pub(super) fn place_in_workspace(state: &mut App, id: TabId) {
    let Some(model_index) = state.tab_model.index_of(id) else {
        return;
    };
    let order: Vec<TabId> = state.workspaces.active().tabs().to_vec();
    let previous = state
        .tab_model
        .iter()
        .take(model_index)
        .map(|tab| tab.id)
        .rev()
        .find(|tab| *tab != id && order.contains(tab));
    let target = previous
        .and_then(|previous| order.iter().position(|tab| *tab == previous))
        .map_or(0, |position| position + 1);
    let _ = state.workspaces.move_tab(id, target);
}

/// The tab a close of `closed` should show, from the close options. `order` is the active workspace
/// before the close. None leaves the choice to the model.
pub(super) fn close_target_for(
    state: &App,
    order: &[TabId],
    closed: TabId,
    opener: Option<TabId>,
) -> Option<TabId> {
    tab_options::close_target(&state.settings.tabs, order, closed, opener, &state.tab_mru)
        .filter(|target| *target != closed && state.tabs.iter().any(|tab| tab.id == *target))
}

/// Reads a freshly arrived title: the count it announces and whether that makes the tab unread.
pub(super) fn note_title(tab: &mut AppTab, title: &str, detect: bool, active: bool) {
    let notify = if detect { tab_options::title_notification(title).unwrap_or(0) } else { 0 };
    tab.extra.title_arrived(notify, !active);
}

/// One tab as the strip draws it beyond the model's own fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabStripExtra {
    pub id: TabId,
    pub unread: bool,
    pub notify: u32,
    /// Hibernated or discarded: the page is not in memory.
    pub asleep: bool,
    pub progress: Option<u8>,
    /// The reload interval and when it next fires.
    pub reload: Option<(u32, Instant)>,
    /// Not used for longer than the inactive threshold.
    pub idle: bool,
}

impl App {
    /// The profile's tab options as they are in force now.
    pub fn tab_options(app: &AppRef) -> tab_options::TabOptions {
        app.borrow().settings.tabs.clone()
    }

    /// Settings > Tabs: one `key=value` change, kept, saved and put to work at once. False (and
    /// nothing changed) when the key or value is not one the page may send.
    pub fn set_tab_option(app: &AppRef, text: &str) -> bool {
        let before = app.borrow().settings.tabs.clone();
        if !tab_options::apply_assignment(&mut app.borrow_mut().settings, text) {
            return false;
        }
        Self::save_settings(app);
        Self::tab_options_changed(app, &before);
        true
    }

    /// What changing the options must redo: the audio rule, the tick that serves it, and the strip,
    /// which draws from the options.
    fn tab_options_changed(app: &AppRef, before: &tab_options::TabOptions) {
        let (hwnd, active, mute_changed) = {
            let state = app.borrow();
            (state.hwnd, state.tab_model.active(), state.settings.tabs.mute_mode != before.mute_mode)
        };
        if mute_changed {
            Self::apply_mute_mode(app);
        }
        Self::sync_tick(app);
        Self::post_tab_changed(hwnd, active);
    }

    // ---- new tabs ----

    /// A new tab from the user's hand (Ctrl+T, the new tab button): the new tab page decides what
    /// it opens.
    pub fn open_new_tab(app: &AppRef) -> TabId {
        let id = Self::new_tab(app);
        let target = tab_options::new_tab_address(&app.borrow().settings.tabs);
        if let NewTabTarget::Page(url) = target {
            Self::submit_url(app, url, None);
        }
        id
    }

    /// A tab a page asked for (a link opened in a new tab, `window.open`). It goes where the
    /// position option says and either takes the view or loads in the background.
    pub(super) fn open_link_tab(app: &AppRef, source: TabId, private: bool, url: url::Url) {
        let background = app.borrow().settings.tabs.link_background;
        if background {
            let id = Self::new_background_tab(app, private, Some(source));
            Self::navigate_tab(app, id, &url);
        } else {
            let child = Self::spawn_tab(app, private, Some(source));
            let _ = app.borrow_mut().tab_model.set_opener(child, source);
            Self::submit_url(app, url, None);
        }
    }

    /// An empty tab placed by the position option that does not take the view: the active tab, its
    /// page and the address bar stay as they are.
    pub(crate) fn new_background_tab(app: &AppRef, private: bool, opener: Option<TabId>) -> TabId {
        let (id, hwnd) = {
            let mut state = app.borrow_mut();
            let placement = placement(&state.settings);
            let id = state.tab_model.add_placed(placement, opener);
            let _ = state.workspaces.add_tab(id);
            place_in_workspace(&mut state, id);
            let mut fresh = AppTab::empty(id);
            fresh.private = private;
            fresh.hidden_since = Some(Instant::now());
            let at = state.tab_model.index_of(id).map_or(state.tabs.len(), |at| at.min(state.tabs.len()));
            state.tabs.insert(at, fresh);
            (id, state.hwnd)
        };
        if private {
            Self::with_engine(app, |engine| {
                engine.set_tab_private(id);
                Ok(())
            });
        }
        Self::apply_mute_mode(app);
        Self::post_tab_changed(hwnd, id);
        Self::save_workspaces(app, WorkspaceChange::TabOpen);
        id
    }

    /// Called when a tab has just been shown (selected, or made on the spot): its unread mark goes,
    /// the time of use is noted and the audio rule follows the new active tab.
    pub(super) fn on_tab_shown(app: &AppRef, previous: TabId, id: TabId) {
        let now = unix_now();
        let need_mute = {
            let mut state = app.borrow_mut();
            for tab in &mut state.tabs {
                if tab.id == id {
                    tab.extra.seen();
                    tab.extra.hibernated = false;
                    tab.extra.last_used = now;
                } else if tab.id == previous {
                    tab.extra.last_used = now;
                }
            }
            state.settings.tabs.mute_mode != MuteMode::All || state.tabs.iter().any(|tab| tab.extra.auto_muted)
        };
        if need_mute {
            Self::apply_mute_mode(app);
        }
        Self::sync_tick(app);
        Self::schedule_thumbnail(app);
    }

    /// Called after a close: the audio rule and the tick may have changed.
    pub(super) fn on_tab_closed(app: &AppRef, _active: TabId, _was_active: bool) {
        Self::apply_mute_mode(app);
        Self::sync_tick(app);
    }

    // ---- audio ----

    /// "Mute tab audio": applies the mode to every tab. A tab the rule mutes stays muted by the rule
    /// alone: the user's own mute is separate and survives it.
    pub(super) fn apply_mute_mode(app: &AppRef) {
        let changes: Vec<(TabId, bool)> = {
            let state = app.borrow();
            let mode = state.settings.tabs.mute_mode;
            let active = state.tab_model.active();
            let active_audible = mode == MuteMode::Prioritise
                && state.engine.as_ref().is_some_and(|engine| engine.tab_playing_audio(active));
            state
                .tabs
                .iter()
                .filter_map(|tab| {
                    let visible = tab.id == active
                        || state.split_view.is_some_and(|split| split.pane_for(tab.id).is_some());
                    let want = !tab_options::tab_may_play(mode, visible, active_audible);
                    (tab.extra.auto_muted != want).then_some((tab.id, want))
                })
                .collect()
        };
        for (id, want) in changes {
            if let Some(tab) = app.borrow_mut().tabs.iter_mut().find(|tab| tab.id == id) {
                tab.extra.auto_muted = want;
            }
            Self::push_effective_mute(app, id);
        }
    }

    /// Tells the engine what the tab's sound should be: muted by the user or by the audio rule.
    pub(super) fn push_effective_mute(app: &AppRef, id: TabId) {
        let effective = app
            .borrow()
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .is_some_and(|tab| tab.muted || tab.extra.auto_muted);
        // A tab with no page yet has nothing to mute; its controller applies this when it exists.
        let _ = Self::with_engine(app, |engine| engine.set_tab_muted(id, effective));
    }

    /// Whether the rule has muted the tab (not the user).
    pub fn tab_auto_muted(app: &AppRef, id: TabId) -> bool {
        app.borrow().tabs.iter().find(|tab| tab.id == id).is_some_and(|tab| tab.extra.auto_muted)
    }

    // ---- periodic reload ----

    /// Reload this tab every `secs` seconds, or stop with `None`. False for a tab that is not open
    /// or an interval outside the allowed range.
    pub fn set_tab_reload(app: &AppRef, id: TabId, secs: Option<u32>) -> bool {
        let reload = match secs {
            Some(secs) => match PeriodicReload::new(secs, Instant::now()) {
                Some(reload) => Some(reload),
                None => return false,
            },
            None => None,
        };
        let hwnd = {
            let mut state = app.borrow_mut();
            let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == id) else {
                return false;
            };
            tab.extra.reload = reload;
            state.hwnd
        };
        Self::sync_tick(app);
        Self::post_tab_changed(hwnd, id);
        true
    }

    /// The tab's reload interval in seconds, when it has one.
    pub fn tab_reload_secs(app: &AppRef, id: TabId) -> Option<u32> {
        app.borrow().tabs.iter().find(|tab| tab.id == id).and_then(|tab| tab.extra.reload).map(|reload| reload.secs())
    }

    /// Stops every periodic reload. Returns how many were running.
    pub fn disable_all_reloads(app: &AppRef) -> usize {
        let (count, hwnd, active) = {
            let mut state = app.borrow_mut();
            let mut count = 0;
            for tab in &mut state.tabs {
                if tab.extra.reload.take().is_some() {
                    count += 1;
                }
            }
            (count, state.hwnd, state.tab_model.active())
        };
        if count > 0 {
            Self::sync_tick(app);
            Self::post_tab_changed(hwnd, active);
        }
        count
    }

    /// How many tabs have a periodic reload running.
    pub fn reloading_tab_count(app: &AppRef) -> usize {
        app.borrow().tabs.iter().filter(|tab| tab.extra.reload.is_some()).count()
    }

    /// The one-second tick: reloads the tabs that are due and re-checks the audio rule.
    pub fn tab_tick(app: &AppRef) {
        Self::tab_tick_at(app, Instant::now());
    }

    pub(crate) fn tab_tick_at(app: &AppRef, now: Instant) {
        let due: Vec<(TabId, bool)> = app
            .borrow()
            .tabs
            .iter()
            .filter(|tab| tab.extra.reload.is_some_and(|reload| reload.due(now)))
            .map(|tab| {
                let reloadable = tab.controller_created
                    && !tab.discarded
                    && tab.url.as_ref().is_some_and(|url| matches!(url.scheme(), "http" | "https"));
                (tab.id, reloadable)
            })
            .collect();
        for (id, reloadable) in due {
            let hwnd = {
                let mut state = app.borrow_mut();
                if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == id) {
                    tab.extra.reload = tab.extra.reload.map(|reload| reload.fired(now));
                }
                state.hwnd
            };
            if reloadable {
                Self::reload_tab(app, id);
            }
            Self::post_tab_changed(hwnd, id);
        }
        if app.borrow().settings.tabs.mute_mode == MuteMode::Prioritise {
            Self::apply_mute_mode(app);
        }
        Self::sync_tick(app);
    }

    /// Whether something needs the one-second tick right now.
    pub(crate) fn tick_wanted(&self) -> bool {
        self.settings.tabs.mute_mode == MuteMode::Prioritise
            || self.tabs.iter().any(|tab| tab.extra.reload.is_some())
    }

    /// Starts or stops the window's one-second timer to match [`Self::tick_wanted`].
    pub(crate) fn sync_tick(app: &AppRef) {
        let (hwnd, change) = {
            let mut state = app.borrow_mut();
            let want = state.tick_wanted();
            let change = (state.tab_state.tick_armed != want).then_some(want);
            if let Some(want) = change {
                state.tab_state.tick_armed = want;
            }
            (state.hwnd, change)
        };
        if let Some(on) = change {
            win32::set_tab_tick(hwnd, on);
        }
    }

    // ---- closing ----

    /// Closes a tab as the user asked: a pinned tab may refuse or minimise, and the last tab may
    /// take the window with it. What happened is returned so the caller can close the window.
    pub fn user_close_tab(app: &AppRef, id: TabId) -> CloseKind {
        let (kind, was_active) = {
            let state = app.borrow();
            let pinned = state.tab_model.get(id).is_some_and(|tab| tab.pinned);
            (
                tab_options::close_kind(&state.settings.tabs, pinned, state.workspaces.active().tabs().len()),
                state.tab_model.active() == id,
            )
        };
        match kind {
            CloseKind::Close => {
                let _ = Self::close_tab(app, id);
            }
            CloseKind::Keep | CloseKind::CloseWindow => {}
            CloseKind::Minimise => {
                if was_active {
                    let _ = Self::minimise_active_tab(app);
                }
                // Another tab is showing now, so this one can sleep. A page with unsaved input or
                // sound stays awake, as for the menu's Hibernate.
                if let Some(tab) = app.borrow_mut().tabs.iter_mut().find(|tab| tab.id == id) {
                    tab.extra.allow_pinned_sleep = true;
                }
                if !Self::hibernate_tab(app, id)
                    && let Some(tab) = app.borrow_mut().tabs.iter_mut().find(|tab| tab.id == id)
                {
                    tab.extra.allow_pinned_sleep = false;
                }
            }
        }
        kind
    }

    /// Whether closing `count` tabs at once should ask first.
    pub fn close_needs_confirmation(app: &AppRef, count: usize) -> bool {
        tab_options::should_confirm_close(count, app.borrow().settings.tabs.confirm_close.0)
    }

    /// Clicking the active tab goes back to the tab used before it ("Minimise active tab").
    pub fn minimise_active_tab(app: &AppRef) -> TabId {
        Self::select_mru_other(app, 1)
    }

    // ---- closed tabs ----

    /// The tabs closed this session, newest first.
    pub fn closed_tabs(app: &AppRef) -> Vec<ClosedTab> {
        app.borrow().recently_closed.iter().cloned().collect()
    }

    /// Reopens the newest closed tab (Ctrl+Shift+T).
    pub fn reopen_closed_tab(app: &AppRef) -> Option<TabId> {
        Self::reopen_closed_at(app, 0)
    }

    /// Reopens the `n`th closed tab (0 is the newest) where it was, pinned if it was pinned.
    pub fn reopen_closed_at(app: &AppRef, n: usize) -> Option<TabId> {
        let entry = {
            let mut state = app.borrow_mut();
            state.recently_closed.remove(n)?
        };
        let url = url::Url::parse(&entry.url).ok()?;
        let id = Self::new_tab(app);
        Self::submit_url(app, url, None);
        if entry.pinned {
            let _ = Self::set_tab_pinned(app, id, true);
        } else {
            let count = Self::tab_ids(app).len();
            let _ = Self::move_tab(app, id, entry.index.min(count.saturating_sub(1)));
        }
        Some(id)
    }

    // ---- the strip ----

    /// What the strip draws for each of `ids`, read in one pass.
    pub fn tab_strip_extras(app: &AppRef, ids: &[TabId]) -> Vec<TabStripExtra> {
        let now_secs = unix_now();
        let state = app.borrow();
        let options = &state.settings.tabs;
        ids.iter()
            .filter_map(|id| {
                let tab = state.tabs.iter().find(|tab| tab.id == *id)?;
                let idle_secs = now_secs.saturating_sub(tab.extra.last_used);
                Some(TabStripExtra {
                    id: *id,
                    unread: options.unread_indicator && tab.extra.unread,
                    notify: if options.title_notifications { tab.extra.notify } else { 0 },
                    asleep: options.dim_hibernated && (tab.discarded || tab.extra.hibernated),
                    progress: tab.progress.percent,
                    reload: tab.extra.reload.map(|reload| (reload.secs(), reload.next)),
                    idle: tab.extra.last_used != 0
                        && state.tab_model.active() != *id
                        && tab_options::is_inactive(options, idle_secs),
                })
            })
            .collect()
    }

    /// The strip's selection (Ctrl and Shift click). Ids that are not tabs are dropped.
    pub fn set_tab_selection(app: &AppRef, ids: &[TabId]) {
        let order = Self::tab_ids(app);
        let mut state = app.borrow_mut();
        let enabled = state.settings.tabs.selection;
        let ids: &[TabId] = if enabled { ids } else { &[] };
        state.tab_state.set_selection(ids, &order);
    }

    /// The tabs a tab action applies to: the selection when `id` is in it, else `id` alone.
    pub fn selection_targets(app: &AppRef, id: TabId) -> Vec<TabId> {
        let state = app.borrow();
        let include_active = state.settings.tabs.selection_active.then(|| state.tab_model.active());
        state.tab_state.targets(id, include_active)
    }

    /// The tabs selected in the strip, in strip order, with the active tab when the options say it
    /// counts. Empty when fewer than two tabs are selected (one tab is not a selection).
    pub fn selected_tabs(app: &AppRef) -> Vec<TabId> {
        let state = app.borrow();
        let active = state.tab_model.active();
        let mut ids = state.tab_state.selected.clone();
        if state.settings.tabs.selection_active && !ids.is_empty() && !ids.contains(&active) {
            ids.push(active);
        }
        if ids.len() < 2 {
            return Vec::new();
        }
        let order = state.workspaces.active().tabs();
        ids.sort_by_key(|id| order.iter().position(|tab| tab == id).unwrap_or(usize::MAX));
        ids
    }

    pub fn selected_tab_count(app: &AppRef) -> usize {
        Self::selected_tabs(app).len()
    }

    /// The user's name for a tab ("Rename tab"); empty clears it.
    pub fn set_tab_name(app: &AppRef, id: TabId, name: &str) -> bool {
        let name: String = name.trim().chars().take(MAX_TAB_NAME_CHARS).collect();
        let hwnd = {
            let mut state = app.borrow_mut();
            let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == id) else {
                return false;
            };
            tab.extra.custom_title = (!name.is_empty()).then_some(name);
            state.hwnd
        };
        Self::post_tab_changed(hwnd, id);
        true
    }

    /// The name the user gave the tab, if any.
    pub fn tab_name(app: &AppRef, id: TabId) -> Option<String> {
        app.borrow().tabs.iter().find(|tab| tab.id == id).and_then(|tab| tab.extra.custom_title.clone())
    }

    // ---- the hover card: thumbnails and memory ----

    /// The thumbnail the hover card shows for `id` (a JPEG), when thumbnails are on and one was captured.
    /// A private tab never has one.
    pub fn tab_thumbnail(app: &AppRef, id: TabId) -> Option<Vec<u8>> {
        let state = app.borrow();
        if !state.settings.tab_preview_images || state.tabs.iter().any(|tab| tab.id == id && tab.private) {
            return None;
        }
        state.tab_state.thumbs.get(&id).cloned()
    }

    /// The memory the hover card shows for `id` in whole megabytes, when memory use is on and was measured.
    pub fn tab_memory_mb(app: &AppRef, id: TabId) -> Option<u32> {
        let state = app.borrow();
        if !state.settings.tab_preview_memory || state.tabs.iter().any(|tab| tab.id == id && tab.private) {
            return None;
        }
        let bytes = *state.tab_state.memory.get(&id)?;
        Some(((bytes + (1 << 19)) >> 20).clamp(1, 99_999) as u32)
    }

    /// Arms the one-shot timer that captures the active tab's thumbnail a moment from now (once a
    /// page has settled), when thumbnails are on.
    pub(super) fn schedule_thumbnail(app: &AppRef) {
        let (hwnd, wanted) = {
            let state = app.borrow();
            (state.hwnd, state.settings.tab_preview_images && state.engine.is_some())
        };
        if wanted {
            win32::set_thumbnail_timer(hwnd);
        }
    }

    /// The thumbnail timer fired: captures the tab on screen.
    pub fn thumbnail_timer_fired(app: &AppRef) {
        let active = Self::active_tab_id(app);
        Self::capture_thumbnail(app, active);
    }

    /// Captures the thumbnail of `id` (a tab on screen with a loaded web page) for its hover card. Not
    /// repeated within [`THUMB_MIN_GAP`], never for a private tab, and the picture stays in memory.
    pub fn capture_thumbnail(app: &AppRef, id: TabId) {
        let now = Instant::now();
        {
            let mut state = app.borrow_mut();
            let eligible = state.settings.tab_preview_images
                && !state.minimized
                && state.tabs.iter().any(|tab| {
                    tab.id == id
                        && !tab.private
                        && tab.controller_created
                        && tab.page_usable
                        && !tab.discarded
                        && tab.url.as_ref().is_some_and(|url| matches!(url.scheme(), "http" | "https"))
                });
            let fresh = state.tab_state.thumb_at.get(&id).is_some_and(|at| now.duration_since(*at) < THUMB_MIN_GAP);
            if !eligible || fresh {
                return;
            }
            state.tab_state.thumb_at.insert(id, now);
        }
        let weak = Rc::downgrade(app);
        Self::with_engine(app, move |engine| {
            engine.capture_tab_thumbnail(
                id,
                crate::thumbnail::THUMB_W,
                crate::thumbnail::THUMB_H,
                Box::new(move |bytes| {
                    let (Some(app), Some(bytes)) = (weak.upgrade(), bytes) else {
                        return;
                    };
                    Self::store_thumbnail(&app, id, bytes);
                }),
            );
            Ok(())
        });
    }

    /// Keeps a captured thumbnail for its tab (a tab that closed meanwhile has none). At most 64 are
    /// kept, the oldest captures dropped first.
    pub(crate) fn store_thumbnail(app: &AppRef, id: TabId, bytes: Vec<u8>) {
        if !crate::thumbnail::sendable(&bytes) {
            return;
        }
        let mut state = app.borrow_mut();
        if !state.tabs.iter().any(|tab| tab.id == id && !tab.private) {
            return;
        }
        state.tab_state.thumbs.insert(id, bytes);
        while state.tab_state.thumbs.len() > MAX_THUMBS {
            let oldest = state.tab_state.thumb_at.iter().filter(|(tab, _)| state.tab_state.thumbs.contains_key(tab) && **tab != id).min_by_key(|(_, at)| **at).map(|(tab, _)| *tab);
            match oldest {
                Some(tab) => {
                    state.tab_state.thumbs.remove(&tab);
                }
                None => break,
            }
        }
    }

    /// Measures the page process memory of the tabs (for the hover card's memory line), when it is on.
    /// The answer comes back later and refreshes the card if a number changed. Not repeated within
    /// [`MEMORY_MIN_GAP`].
    pub fn refresh_tab_memory(app: &AppRef) {
        let now = Instant::now();
        {
            let mut state = app.borrow_mut();
            if !state.settings.tab_preview_memory || state.engine.is_none() || state.tab_state.memory_at.is_some_and(|at| now.duration_since(at) < MEMORY_MIN_GAP) {
                return;
            }
            state.tab_state.memory_at = Some(now);
        }
        let weak = Rc::downgrade(app);
        Self::with_engine(app, move |engine| {
            engine.tab_memory_usage(Box::new(move |usage| {
                if let Some(app) = weak.upgrade() {
                    Self::store_memory(&app, usage);
                }
            }));
            Ok(())
        });
    }

    /// Keeps measured memory per tab; a change repaints the hover card.
    pub(crate) fn store_memory(app: &AppRef, usage: Vec<(TabId, u64)>) {
        let (changed, hwnd, active) = {
            let mut state = app.borrow_mut();
            let live: Vec<TabId> = state.tabs.iter().map(|tab| tab.id).collect();
            let fresh: HashMap<TabId, u64> = usage.into_iter().filter(|(id, _)| live.contains(id)).collect();
            // Compared in whole megabytes: a card that says 41 MB does not redraw for 41.2.
            let mb = |bytes: &u64| (bytes + (1 << 19)) >> 20;
            let changed = fresh.len() != state.tab_state.memory.len() || fresh.iter().any(|(id, bytes)| state.tab_state.memory.get(id).map(mb) != Some(mb(bytes)));
            state.tab_state.memory = fresh;
            (changed, state.hwnd, state.tab_model.active())
        };
        if changed {
            Self::post_tab_changed_kind(hwnd, active, false, false);
        }
    }

    // ---- pinned tabs stay on their site ----

    /// Notes the site a tab is pinned on (or forgets it when unpinned): the registrable host of its
    /// address, taken from the committed address, else from `fallback`.
    pub(super) fn note_pinned_site(state: &mut App, id: TabId, pinned: bool, fallback: Option<&url::Url>) {
        let site = pinned
            .then(|| {
                state
                    .tabs
                    .iter()
                    .find(|tab| tab.id == id)
                    .and_then(|tab| tab.url.as_ref())
                    .or(fallback)
                    .and_then(|url| url.host_str())
                    .map(tab_options::registrable)
            })
            .flatten();
        if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == id) {
            tab.extra.pinned_site = site;
        }
    }

    /// The engine is about to load a link the user followed in `tab`: when pinned tabs are kept on
    /// their site and this leaves it, the page opens in a new tab (a moment later, outside the engine's
    /// event) and the pinned tab stays where it is. True means the navigation was refused.
    pub(super) fn pinned_link(app: &AppRef, tab: TabId, url: &url::Url) -> bool {
        let (hwnd, refuse) = {
            let mut state = app.borrow_mut();
            let pinned = state.tab_model.get(tab).is_some_and(|model| model.pinned);
            // The site noted at pin time or restore; failing that, the site the tab is on now.
            let site = state.tabs.iter().find(|entry| entry.id == tab).and_then(|entry| {
                entry.extra.pinned_site.clone().or_else(|| {
                    entry.url.as_ref().and_then(|current| current.host_str()).map(tab_options::registrable)
                })
            });
            let refuse = tab_options::leaves_pinned_site(&state.settings.tabs, pinned, site.as_deref(), url);
            if refuse {
                state.tab_state.deferred_links.push((tab, url.clone()));
            }
            (state.hwnd, refuse)
        };
        if refuse {
            win32::set_link_timer(hwnd);
        }
        refuse
    }

    /// Opens the links pinned tabs refused, each in a tab of its own beside the pinned tab.
    pub fn run_deferred_links(app: &AppRef) {
        let links = std::mem::take(&mut app.borrow_mut().tab_state.deferred_links);
        for (tab, url) in links {
            let private = crate::private::inherits_privacy(Self::tab_private(app, tab));
            Self::open_link_tab(app, tab, private, url);
        }
    }

    /// Loads the restored tabs the options ask to load now, once the engine can. Called after a
    /// restore and again when the engine is ready; a tab already loaded is left alone.
    pub(super) fn load_eager_tabs(app: &AppRef) {
        if app.borrow().engine.is_none() {
            return;
        }
        let ids = std::mem::take(&mut app.borrow_mut().tab_state.eager);
        for id in ids {
            Self::restore_discarded_tab(app, id);
        }
    }

    // ---- sessions ----

    /// The tabs `ids` as a named session: their web pages in the order given, with the active tab
    /// marked. Private tabs and pages that are not http(s) are left out. None when nothing is left.
    pub fn session_of(app: &AppRef, name: &str, ids: &[TabId]) -> Option<crate::productivity::SavedSession> {
        let state = app.borrow();
        let active = state.tab_model.active();
        let mut urls = Vec::new();
        let mut active_at = 0;
        for id in ids {
            let Some(tab) = state.tabs.iter().find(|tab| tab.id == *id) else {
                continue;
            };
            if !crate::private::may_persist(crate::private::Store::SavedSessions, tab.private, false) {
                continue;
            }
            let Some(url) = tab.url.as_ref().filter(|url| matches!(url.scheme(), "http" | "https")) else {
                continue;
            };
            if tab.id == active {
                active_at = urls.len();
            }
            urls.push(url.to_string());
        }
        let name = name.trim();
        (!urls.is_empty() && !name.is_empty())
            .then(|| crate::productivity::SavedSession { name: name.chars().take(MAX_SESSION_NAME_CHARS).collect(), urls, active: active_at })
    }

    /// "Save all tabs as session": every tab of the window's active workspace.
    pub fn session_of_all(app: &AppRef, name: &str) -> Option<crate::productivity::SavedSession> {
        Self::session_of(app, name, &Self::tab_ids(app))
    }

    /// Saves a session of `ids` in the profile. The error is a sentence for the user.
    pub fn save_session(app: &AppRef, name: &str, ids: &[TabId]) -> std::result::Result<usize, String> {
        let profile = crate::profile::active().map_err(|_| "The profile is not ready.".to_owned())?;
        if !crate::productivity::profile_allows_productivity(&profile) {
            return Err("A guest window does not keep sessions.".to_owned());
        }
        let Some(session) = Self::session_of(app, name, ids) else {
            return Err("There are no web pages to save in this session.".to_owned());
        };
        let count = session.urls.len();
        crate::productivity::save_session(&profile.saved_sessions_file(), session)
            .map(|()| count)
            .map_err(|error| format!("The session could not be saved: {error}"))
    }

    /// Opens a saved session: one tab per page, in order, at the end of the strip (or in place of
    /// the open tabs with `replace`). The session's active page loads; the others wait asleep and
    /// load when selected, like restored tabs. Returns how many tabs opened.
    pub fn open_session(app: &AppRef, session: &crate::productivity::SavedSession, replace: bool) -> usize {
        let old = if replace { Self::tab_ids(app) } else { Vec::new() };
        let urls: Vec<url::Url> = session.urls.iter().filter_map(|value| url::Url::parse(value).ok()).collect();
        let active_at = session.active.min(urls.len().saturating_sub(1));
        let mut created = Vec::with_capacity(urls.len());
        let mut shown = None;
        for (at, url) in urls.iter().enumerate() {
            if at == active_at {
                let id = Self::new_tab(app);
                Self::submit_url(app, url.clone(), None);
                shown = Some(id);
                created.push(id);
            } else {
                let id = Self::new_background_tab(app, false, None);
                Self::make_asleep(app, id, url);
                created.push(id);
            }
        }
        for id in &created {
            let _ = Self::move_tab(app, *id, usize::MAX);
        }
        if let Some(id) = shown {
            let _ = Self::select_tab(app, id);
        }
        for id in old {
            let _ = Self::close_tab(app, id);
        }
        created.len()
    }

    /// Gives an empty background tab a page it has not loaded: it shows its address and loads when
    /// it is first selected.
    fn make_asleep(app: &AppRef, id: TabId, url: &url::Url) {
        let mut state = app.borrow_mut();
        state.tab_model.set_url(id, Some(url.as_str()));
        let title = url.host_str().unwrap_or_default().to_owned();
        state.tab_model.set_title(id, &title);
        if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == id) {
            tab.url = Some(url.clone());
            tab.title = title;
            tab.discarded = true;
        }
    }
}

/// Longest name a saved session can have.
pub const MAX_SESSION_NAME_CHARS: usize = 100;
/// Shortest time between two captures of the same tab's thumbnail.
pub const THUMB_MIN_GAP: Duration = Duration::from_secs(4);
/// Most thumbnails kept in memory.
pub const MAX_THUMBS: usize = 64;
/// Shortest time between two measurements of tab memory.
pub const MEMORY_MIN_GAP: Duration = Duration::from_secs(3);

/// Longest name a tab can be given.
pub const MAX_TAB_NAME_CHARS: usize = 100;

#[allow(dead_code)]
fn _keep_imports(_: &TabExtra, _: PinnedClose) {}
