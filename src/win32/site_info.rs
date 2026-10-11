//! Site information: the panel behind the connection icon at the left of the address pill.
//!
//! Each row is read when the panel opens: the connection state from the tab's last navigation, the
//! cookies WebView2 holds for the page's address, the profile's tracking-prevention level, the blocker
//! state for the site, and the stored permission decisions. Choices write to the permission store or
//! to the browser, then the panel reopens showing what is now stored.

use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use url::Url;
use windows::Win32::{
    Foundation::{HWND, POINT},
    Graphics::Gdi::ClientToScreen,
    UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, PostQuitMessage, TranslateMessage, WM_QUIT,
    },
};

use super::{
    menu::{self, Entry, Icon, Item, Menu, ProfileHeader},
    show_error_message,
};
use crate::{
    app::{App, AppRef},
    chrome::layout::Rect,
    permissions::{self, Decision, Permission},
    site_security::SiteSecurity,
    tabs::TabId,
};

/// Command ids. A permission choice is `PERMISSION + index * 3 + choice`, where `index` follows
/// [`Permission::ALL`] and `choice` follows [`CHOICES`].
const PERMISSION: usize = 7000;
const CLEAR_COOKIES: usize = 7100;
const BLOCKER: usize = 7101;
const RESET_PERMISSIONS: usize = 7102;
/// Rows that are not commands, such as informational lines. Never chosen.
const INFO: usize = 0;

/// Allow, Block, or Default (no stored decision, so blocked). Default removes the stored line.
const CHOICES: [Option<Decision>; 3] = [Some(Decision::Allow), Some(Decision::Block), None];
const CHOICE_LABELS: [&str; 3] = ["Allow", "Block", "Default (blocked)"];

/// How long to wait for the browser to answer a cookie query before showing it as unavailable.
const QUERY_TIMEOUT: Duration = Duration::from_secs(2);

/// What the panel shows, read when it opens.
struct Snapshot {
    tab: Option<TabId>,
    url: Option<Url>,
    security: SiteSecurity,
    /// The origin permission decisions are stored under. `None` for pages that have none.
    origin: Option<String>,
    /// Stored decision per permission, in [`Permission::ALL`] order.
    decisions: [Option<Decision>; 4],
    /// Cookies sent to the page's address. `None` when the page has no cookies to ask about.
    cookies: Option<Result<usize, String>>,
    blocked: u32,
    /// `Some(true)` while the blocker acts on the site, `None` when the blocker is disabled.
    blocking: Option<bool>,
    /// The profile's tracking-prevention level as WebView2 reports it.
    tracking: Option<&'static str>,
}

/// Shows the panel below the connection icon. Choices run and the panel reopens, so it always shows
/// the stored state. It closes when the user dismisses it.
pub(crate) fn show(hwnd: HWND, app: &AppRef, button: Rect) {
    let mut anchor = POINT {
        x: button.left,
        y: button.bottom + 4,
    };
    unsafe {
        let _ = ClientToScreen(hwnd, &mut anchor);
    }
    loop {
        let snapshot = gather(app);
        let Some(chosen) = menu::track_below(hwnd, anchor, panel(&snapshot)) else {
            return;
        };
        apply(app, &snapshot, chosen.id);
    }
}

fn gather(app: &AppRef) -> Snapshot {
    let site = App::active_site(app);
    let tab = site.as_ref().map(|(tab, _)| *tab);
    let url = site.map(|(_, url)| url);
    let origin = url
        .as_ref()
        .and_then(|url| permissions::origin_of(url.as_str()));
    let decisions = match (&origin, App::permissions_handle(app)) {
        (Some(origin), Some(store)) => store.borrow().site_decisions(origin),
        _ => [None; 4],
    };
    let cookies = match (tab, url.as_ref()) {
        (Some(tab), Some(url)) if origin.is_some() => ask(|done| {
            App::call_engine(app, |engine| engine.site_cookie_count(tab, url, done)).is_some()
        })
        .or_else(|| Some(Err("The browser did not answer in time.".to_owned()))),
        _ => None,
    };
    let tracking = tab.and_then(|tab| {
        App::call_engine(app, |engine| engine.tracking_prevention_level(tab)).flatten()
    });
    Snapshot {
        tab,
        url,
        security: App::site_security(app),
        origin,
        decisions,
        cookies,
        blocked: App::active_blocked_count(app),
        blocking: App::blocker_active_for_site(app),
        tracking,
    }
}

