//! "This page isn't responding" — a nonmodal Wait / Reload bar over a hung tab's pane.
//!
//! Native controls, like the content-failure cards, so it works with the HTML chrome and the
//! native fallback alike and needs no WebView2 process to stay usable. It never takes the
//! keyboard on its own: F6 from the address bar reaches it, Reload (F5 / Ctrl+R) works from
//! anywhere, Tab / Shift+Tab / arrows move between its buttons and Escape chooses Wait.
use super::*;
use windows::core::PCWSTR;
use windows::Win32::UI::{
    Input::KeyboardAndMouse::{GetFocus, SetFocus, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT, VK_TAB, VK_UP},
    WindowsAndMessaging::{BM_CLICK, BS_PUSHBUTTON, GetDlgCtrlID, SW_SHOWNA, WINDOW_EX_STYLE, WM_KEYDOWN, WM_SETFONT, WS_CLIPSIBLINGS, WS_TABSTOP},
};

pub(super) const WAIT_ID: usize = 0x6f40;
pub(super) const RELOAD_ID: usize = 0x6f48;
const SUBCLASS_ID: usize = 0x6f40;
// STATIC styles (WinUser.h): left-aligned, vertically centred single line, no & mnemonics.
const SS_LEFT: u32 = 0x0;
const SS_CENTERIMAGE: u32 = 0x200;
const SS_NOPREFIX: u32 = 0x80;

/// One pane's prompt: its label, its two buttons, and the tab it speaks for.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Prompt {
    pub(super) label: HWND,
    pub(super) wait: HWND,
    pub(super) reload: HWND,
    pub(super) tab: Option<TabId>,
}

impl Prompt {
    fn controls(self) -> [HWND; 3] { [self.label, self.wait, self.reload] }
    fn owns_focus(self, focus: HWND) -> bool { !focus.is_invalid() && (focus == self.wait || focus == self.reload) }
}

/// Which prompt a WM_COMMAND id belongs to, and whether it is the Reload button.
pub(super) fn command(id: usize) -> Option<(usize, bool)> {
    if (WAIT_ID..WAIT_ID + 4).contains(&id) { return Some((id - WAIT_ID, false)); }
    if (RELOAD_ID..RELOAD_ID + 4).contains(&id) { return Some((id - RELOAD_ID, true)); }
    None
}

/// A prompt button was chosen. Wait keeps the document; Reload opens the address again as a GET.
/// A button that held the keyboard hands it back to the page so focus never rests on a hidden control.
pub(super) fn choose(hwnd: HWND, app: &AppRef, index: usize, reload: bool) {
    let Some(prompt) = with_chrome(hwnd, |s| s.hang_prompts.get(index).copied()).flatten() else { return; };
    let Some(tab) = prompt.tab else { return; };
    let had_focus = prompt.owns_focus(unsafe { GetFocus() });
    let acted = if reload { App::reload_unresponsive_tab(app, tab) } else { App::wait_unresponsive_tab(app, tab) };
    if acted {
        sync(hwnd, app);
        if had_focus {
            unsafe { let _ = SetFocus(Some(hwnd)); }
            App::focus_page(app);
        }
    }
}

/// F6 towards the page: a hung or failed page cannot take the keyboard, so its visible Wait
/// button or failure card does.
pub(super) fn focus(hwnd: HWND, app: &AppRef) -> bool {
    let active = App::active_tab_id(app);
    let target = with_chrome(hwnd, |s| {
        let hang = s.hang_prompts.iter().find(|prompt| prompt.tab == Some(active)).map(|prompt| prompt.wait);
        let card = s.recovery_buttons.iter().find(|(_, tab)| *tab == Some(active)).map(|(button, _)| *button);
        hang.filter(|_| App::unresponsive_prompt(app, active))
            .or(card.filter(|_| App::content_failure_message(app, active).is_some()))
    }).flatten();
    let Some(target) = target.filter(|target| !target.is_invalid()) else { return false; };
    unsafe { let _ = SetFocus(Some(target)); }
    true
}

fn create(class: PCWSTR, text: &str, style: WINDOW_STYLE, parent: HWND, id: usize) -> Option<HWND> {
    let text: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        CreateWindowExW(WINDOW_EX_STYLE(0), class, PCWSTR(text.as_ptr()), WS_CHILD | style, 0, 0, 1, 1,
            Some(parent), Some(HMENU(id as *mut _)), None, None).ok()
    }
}

/// Keyboard for a native page-area button (a hang prompt choice or a failure card). Outside a
/// dialog a BUTTON clicks only on Space; Enter must click it too.
pub(super) fn install_keys(button: HWND) {
    if !button.is_invalid() { unsafe { let _ = SetWindowSubclass(button, Some(button_keys), SUBCLASS_ID, 0); } }
}

unsafe extern "system" fn button_keys(button: HWND, msg: u32, w: WPARAM, l: LPARAM, _: usize, _: usize) -> LRESULT {
    if msg == WM_KEYDOWN {
        let key = w.0 as u16;
        if key == VK_RETURN.0 {
            unsafe { let _ = SendMessageW(button, BM_CLICK, None, None); }
            return LRESULT(0);
        }
        let parent = unsafe { GetParent(button) }.unwrap_or_default();
        let id = unsafe { GetDlgCtrlID(button) } as usize;
        if let Some((index, reload)) = command(id) {
            if key == VK_ESCAPE.0 {
                if let Some(app) = app_of(parent) { choose(parent, &app, index, false); }
                return LRESULT(0);
            }
            if [VK_TAB.0, VK_LEFT.0, VK_RIGHT.0, VK_UP.0, VK_DOWN.0].contains(&key) {
                let other = with_chrome(parent, |s| s.hang_prompts.get(index).map(|prompt| if reload { prompt.wait } else { prompt.reload })).flatten();
                if let Some(other) = other.filter(|other| !other.is_invalid()) { unsafe { let _ = SetFocus(Some(other)); } }
                return LRESULT(0);
            }
        }
    }
    unsafe { DefSubclassProc(button, msg, w, l) }
}

