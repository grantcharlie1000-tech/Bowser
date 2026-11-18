//! The toolbar customiser docked beside the page (docs/toolbars.md): every button the address bar and the
//! tab bar can show, as switches, with the page pushed aside so the bar it edits stays in view and changes
//! as the switches move.
//!
//! The panel is a [`side_panel`] content. The side panel host owns the width, the 220 ms open and close and
//! the page's narrower rectangle; this content gives it one native child window, and that child holds the
//! HTML page (`?view=dialog&p=toolbar`, `ui/src/panels/toolbar.ts`) through `html_dialog`'s host, the same
//! way Feeds and Notes are hosted. The child is made once per window and kept: closing the panel slides it
//! out with the host and reopening shows it again with a fresh snapshot, so nothing reloads.
//!
//! The page sends `tbar` commands: `set` with `toolbar_buttons=...` or `toolbar_tabs=...` (checked whole by
//! `toolbar_prefs::apply_assignment`, as every other writer of those lists), and `panel`/`dismiss` closes it.

use std::{cell::RefCell, collections::HashMap};

use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::{GetParent, IsWindow, KillTimer, PostMessageW, SetTimer, ShowWindow, SW_HIDE, SW_SHOWNA},
};

use crate::{
    app::{App, AppRef},
    chrome_host::protocol::{self, PanelFields},
};

use super::side_panel::{self, PanelContent, PanelId};

/// The child window's "the panel's content changed" message: hide the page if another panel took over.
pub(crate) const WM_TOOLBAR_RETIRED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x5C7;
/// Timer on the child window that hides it once a close animation has had time to finish.
pub(crate) const HIDE_TIMER: usize = 0x7B01;
/// A little longer than the side panel's open and close (220 ms).
const HIDE_AFTER_MS: u32 = 260;

thread_local! {
    /// The customiser's child window per Bowser window (`owner` -> child), kept while the window lives.
    static CHILDREN: RefCell<HashMap<isize, isize>> = RefCell::new(HashMap::new());
}

fn child_of(owner: HWND) -> Option<HWND> {
    let raw = CHILDREN.with(|map| map.borrow().get(&(owner.0 as isize)).copied())?;
    let child = HWND(raw as *mut std::ffi::c_void);
    if unsafe { IsWindow(Some(child)) }.as_bool() {
        Some(child)
    } else {
        CHILDREN.with(|map| map.borrow_mut().remove(&(owner.0 as isize)));
        None
    }
}

/// The child window `child` was destroyed: forget it.
pub(crate) fn forget(child: HWND) {
    CHILDREN.with(|map| map.borrow_mut().retain(|_, raw| *raw != child.0 as isize));
}

/// Registers the customiser with the side panel host. The registry is shared by every window, so a content
/// learns its window from the host it is given (`on_host_created`).
pub(crate) fn register(app: &AppRef) {
    let app = app.clone();
    side_panel::register_content(PanelId::Toolbar, move || Box::new(Content { owner: HWND::default(), app: app.clone(), child: HWND::default() }));
}

/// Opens the customiser beside the page, or closes it when it is already open (the settings row and the
/// command both toggle). Without the HTML chrome there is no page to show: Bowser says so.
pub(crate) fn toggle(owner: HWND, app: &AppRef) {
    if !super::html_dialog::html_only(owner) || super::window::html_environment(owner).is_none() {
        super::window::show_toast(owner, "Customising the toolbar needs Bowser's own interface, which is not running", "Toolbar");
        return;
    }
    let _ = app;
    if !side_panel::toggle(owner, PanelId::Toolbar)
        && let Some(reason) = side_panel::last_status()
    {
        super::window::show_toast(owner, reason, "Toolbar");
    }
}

struct Content {
    owner: HWND,
    app: AppRef,
    child: HWND,
}

impl PanelContent for Content {
    fn on_host_created(&mut self, host: HWND) {
        // The host is a child of the Bowser window it docks in.
        let Ok(owner) = (unsafe { GetParent(host) }) else { return };
        self.owner = owner;
        let child = match child_of(self.owner) {
            Some(child) => child,
            None => {
                let Some(env) = super::window::html_environment(self.owner) else { return };
                let Some(child) = super::html_dialog::create_child(host) else { return };
                CHILDREN.with(|map| map.borrow_mut().insert(self.owner.0 as isize, child.0 as isize));
                super::html_dialog::attach(&self.app, self.owner, child, env, super::html_dialog::Kind::Toolbar);
                child
            }
        };
        unsafe {
            let _ = KillTimer(Some(child), HIDE_TIMER);
            let _ = ShowWindow(child, SW_SHOWNA);
        }
        self.child = child;
        // Reopened: the lists may have changed elsewhere (the settings page, a reset).
        super::html_dialog::refresh(child);
    }

    fn on_focus(&mut self, focused: bool) {
        if focused && !self.child.is_invalid() {
            super::html_dialog::focus(self.child);
        }
    }

    /// The side panel focuses its host after showing it; the page inside takes the keyboard from there.
    fn after_shown(&mut self) {
        if !self.child.is_invalid() {
            super::html_dialog::focus(self.child);
        }
    }

