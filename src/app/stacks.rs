//! What the tab stack, tiling and workspace options and commands do to the running app (T10). The
//! pure rules are in `crate::stacks` and `crate::workspace_rules`; a stack is a site island, so this
//! file only drives the island machinery (`tab_groups`, `tab_organisation`), the split view and the
//! workspace set that `app.rs` already keeps.

use super::*;
use crate::stacks::{self, BallClick, StackInput, TileMode};
use crate::tab_options::StackStyle;

impl App {
    /// The stack style in force (Settings > Tab stacks, tiling and workspaces).
    pub fn stack_style(app: &AppRef) -> StackStyle {
        app.borrow().settings.tabs.stack_style
    }

    /// The group preferences as the strip draws them: the stored ones, folded the way the stack style
    /// says (`stacks::collapse_rule`).
    pub(super) fn styled_group_preferences(&self) -> crate::tab_groups::GroupPreferences {
        let style = self.settings.tabs.stack_style;
        let open = if style == StackStyle::Accordion { self.domain_island_of(self.tab_model.active()) } else { None };
        self.group_preferences.with_collapse_rule(stacks::collapse_rule(style, open))
    }

    // --- two-level stacks (T10b) ---
    /// The second tab row two-level stacks want now (`stacks::stack_row`): only with that style and
    /// the tab in use in a stack. Any other style returns at once, without projecting the islands.
    pub(crate) fn stack_row(&self) -> crate::chrome::layout::StackRow {
        let options = &self.settings.tabs;
        if options.stack_style != StackStyle::TwoLevel {
            return crate::chrome::layout::StackRow::None;
        }
        let in_stack = self.domain_island_of(self.tab_model.active()).is_some();
        stacks::stack_row(options.stack_style, options.stack_compact_row, in_stack)
    }

    /// The tabs the second row shows: the members of the stack the tab in use is in, in strip
    /// order, while two-level stacks lay that row out. Empty otherwise.
    pub fn stack_row_tabs(app: &AppRef) -> Vec<TabId> {
        let active = {
            let state = app.borrow();
            if state.stack_row() == crate::chrome::layout::StackRow::None {
                return Vec::new();
            }
            state.tab_model.active()
        };
        Self::stack_members_of_active(app, active)
    }

    fn stack_members_of_active(app: &AppRef, active: TabId) -> Vec<TabId> {
        let state = app.borrow();
        let Some(key) = state.domain_island_of(active) else {
            return Vec::new();
        };
        let inputs = state.group_inputs();
        let keys = tab_groups::group_keys(&inputs, &state.group_preferences);
        tab_groups::group_members(&keys, &TabGroupKey::Domain(key)).into_iter().map(|index| inputs[index].id).collect()
    }
    // --- end two-level stacks ---

    /// The members of the stack `id` is in, in strip order; empty when it is in none.
    pub fn stack_members(app: &AppRef, id: TabId) -> Vec<TabId> {
        Self::island_menu(app, id).map(|(_, ids)| ids).unwrap_or_default()
    }

    /// The stack's name: the one given, else the one Bowser suggests for its site.
    pub fn stack_name(app: &AppRef, id: TabId) -> Option<String> {
        let state = app.borrow();
        let key = state.domain_island_of(id)?;
        Some(state.group_preferences.name(&key).map_or_else(|| tab_organisation::suggested_name(&key), str::to_owned))
    }

    /// A click on the ball of the stack `member` is in. Under Islands the ball folds the island;
    /// Compact and Accordion fold by themselves, so the click selects a member instead
    /// (`stacks::ball_click`). Returns whether anything changed.
    pub fn stack_ball(app: &AppRef, member: TabId) -> bool {
        let (style, members, active, recent) = {
            let state = app.borrow();
            let members = match state.domain_island_of(member) {
                Some(key) => {
                    let inputs = state.group_inputs();
                    let keys = tab_groups::group_keys(&inputs, &state.group_preferences);
                    tab_groups::group_members(&keys, &TabGroupKey::Domain(key)).into_iter().map(|index| inputs[index].id).collect()
                }
                None => Vec::new(),
            };
            (state.settings.tabs.stack_style, members, state.tab_model.active(), state.tab_mru.clone())
        };
        match stacks::ball_click(style, &members, active, &recent) {
            BallClick::Toggle => false,
            BallClick::Select(tab) => Self::select_tab(app, tab),
            BallClick::Nothing => false,
        }
    }