fn panel(snapshot: &Snapshot) -> Menu {
    let host = snapshot
        .url
        .as_ref()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| "No site".to_owned());
    let caption = snapshot
        .url
        .as_ref()
        .map_or_else(|| "No page loaded".to_owned(), |url| url.origin().ascii_serialization());

    let mut entries = vec![Entry::SectionHeader("Connection".to_owned())];
    entries.push(Entry::Item(
        Item::new(INFO, snapshot.security.summary(), connection_icon(snapshot.security)).disabled(),
    ));
    entries.push(Entry::Item(
        Item::new(INFO, "Certificate details unavailable", Icon::None).disabled(),
    ));

    entries.push(Entry::SectionHeader("Permissions for this site".to_owned()));
    if snapshot.origin.is_some() {
        for (index, (permission, stored)) in Permission::ALL
            .into_iter()
            .zip(snapshot.decisions)
            .enumerate()
        {
            let state = match stored {
                Some(Decision::Allow) => "Allowed",
                Some(Decision::Block) => "Blocked",
                None => "Blocked by default",
            };
            let choices = CHOICES
                .into_iter()
                .enumerate()
                .map(|(choice, value)| {
                    let current = value == stored;
                    let item = Item::new(
                        PERMISSION + index * CHOICES.len() + choice,
                        CHOICE_LABELS[choice],
                        if current { Icon::Check } else { Icon::None },
                    );
                    Entry::Item(item.checked(current))
                })
                .collect();
            entries.push(Entry::Item(
                Item::new(INFO, format!("{}: {state}", permission.label()), permission_icon(permission))
                    .submenu_with(choices),
            ));
        }
    } else {
        entries.push(Entry::Item(
            Item::new(INFO, "Permissions need an http or https page", Icon::None).disabled(),
        ));
    }

    entries.push(Entry::SectionHeader("Site data and blocking".to_owned()));
    let cookie_label = match &snapshot.cookies {
        Some(Ok(count)) => format!("Cookies sent to this site: {count}"),
        Some(Err(_)) => "Cookies: unavailable".to_owned(),
        None => "Cookies: not applicable to this page".to_owned(),
    };
    entries.push(Entry::Item(
        Item::new(INFO, cookie_label, Icon::Cookie).disabled(),
    ));
    if matches!(snapshot.cookies, Some(Ok(count)) if count > 0) {
        entries.push(Entry::Item(Item::new(
            CLEAR_COOKIES,
            "Clear cookies for this site",
            Icon::Close,
        )));
    }
    let blocker_label = match snapshot.blocking {
        Some(true) => format!("Ad and tracker blocker: on ({} blocked here)", snapshot.blocked),
        Some(false) => "Ad and tracker blocker: off for this site".to_owned(),
        None => "Ad and tracker blocker: unavailable".to_owned(),
    };
    entries.push(Entry::Item(
        Item::new(BLOCKER, blocker_label, Icon::Shield)
            .checked(snapshot.blocking == Some(true))
            .disabled_if(snapshot.blocking.is_none()),
    ));
    let tracking_label = match snapshot.tracking {
        Some(level) => format!("Tracking prevention: {level}"),
        None => "Tracking prevention: unavailable".to_owned(),
    };
    entries.push(Entry::Item(
        Item::new(INFO, tracking_label, Icon::Shield).disabled(),
    ));

    entries.push(Entry::Separator);
    let any_stored = snapshot.decisions.iter().any(Option::is_some);
    entries.push(Entry::Item(
        Item::new(RESET_PERMISSIONS, "Reset permissions for this site", Icon::Reset)
            .disabled_if(snapshot.origin.is_none() || !any_stored),
    ));

    Menu {
        header: Some(ProfileHeader {
            profile: host,
            caption,
        }),
        entries,
        footer: "Changes apply to the next request".to_owned(),
    }
}