    fn min_width(&self) -> i32 { 300 }
    fn preferred_width(&self) -> i32 { 340 }
    fn title(&self) -> &str { "Toolbar" }
    /// The page draws its own title row and close button.
    fn native_header(&self) -> bool { false }
    fn wants_native_child(&self) -> Option<HWND> { (!self.child.is_invalid()).then_some(self.child) }
}

impl Drop for Content {
    /// The side panel let go of the customiser: another panel replaced it, or it is closing. Which one is
    /// asked after the host has finished this change (a posted message), never from inside it.
    fn drop(&mut self) {
        if !self.child.is_invalid() {
            unsafe { let _ = PostMessageW(Some(self.child), WM_TOOLBAR_RETIRED, WPARAM(0), LPARAM(0)); }
        }
    }
}

/// The child's `WM_TOOLBAR_RETIRED`: hidden at once when another panel now shows (it would cover it), or
/// once the close animation has carried it out of sight.
pub(crate) fn retired(child: HWND, owner: HWND) {
    match side_panel::active_id(owner) {
        Some(PanelId::Toolbar) => {}
        Some(_) => unsafe { let _ = ShowWindow(child, SW_HIDE); },
        None => unsafe { let _ = SetTimer(Some(child), HIDE_TIMER, HIDE_AFTER_MS, None); },
    }
}

/// The child's `HIDE_TIMER`.
pub(crate) fn hide_timer(child: HWND, owner: HWND) {
    unsafe {
        let _ = KillTimer(Some(child), HIDE_TIMER);
        if side_panel::active_id(owner) != Some(PanelId::Toolbar) {
            let _ = ShowWindow(child, SW_HIDE);
        }
    }
}

/// The customiser's `toolbar` snapshot: both lists and every command a button can run.
pub(crate) fn snapshot_json(app: &AppRef) -> Option<String> {
    let settings = App::settings(app);
    let live = crate::commands::live();
    let entries = live.store.entries();
    protocol::snap_json("toolbar", |out| {
        out.string("address", &settings.toolbar_buttons.join(","));
        out.string("tabs", &settings.toolbar_tabs.join(","));
        out.array("commands", |list| {
            for entry in &entries {
                // Shortcut-only commands are never offered as a button (as in the settings page).
                if entry.id.starts_with("select-") {
                    continue;
                }
                let chain = entry.id.starts_with("chain:");
                list.object(|item| {
                    item.string("id", &entry.id);
                    item.string("label", &entry.label);
                    item.bool("chain", chain);
                    if let Some(glyph) = crate::toolbar_prefs::command_glyph(&entry.id) {
                        item.string("glyph", glyph);
                    }
                });
            }
        });
    })
}

/// What a `tbar` command asks, checked. Pure.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// `toolbar_buttons=...` or `toolbar_tabs=...`, already accepted by `apply_assignment` on a probe.
    Set(String),
}

pub(crate) fn action_of(f: &PanelFields) -> Option<Action> {
    let bare = f.id.is_none() && f.parent.is_none() && f.before.is_none() && f.n.is_none() && f.on.is_none() && f.url.is_none();
    match f.a.as_str() {
        "set" if bare => {
            let text = f.text.as_deref()?;
            if !(text.starts_with("toolbar_buttons=") || text.starts_with("toolbar_tabs=")) || text.len() > 1024 {
                return None;
            }
            let mut probe = crate::settings::Settings::default();
            crate::toolbar_prefs::apply_assignment(&mut probe, text).then(|| Action::Set(text.to_owned()))
        }
        _ => None,
    }
}

/// Runs one checked command: the list is kept, saved and put to work at once (the toolbar or strip beside the
/// panel redraws), and an open settings page reads it. `Err` says why nothing (or not all of it) happened.
pub(crate) fn handle(app: &AppRef, owner: HWND, action: Action) -> Result<(), String> {
    match action {
        Action::Set(text) => {
            let result = App::set_bar_option(app, &text);
            super::settings_page::refresh(owner);
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(a: &str, text: Option<&str>) -> PanelFields {
        PanelFields { a: a.to_owned(), text: text.map(str::to_owned), ..PanelFields::default() }
    }

    #[test]
    fn only_whole_valid_lists_of_the_two_bars_are_accepted() {
        assert_eq!(action_of(&fields("set", Some("toolbar_buttons=back,reload"))), Some(Action::Set("toolbar_buttons=back,reload".into())));
        assert_eq!(action_of(&fields("set", Some("toolbar_tabs=closed-tabs,cmd:new-tab"))), Some(Action::Set("toolbar_tabs=closed-tabs,cmd:new-tab".into())));
        assert_eq!(action_of(&fields("set", Some("toolbar_buttons="))), Some(Action::Set("toolbar_buttons=".into())), "an empty bar is allowed");
        for bad in ["toolbar_panel=feeds", "toolbar_buttons=back,back", "toolbar_buttons=bogus", "feeds_detect=false", "toolbar_tabs=back", ""] {
            assert_eq!(action_of(&fields("set", Some(bad))), None, "{bad}");
        }
        assert_eq!(action_of(&fields("set", None)), None);
        assert_eq!(action_of(&PanelFields { n: Some(1), ..fields("set", Some("toolbar_buttons=back")) }), None, "no stray fields");
        assert_eq!(action_of(&fields("reset", Some("toolbar_buttons=back"))), None);
    }
}