    /// The active workspace's tabs as the stacking commands see them, in strip order.
    fn stack_inputs(&self) -> Vec<StackInput> {
        self.workspaces
            .active()
            .tabs()
            .iter()
            .filter_map(|id| {
                let tab = self.tab_model.get(*id)?;
                let mirror = self.mirror(*id)?;
                let domain = mirror
                    .url
                    .as_ref()
                    .filter(|url| !tab_groups::is_search_results(url))
                    .and_then(tab_groups::DomainKey::from_url);
                Some(StackInput { id: *id, domain, pinned: tab.pinned, private: mirror.private })
            })
            .collect()
    }

    /// Places every tab of `ids` in `domain`'s stack as one undo step. Whether anything changed.
    fn join_stack(&mut self, ids: &[TabId], domain: &tab_groups::DomainKey) -> bool {
        let before = self.organisation_snapshot();
        let mut changed = false;
        for id in ids {
            let placement = Membership::Joined(domain.clone());
            if self.memberships.get(id) != Some(&placement) {
                self.memberships.insert(*id, placement);
                changed = true;
            }
        }
        self.record_organisation_step(before, changed);
        changed
    }

    fn after_stack_change(app: &AppRef, changed: bool) {
        if changed {
            let (hwnd, active) = {
                let state = app.borrow();
                (state.hwnd, state.tab_model.active())
            };
            Self::post_tab_changed_kind(hwnd, active, true, false);
        }
    }

    /// "Stack tabs of this site": every tab of `id`'s site in this workspace joins one stack.
    /// Returns how many tabs the stack holds (0 when there were not two to stack).
    pub fn stack_site(app: &AppRef, id: TabId) -> usize {
        let (changed, count) = {
            let mut state = app.borrow_mut();
            if !stacks::stacks_on(state.settings.tabs.stack_style) {
                return 0;
            }
            let inputs = state.stack_inputs();
            let Some(domain) = inputs.iter().find(|tab| tab.id == id).and_then(|tab| tab.domain.clone()) else {
                return 0;
            };
            let members = stacks::site_members(&inputs, &domain);
            if members.len() < 2 {
                return 0;
            }
            (state.join_stack(&members, &domain), members.len())
        };
        Self::after_stack_change(app, changed);
        count
    }

    /// "Stack all tabs by site": one stack per site with two tabs or more. Returns how many stacks.
    pub fn stack_all_by_site(app: &AppRef) -> usize {
        let (changed, count) = {
            let mut state = app.borrow_mut();
            if !stacks::stacks_on(state.settings.tabs.stack_style) {
                return 0;
            }
            let plan = stacks::sites_to_stack(&state.stack_inputs());
            let before = state.organisation_snapshot();
            let mut changed = false;
            for (domain, members) in &plan {
                for id in members {
                    let placement = Membership::Joined(domain.clone());
                    if state.memberships.get(id) != Some(&placement) {
                        state.memberships.insert(*id, placement);
                        changed = true;
                    }
                }
            }
            state.record_organisation_step(before, changed);
            (changed, plan.len())
        };
        Self::after_stack_change(app, changed);
        count
    }

    /// [`Self::stack_all_by_site`], plus the tab whose stack "Open the stack editor when a stack is
    /// made" opens: the tab in front when it is now in a stack, else the first tab of any stack.
    /// The tab is `None` when no stack was made.
    pub fn stack_all_by_site_for_editor(app: &AppRef) -> (usize, Option<TabId>) {
        let count = Self::stack_all_by_site(app);
        if count == 0 {
            return (0, None);
        }
        let active = Self::active_tab_id(app);
        if !Self::stack_members(app, active).is_empty() {
            return (count, Some(active));
        }
        let first = Self::tab_ids(app).into_iter().find(|id| !Self::stack_members(app, *id).is_empty());
        (count, first)
    }

    /// "Unstack": every tab of `id`'s stack leaves it and stays out of automatic grouping.
    pub fn unstack(app: &AppRef, id: TabId) -> bool {
        let members = Self::stack_members(app, id);
        if members.is_empty() {
            return false;
        }
        let changed = {
            let mut state = app.borrow_mut();
            let before = state.organisation_snapshot();
            for member in &members {
                state.memberships.insert(*member, Membership::Ungrouped);
            }
            state.record_organisation_step(before, true);
            true
        };
        Self::after_stack_change(app, changed);
        changed
    }

