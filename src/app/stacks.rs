//! What the tab stack, tiling and workspace options and commands do to the running app (T10). The
//! pure rules are in `crate::stacks` and `crate::workspace_rules`; a stack is a site island, so this
//! file only drives the island machinery (`tab_groups`, `tab_organisation`), the split view and the
//! workspace set that `app.rs` already keeps.

use super::*;
use crate::stacks::{self, StackInput, TileMode};

impl App {
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

    /// "Group tabs of this site": every tab of `id`'s site in this workspace joins one stack.
    /// Returns how many tabs the stack holds (0 when there were not two to stack).
    pub fn stack_site(app: &AppRef, id: TabId) -> usize {
        let (changed, count) = {
            let mut state = app.borrow_mut();
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

    /// "Group all tabs by site": one stack per site with two tabs or more. Returns how many stacks.
    pub fn stack_all_by_site(app: &AppRef) -> usize {
        let (changed, count) = {
            let mut state = app.borrow_mut();
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

    /// "Ungroup": every tab of `id`'s stack leaves it and stays out of automatic grouping.
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

    /// A tab dragged onto `target` and held there ("Group tabs by dragging one onto another"): both
    /// join `target`'s stack, or a new stack of `target`'s site, and the dragged tab moves next to it.
    /// Refused when either tab is pinned, private or has no site.
    pub fn stack_onto(app: &AppRef, dragged: TabId, target: TabId) -> bool {
        if dragged == target {
            return false;
        }
        let changed = {
            let mut state = app.borrow_mut();
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

    /// "New tab position: grouped with related": a new tab joins the stack of `anchor` (the tab that
    /// opened it), or starts one of `anchor`'s site with it. Whether the new tab joined.
    fn join_new_tab_to_stack(app: &AppRef, new: TabId, anchor: TabId) -> bool {
        let changed = {
            let mut state = app.borrow_mut();
            if new == anchor {
                return false;
            }
            let private = |id: TabId| state.mirror(id).is_none_or(|tab| tab.private);
            if private(new) || private(anchor) || state.tab_model.get(anchor).is_none_or(|tab| tab.pinned) {
                return false;
            }
            let island = state.domain_island_of(anchor).or_else(|| {
                state
                    .mirror(anchor)
                    .and_then(|tab| tab.url.as_ref())
                    .filter(|url| !tab_groups::is_search_results(url))
                    .and_then(tab_groups::DomainKey::from_url)
            });
            let Some(domain) = island else {
                return false;
            };
            state.join_stack(&[anchor, new], &domain)
        };
        Self::after_stack_change(app, changed);
        changed
    }

    /// "New tab position: grouped with related": a tab a page opened from `opener` joins its stack.
    pub(super) fn stack_new_tab(app: &AppRef, new: TabId, opener: Option<TabId>) {
        let as_stack = crate::tab_options::position_of(&app.borrow().settings) == crate::tab_options::NewTabPosition::Stack;
        if let Some(opener) = opener.filter(|_| as_stack) {
            let _ = Self::join_new_tab_to_stack(app, new, opener);
        }
    }

    /// "Pin all tabs in the group". Pinned tabs never stack, so the stack ends. Returns how many.
    pub fn pin_stack(app: &AppRef, id: TabId) -> usize {
        Self::stack_members(app, id).into_iter().filter(|member| Self::set_tab_pinned(app, *member, true)).count()
    }

    /// "Hibernate the group": every member but the tab in front sleeps. Returns how many were asked.
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
        let folder = Self::stack_name(app, id).ok_or_else(|| "This tab is not in a group.".to_owned())?;
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

    /// Moves the whole island (stack) `id` is in so it sits in front of tab `before` (`None`: the end
    /// of the strip): its handle was dragged, or moved by the keyboard. Every member moves, in its own
    /// order, and stays in the island; a drop inside another island lands in front of it and one among
    /// the pinned tabs after them (`stacks::island_move_anchor`). Like a single tab's move this is a
    /// reorder, not an organisation step, so Undo tab organisation has nothing to undo. Returns
    /// whether anything moved.
    pub fn move_island(app: &AppRef, id: TabId, before: Option<TabId>) -> bool {
        let (island, slots) = {
            let state = app.borrow();
            let Some(island) = state.domain_island_of(id) else {
                return false;
            };
            let inputs = state.group_inputs();
            let keys = tab_groups::group_keys(&inputs, &state.group_preferences);
            let slots: Vec<stacks::StripSlot> = tab_groups::strip_order(&inputs, &keys)
                .into_iter()
                .map(|(tab, key)| stacks::StripSlot {
                    id: tab,
                    island: key.map(|TabGroupKey::Domain(domain)| domain),
                    pinned: inputs.iter().any(|input| input.id == tab && input.pinned),
                })
                .collect();
            (island, slots)
        };
        let Some(anchor) = stacks::island_move_anchor(&slots, &island, before) else {
            return false;
        };
        let members: Vec<TabId> = slots
            .iter()
            .filter(|slot| !slot.pinned && slot.island.as_ref() == Some(&island))
            .map(|slot| slot.id)
            .collect();
        // Each member in turn goes right in front of the anchor (or to the end), so the block arrives
        // whole and in order. `move_tab` takes the index the tab has once moved.
        let mut moved = false;
        for member in members {
            let ids = Self::tab_ids(app);
            let Some(from) = ids.iter().position(|tab| *tab == member) else {
                continue;
            };
            let to = match anchor {
                stacks::IslandAnchor::End => ids.len() - 1,
                stacks::IslandAnchor::Before(target) => {
                    let Some(at) = ids.iter().position(|tab| *tab == target) else {
                        return moved;
                    };
                    if from < at { at - 1 } else { at }
                }
            };
            if to != from && Self::move_tab(app, member, to) {
                moved = true;
            }
        }
        moved
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

    /// A link followed in `tab` loads in place: when a workspace rule matches its site, the tab is
    /// moved to the rule's workspace a moment later ([`App::run_deferred_links`]), outside the
    /// engine's event. Whether a move was queued.
    pub(super) fn defer_workspace_rule(app: &AppRef, tab: TabId, url: &url::Url) -> bool {
        let (hwnd, queued) = {
            let mut state = app.borrow_mut();
            let applies = state.settings.tabs.workspaces && crate::workspace_rules::matching(&state.workspace_rules, url).is_some();
            if applies {
                state.tab_state.deferred_workspace.push((tab, url.clone()));
            }
            (state.hwnd, applies)
        };
        if queued {
            win32::set_link_timer(hwnd);
        }
        queued
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
