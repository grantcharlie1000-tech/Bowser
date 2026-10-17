//! Opening the Sessions panel (saved sessions: save the open tabs or the selected ones, open one,
//! delete one). The panel itself is an HTML page in the dialog host (`html_dialog::Kind::Sessions`,
//! `ui/src/panels/sessions.ts`); without the HTML chrome the older Saved sessions menu is what opens.

use windows::Win32::Foundation::HWND;

use crate::app::AppRef;

/// Opens the panel, or brings the open one to the front.
pub(crate) fn open(hwnd: HWND, app: &AppRef) {
    if super::html_dialog::html_only(hwnd)
        && let Some(env) = super::window::html_environment(hwnd)
    {
        super::html_dialog::open_sessions(hwnd, app, env);
    } else {
        super::window::show_sessions(hwnd, app);
    }
}

/// The saved sessions changed from outside the panel: an open panel redraws.
pub(crate) fn refresh(_hwnd: HWND) {
    super::html_dialog::refresh_sessions();
}