    /// A tab dragged onto `target` and held there ("Stack tabs by dragging one onto another"): both
    /// join `target`'s stack, or a new stack of `target`'s site, and the dragged tab moves next to it.
    /// Refused when the option is off, stacks are off, or either tab is pinned, private or has no site.
    pub fn stack_onto(app: &AppRef, dragged: TabId, target: TabId) -> bool {
        if dragged == target {
            return false;
        }
        let changed = {
            let mut state = app.borrow_mut();
            let options = &state.settings.tabs;
            if !options.stack_drag || !stacks::stacks_on(options.stack_style) {
                return false;
            }
            let inputs = state.stack_inputs();
            let usable = |id: TabId| inputs.iter().find(|tab| tab.id == id).filter(|tab| !tab.pinned && !tab.private);
            let (Some(_), Some(target_input)) = (usable(dragged), usable(target)) else {
                return false;
            };
            let Some(domain) = state.domain_island_of(target).or_else(|| target_input.domain.clone()) else {
                return false;
            };
            state.join_stack(&[target, dragged], &domain)
        };
        Self::place_after(app, dragged, target);
        Self::after_stack_change(app, changed);
        true
    }

    /// "Open new tabs in the current stack" and "New tab position: as stack with related": a new tab
    /// joins the stack of `anchor` (the tab in front, or the tab that opened it). With `own_site`, a
    /// tab not yet in a stack starts one of its own site. Whether the new tab joined.
    pub(super) fn join_new_tab_to_stack(app: &AppRef, new: TabId, anchor: TabId, own_site: bool) -> bool {
        let changed = {
            let mut state = app.borrow_mut();
            if !stacks::stacks_on(state.settings.tabs.stack_style) || new == anchor {
                return false;
            }
            let private = |id: TabId| state.mirror(id).is_none_or(|tab| tab.private);
            if private(new) || private(anchor) || state.tab_model.get(anchor).is_none_or(|tab| tab.pinned) {
                return false;
            }
            let island = state.domain_island_of(anchor);
            let domain = match island {
                Some(domain) => domain,
                None if own_site => match state
                    .mirror(anchor)
                    .and_then(|tab| tab.url.as_ref())
                    .filter(|url| !tab_groups::is_search_results(url))
                    .and_then(tab_groups::DomainKey::from_url)
                {
                    Some(domain) => domain,
                    None => return false,
                },
                None => return false,
            };
            state.join_stack(&[anchor, new], &domain)
        };
        Self::after_stack_change(app, changed);
        changed
    }

    /// The stack options a freshly opened tab follows: `user` for Ctrl+T and the new tab button,
    /// else a tab a page opened from `opener`.
    pub(super) fn stack_new_tab(app: &AppRef, new: TabId, previous: TabId, opener: Option<TabId>) {
        let (in_current, as_stack) = {
            let state = app.borrow();
            (state.settings.tabs.stack_open_in_current, crate::tab_options::position_of(&state.settings) == crate::tab_options::NewTabPosition::Stack)
        };
        if let Some(opener) = opener.filter(|_| as_stack) {
            if Self::join_new_tab_to_stack(app, new, opener, true) {
                return;
            }
        }
        if in_current {
            let _ = Self::join_new_tab_to_stack(app, new, previous, false);
        }
    }

    /// "Pin all tabs in the stack". Pinned tabs never stack, so the stack ends. Returns how many.
    pub fn pin_stack(app: &AppRef, id: TabId) -> usize {
        Self::stack_members(app, id).into_iter().filter(|member| Self::set_tab_pinned(app, *member, true)).count()
    }

    /// "Hibernate the stack": every member but the tab in front sleeps. Returns how many were asked.
    pub fn hibernate_stack(app: &AppRef, id: TabId) -> usize {
        let active = Self::active_tab_id(app);
        Self::stack_members(app, id)
            .into_iter()
            .filter(|member| *member != active && Self::hibernate_tab(app, *member))
            .count()
    }

    /// The web addresses of the stack's tabs, in strip order.
    pub fn stack_links(app: &AppRef, id: TabId) -> Vec<url::Url> {
        Self::stack_members(app, id)
            .into_iter()
            .filter_map(|member| Self::tab_committed_url(app, member))
            .filter(|url| matches!(url.scheme(), "http" | "https"))
            .collect()
    }

    /// "Copy all links": one address per line.
    pub fn stack_links_text(app: &AppRef, id: TabId) -> String {
        Self::stack_links(app, id).iter().map(url::Url::as_str).collect::<Vec<_>>().join("\r\n")
    }