fn apply(app: &AppRef, snapshot: &Snapshot, id: usize) {
    match id {
        CLEAR_COOKIES => {
            let (Some(tab), Some(url)) = (snapshot.tab, snapshot.url.as_ref()) else {
                return;
            };
            match ask(|done| {
                App::call_engine(app, |engine| engine.clear_site_cookies(tab, url, done)).is_some()
            }) {
                Some(Ok(_)) => {}
                Some(Err(error)) => {
                    show_error_message(&format!("Bowser could not clear cookies for this site: {error}"))
                }
                None => show_error_message("Bowser could not clear cookies: the browser did not answer."),
            }
        }
        BLOCKER => App::toggle_blocker(app),
        RESET_PERMISSIONS => {
            let (Some(origin), Some(store)) = (snapshot.origin.as_ref(), App::permissions_handle(app))
            else {
                return;
            };
            if let Err(error) = store.borrow_mut().reset_site(origin) {
                show_error_message(&format!("Bowser could not save the permissions: {error}"));
            }
        }
        id if (PERMISSION..PERMISSION + Permission::ALL.len() * CHOICES.len()).contains(&id) => {
            let offset = id - PERMISSION;
            let permission = Permission::ALL[offset / CHOICES.len()];
            let decision = CHOICES[offset % CHOICES.len()];
            let (Some(origin), Some(store)) = (snapshot.origin.as_ref(), App::permissions_handle(app))
            else {
                return;
            };
            if let Err(error) = store.borrow_mut().set(origin, permission, decision) {
                show_error_message(&format!("Bowser could not save the permission: {error}"));
            }
        }
        _ => {}
    }
}

fn connection_icon(state: SiteSecurity) -> Icon {
    match state {
        SiteSecurity::Secure => Icon::Lock,
        SiteSecurity::NotSecure => Icon::Unlock,
        SiteSecurity::CertificateProblem => Icon::Warning,
        SiteSecurity::Internal | SiteSecurity::Unknown | SiteSecurity::NoSite => Icon::Info,
    }
}

fn permission_icon(permission: Permission) -> Icon {
    match permission {
        Permission::Microphone => Icon::Mic,
        Permission::Camera => Icon::Camera,
        Permission::Geolocation => Icon::Location,
        Permission::Notifications => Icon::Bell,
    }
}

/// Starts a browser request and runs queued messages until its answer arrives. `start` returns false
/// when the request could not be started, which gives `None` at once. `None` also means the answer
/// did not arrive within [`QUERY_TIMEOUT`].
fn ask<T: 'static>(start: impl FnOnce(Box<dyn FnOnce(T)>) -> bool) -> Option<T> {
    let slot: Rc<RefCell<Option<T>>> = Rc::new(RefCell::new(None));
    let writer = slot.clone();
    if !start(Box::new(move |value| *writer.borrow_mut() = Some(value))) {
        return None;
    }
    let deadline = Instant::now() + QUERY_TIMEOUT;
    loop {
        if let Some(value) = slot.borrow_mut().take() {
            return Some(value);
        }
        if Instant::now() >= deadline || !pump() {
            return None;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Dispatches queued messages. Returns false when the app is quitting, after re-posting `WM_QUIT`.
fn pump() -> bool {
    let mut msg = MSG::default();
    while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
        if msg.message == WM_QUIT {
            unsafe { PostQuitMessage(msg.wParam.0 as i32) };
            return false;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    true
}