/// Shows, places or hides each pane's prompt to match the App. Called with the failure cards.
pub(super) fn sync(hwnd: HWND, app: &AppRef) {
    if hwnd.is_invalid() || app.borrow().closing { return; }
    let mut shown = [None; 4];
    if let Some(split) = App::split_view(app) {
        if let Some(bounds) = split_rects(hwnd, None) {
            for (pane, tab) in split.panes() {
                if let Some(rect) = bounds.get(pane) && App::unresponsive_prompt(app, tab) {
                    shown[pane.index()] = Some((tab, rect));
                }
            }
        }
    } else if let Some(rect) = split_viewport(hwnd) && App::unresponsive_prompt(app, App::active_tab_id(app)) {
        shown[0] = Some((App::active_tab_id(app), rect));
    }
    let Some(prompts) = with_chrome(hwnd, |s| s.hang_prompts) else { return; };
    let font = app.borrow().input_font;
    let focus = unsafe { GetFocus() };
    let mut regions = [None; 4];
    let mut lost_focus = false;
    for (index, mut prompt) in prompts.into_iter().enumerate() {
        let Some((tab, rect)) = shown[index] else {
            if prompt.tab.is_some() {
                lost_focus |= prompt.owns_focus(focus);
                for control in prompt.controls().into_iter().filter(|control| !control.is_invalid()) {
                    unsafe { let _ = ShowWindow(control, SW_HIDE); }
                }
                let _ = with_chrome(hwnd, |s| s.hang_prompts[index].tab = None);
            }
            continue;
        };
        if prompt.label.is_invalid() {
            // Clip the label to its siblings so its background never paints over the buttons.
            let style = WS_CLIPSIBLINGS | WINDOW_STYLE(SS_LEFT | SS_CENTERIMAGE | SS_NOPREFIX);
            prompt.label = create(w!("STATIC"), &format!("  {}", crate::app::UNRESPONSIVE_MESSAGE), style, hwnd, 0xffff).unwrap_or_default();
        }
        for (slot, id, text) in [(&mut prompt.wait, WAIT_ID, crate::app::UNRESPONSIVE_WAIT), (&mut prompt.reload, RELOAD_ID, crate::app::UNRESPONSIVE_RELOAD)] {
            if slot.is_invalid() {
                *slot = create(w!("BUTTON"), text, WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), hwnd, id + index).unwrap_or_default();
                install_keys(*slot);
            }
        }
        if prompt.controls().iter().any(|control| control.is_invalid()) {
            for control in prompt.controls().into_iter().filter(|control| !control.is_invalid()) { unsafe { let _ = DestroyWindow(control); } }
            let _ = with_chrome(hwnd, |s| s.hang_prompts[index] = Prompt::default());
            continue;
        }
        let margin = split_px(hwnd, 8);
        let pad = split_px(hwnd, 4);
        let gap = split_px(hwnd, 6);
        let pane_width = rect.right - rect.left;
        let width = split_px(hwnd, 460).min((pane_width - 2 * margin).max(1));
        let height = split_px(hwnd, 36).min((rect.bottom - rect.top).max(1));
        let left = rect.left + (pane_width - width) / 2;
        let top = rect.top + margin.min((rect.bottom - rect.top - height).max(0));
        let reload_width = split_px(hwnd, 104).min(width / 3);
        let wait_width = split_px(hwnd, 72).min(width / 4);
        let reload_left = left + width - pad - reload_width;
        let wait_left = reload_left - gap - wait_width;
        let button_height = (height - 2 * pad).max(1);
        unsafe {
            for control in prompt.controls() {
                let _ = SendMessageW(control, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(0)));
            }
            // Raised in order: label, then the buttons above it, all above the page's controller.
            let _ = SetWindowPos(prompt.label, Some(HWND_TOP), left, top, width, height, SWP_NOACTIVATE);
            let _ = SetWindowPos(prompt.wait, Some(HWND_TOP), wait_left, top + pad, wait_width, button_height, SWP_NOACTIVATE);
            let _ = SetWindowPos(prompt.reload, Some(HWND_TOP), reload_left, top + pad, reload_width, button_height, SWP_NOACTIVATE);
        }
        prompt.tab = Some(tab);
        if with_chrome(hwnd, |s| s.hang_prompts[index] = prompt).is_none() {
            for control in prompt.controls() { unsafe { let _ = DestroyWindow(control); } }
            continue;
        }
        for control in prompt.controls() { unsafe { let _ = ShowWindow(control, SW_SHOWNA); } }
        regions[index] = Some(Rect::new(left, top, left + width, top + height));
    }
    if with_chrome(hwnd, |s| std::mem::replace(&mut s.hang_regions, regions) != regions).unwrap_or(false) {
        // The layered overlay must expose the bar where it appears, and repaint where it left.
        unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
    }
    if lost_focus {
        // The page answered (or navigated) while a prompt button held the keyboard.
        unsafe { let _ = SetFocus(Some(hwnd)); }
        App::focus_page(app);
    }
}