    /// "Bookmark all tabs": a bookmark folder named after the stack with every web page in it.
    /// Returns how many were bookmarked; the error is a sentence for the user.
    pub fn bookmark_stack(app: &AppRef, id: TabId) -> std::result::Result<usize, String> {
        let profile = crate::profile::active().map_err(|_| "The profile is not ready.".to_owned())?;
        if !crate::productivity::profile_allows_productivity(&profile) {
            return Err("A guest window does not keep bookmarks.".to_owned());
        }
        let folder = Self::stack_name(app, id).ok_or_else(|| "This tab is not in a stack.".to_owned())?;
        let mut count = 0;
        for member in Self::stack_members(app, id) {
            let Some(url) = Self::tab_committed_url(app, member).filter(|url| matches!(url.scheme(), "http" | "https")) else {
                continue;
            };
            let title = Self::tab_title(app, member).filter(|title| !title.trim().is_empty()).unwrap_or_else(|| url.to_string());
            crate::productivity::add_bookmark_in_folder(&profile.bookmarks_file(), url.as_str(), &title, &folder)
                .map_err(|error| format!("The bookmarks could not be saved: {error}"))?;
            count += 1;
        }
        Ok(count)
    }

    /// Tiles `tabs` (2 to 4) in the split view (`stacks::tile_layout`). `bounds_for` gives the pane
    /// rectangles of the layout, from the window. Whether the tiling opened.
    pub fn tile_tabs(
        app: &AppRef,
        tabs: &[TabId],
        mode: TileMode,
        bounds_for: impl FnOnce(crate::split_view::SplitView) -> Option<SplitBounds>,
    ) -> bool {
        let active = Self::active_tab_id(app);
        let open: Vec<TabId> = tabs.iter().copied().filter(|tab| app.borrow().tab_model.get(*tab).is_some()).collect();
        let Some(view) = stacks::tile_layout(&open, mode, active) else {
            return false;
        };
        let bounds = bounds_for(view);
        Self::open_split_arranged(app, view, bounds)
    }

    /// The tabs a tiling command tiles: the selection, else the stack of the tab in front.
    pub fn tile_targets(app: &AppRef) -> Vec<TabId> {
        let active = Self::active_tab_id(app);
        let order = Self::tab_ids(app);
        let mut selected = Self::selected_tabs(app);
        selected.sort_by_key(|tab| order.iter().position(|other| other == tab).unwrap_or(usize::MAX));
        stacks::tile_targets(&selected, &Self::stack_members(app, active))
    }

    /// Moves `tabs` to workspace `target`. A workspace keeps one tab at least, so when every tab of
    /// the workspace in front would leave, a new tab is opened there first. With `follow`, the window
    /// switches to `target` and shows the first moved tab; otherwise it stays, and a moved tab that was
    /// in front gives way to another. Returns how many tabs moved.
    pub fn move_tabs_to_workspace(app: &AppRef, tabs: &[TabId], target: crate::workspaces::WorkspaceId, follow: bool) -> usize {
        let (source, leaving_all) = {
            let state = app.borrow();
            if state.workspaces.get(target).is_none() {
                return 0;
            }
            let source = state.workspaces.active_id();
            let in_source: Vec<TabId> = state.workspaces.active().tabs().to_vec();
            let leaving_all = in_source.iter().all(|tab| tabs.contains(tab));
            (source, leaving_all)
        };
        if source == target {
            return 0;
        }
        // A pane of a split may not leave it for another workspace: the layout separates first.
        if app.borrow().split_view.or(app.borrow().split_parked).is_some_and(|split| tabs.iter().any(|tab| split.contains(*tab))) {
            let _ = Self::separate_split(app);
        }
        if leaving_all {
            let _ = Self::new_tab(app);
        }
        let active = Self::active_tab_id(app);
        let mut moved = Vec::new();
        {
            let mut state = app.borrow_mut();
            for tab in tabs {
                if state.workspaces.workspace_for(*tab) == Some(source) && state.workspaces.move_tab_to(*tab, target).is_ok() {
                    moved.push(*tab);
                }
            }
        }
        if moved.is_empty() {
            return 0;
        }
        if follow {
            let first = moved[0];
            let _ = app.borrow_mut().workspaces.select_tab(first);
            let _ = Self::select_tab(app, first);
        } else if moved.contains(&active) {
            let shown = app.borrow().workspaces.active().active_tab();
            let _ = Self::select_tab(app, shown);
        }
        let hwnd = app.borrow().hwnd;
        Self::post_tab_changed(hwnd, Self::active_tab_id(app));
        Self::save_workspaces(app, WorkspaceChange::TabMove);
        Self::notify_rail_changed(app);
        moved.len()
    }

    // ----- workspace rules -----

    /// The workspace rules (Settings > Tab stacks, tiling and workspaces > Workspace rules).
    pub fn workspace_rules(app: &AppRef) -> Vec<crate::workspace_rules::Rule> {
        app.borrow().workspace_rules.clone()
    }

    /// Replaces the rules and saves them (never for a guest window). The error is a sentence.
    pub fn set_workspace_rules(app: &AppRef, rules: Vec<crate::workspace_rules::Rule>) -> std::result::Result<(), String> {
        let profile = crate::profile::active().ok().filter(|profile| !profile.is_guest());
        if let Some(profile) = &profile {
            crate::workspace_rules::save(profile.root(), &rules).map_err(|error| format!("The workspace rules could not be saved: {error}"))?;
        }
        app.borrow_mut().workspace_rules = rules;
        Ok(())
    }

    /// Before `tab` opens `url`: a rule for the site sends the tab to its workspace. `follow` switches
    /// there too (the tab in front); a background tab moves without the window following. Only with
    /// workspaces on, never for a private or pinned tab. Whether the tab moved.
    pub(super) fn apply_workspace_rule(app: &AppRef, tab: TabId, url: &url::Url, follow: bool) -> bool {
        let target = {
            let state = app.borrow();
            if !state.settings.tabs.workspaces || state.workspace_rules.is_empty() {
                return false;
            }
            if state.mirror(tab).is_none_or(|mirror| mirror.private) || state.tab_model.get(tab).is_none_or(|model| model.pinned) {
                return false;
            }
            let Some(rule) = crate::workspace_rules::matching(&state.workspace_rules, url) else {
                return false;
            };
            let Some(target) = state.workspaces.find_by_name(&rule.workspace) else {
                return false;
            };
            if state.workspaces.workspace_for(tab) != Some(state.workspaces.active_id()) {
                return false;
            }
            target
        };
        let moved = Self::move_tabs_to_workspace(app, &[tab], target, follow) == 1;
        if moved {
            // A stack it joined on the way stays behind: the tab starts afresh in its workspace.
            app.borrow_mut().memberships.remove(&tab);
        }
        moved
    }

    /// The wheel turned `steps` notches over the rail's workspaces ("Switch workspaces by
    /// scrolling"). Whether the workspace changed.
    pub fn scroll_workspaces(app: &AppRef, steps: i32) -> bool {
        let on = {
            let options = &app.borrow().settings.tabs;
            options.workspace_scroll && options.workspaces
        };
        on && Self::step_workspace(app, steps)
    }

    /// "Next workspace" / "Previous workspace": `steps` places along the rail's order, stopping at
    /// the ends. Whether the workspace changed.
    pub fn step_workspace(app: &AppRef, steps: i32) -> bool {
        let target = {
            let state = app.borrow();
            let order: Vec<_> = state.workspaces.all().iter().map(|workspace| workspace.id()).collect();
            stacks::workspace_scroll_target(&order, state.workspaces.active_id(), steps)
        };
        target.is_some_and(|target| Self::switch_workspace(app, target))
    }

    /// "Icon or emoji": shows `emoji` instead of the workspace's icon (`None` goes back to the icon).
    /// Returns whether the workspace now shows an emoji.
    pub fn workspace_set_emoji(
        app: &AppRef,
        id: crate::workspaces::WorkspaceId,
        emoji: Option<&str>,
    ) -> std::result::Result<bool, crate::workspaces::WorkspaceError> {
        let shown = app.borrow_mut().workspaces.set_emoji(id, emoji)?;
        Self::save_workspaces(app, WorkspaceChange::Icon);
        Self::notify_rail_changed(app);
        let hwnd = app.borrow().hwnd;
        Self::post_tab_changed(hwnd, Self::active_tab_id(app));
        Ok(shown)
    }

    /// Asks the window to rebuild the rail (the tab counts changed).
    pub fn rail_changed(app: &AppRef) {
        Self::notify_rail_changed(app);
    }

    /// The emoji workspace `id` shows instead of its icon, if any.
    pub fn workspace_emoji(app: &AppRef, id: crate::workspaces::WorkspaceId) -> Option<String> {
        app.borrow().workspaces.get(id).and_then(|workspace| workspace.emoji().map(str::to_owned))
    }

    /// Whether workspaces are on (Settings > Tab stacks, tiling and workspaces > Workspaces).
    pub fn workspaces_enabled(app: &AppRef) -> bool {
        app.borrow().settings.tabs.workspaces
    }
}
