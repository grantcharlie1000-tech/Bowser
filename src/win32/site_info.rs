//! Site information: the panel behind the connection icon at the left of the address pill.
//!
//! Each row is read when the panel opens: the connection state from the tab's last navigation, the
//! cookies WebView2 holds for the page's address, the profile's tracking-prevention level, the blocker
//! state for the site, and the stored permission decisions. Choices write to the permission store or
//! to the browser, then the panel reopens showing what is now stored.
//!
//! With the HTML chrome the panel is a dedicated page in the popup host (`{t:"siteinfo"}`): the
//! controls send `act` messages, Rust applies them here and re-sends the page, and the popup stays
//! open. NATIVE FALLBACK: when the popup host is not ready (or the HTML chrome is off) the panel is
//! the menu built by `panel`, shown with `menu::show`.
//!
//! Policy (unchanged): a permission is granted only by a stored Allow. Default means blocked, and
//! Bowser never prompts.

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
    permissions::{self, Decision, PERMISSION_COUNT, Permission},
    site_security::SiteSecurity,
    tabs::TabId,
};

/// Command ids. A permission choice is `PERMISSION + index * 3 + choice`, where `index` follows
/// [`Permission::ALL`] and `choice` follows [`CHOICES`].
const PERMISSION: usize = 7000;
const CLEAR_COOKIES: usize = 7100;
const RESET_PERMISSIONS: usize = 7102;
const RESET_ZOOM: usize = 7103;
/// The shield's blocking level for this site (thread T3): `BLOCKER_LEVEL + n`, `n` following
/// [`SHIELD_LEVELS`]. Cookie notices on this site: `COOKIE_NOTICES + n`, 0 show them, 1 hide them.
const BLOCKER_LEVEL: usize = 7110;
const COOKIE_NOTICES: usize = 7120;

/// The levels the shield offers, in the order of its choices.
const SHIELD_LEVELS: [crate::blocker::Level; 3] =
    [crate::blocker::Level::None, crate::blocker::Level::Trackers, crate::blocker::Level::Ads];
const SHIELD_LEVEL_LABELS: [&str; 3] = ["None", "Trackers", "Trackers and ads"];
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
    decisions: [Option<Decision>; PERMISSION_COUNT],
    /// The page zoom in percent; 100 shows no zoom row.
    zoom: u16,
    /// Cookies sent to the page's address. `None` when the page has no cookies to ask about.
    cookies: Option<Result<usize, String>>,
    blocked: u32,
    /// `Some(true)` while the blocker acts on the site, `None` when the blocker is disabled.
    blocking: Option<bool>,
    /// The blocking level in force on the site (`None` when there is no blocker).
    level: Option<crate::blocker::Level>,
    /// Whether cookie notices are hidden on the site.
    notices: bool,
    /// The profile's tracking-prevention level as WebView2 reports it.
    tracking: Option<&'static str>,
    /// The tab is private: no permission is ever allowed or stored, and nothing is saved.
    private: bool,
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
    open_at(hwnd, app, anchor);
}

// --- popup surfaces (lane PP) ---
/// Act ids of the HTML page. A permission row is `PERMISSION_ROW + index` in
/// [`Permission::ALL`]; its choice follows [`CHOICES`] (0 Allow, 1 Block, 2 Default).
const PERMISSION_ROW: u32 = 1;
const ACT_CLEAR_COOKIES: u32 = 10;
const ACT_BLOCKER: u32 = 11;
const ACT_RESET: u32 = 12;
const ACT_ZOOM_RESET: u32 = 13;
/// Cookie notices on this site: 0 show them, 1 hide them (the wanted state, so a repeated click
/// never flips it back).
const ACT_NOTICES: u32 = 14;
/// Display order of the permission rows.
const ROW_ORDER: [Permission; PERMISSION_COUNT] = [
    Permission::Camera,
    Permission::Microphone,
    Permission::Geolocation,
    Permission::Notifications,
    Permission::Popups,
];

fn permission_index(permission: Permission) -> usize {
    Permission::ALL.iter().position(|p| *p == permission).unwrap_or(0)
}

fn permission_key(permission: Permission) -> &'static str {
    match permission {
        Permission::Microphone => "mic",
        Permission::Camera => "camera",
        Permission::Geolocation => "location",
        Permission::Notifications => "notifications",
        Permission::Popups => "popups",
    }
}

/// The HTML page for `snapshot`: every control the user may act on is declared with its range.
fn html_page(snapshot: &Snapshot, blocked_here: bool) -> super::html_popup::Page {
    let host = snapshot
        .url
        .as_ref()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| "No site".to_owned());
    let origin = snapshot
        .url
        .as_ref()
        .map_or_else(String::new, |url| url.origin().ascii_serialization());
    let security = crate::chrome_host::security_of(snapshot.security).as_str();
    let summary = snapshot.security.summary();
    let has_origin = snapshot.origin.is_some();
    let decisions = snapshot.decisions;
    let cookies = snapshot.cookies.clone();
    let blocking = snapshot.blocking;
    let level = snapshot.level.unwrap_or(crate::blocker::Level::None);
    let notices = snapshot.notices;
    let blocked = snapshot.blocked;
    let tracking = snapshot.tracking.unwrap_or("");
    let zoom = snapshot.zoom;
    let any_stored = decisions.iter().any(Option::is_some);
    let can_clear = matches!(cookies, Some(Ok(count)) if count > 0);
    let private = snapshot.private;
    let mut page = super::html_popup::Page::new("siteinfo", move |out| {
        out.string("host", &host);
        out.string("origin", &origin);
        out.string("security", security);
        out.string("summary", summary);
        out.array("perms", |rows| {
            // A private tab has no permission rows: every request is blocked and nothing is saved.
            if !has_origin || private {
                return;
            }
            for permission in ROW_ORDER {
                let index = permission_index(permission);
                rows.object(|row| {
                    row.uint("id", u64::from(PERMISSION_ROW) + index as u64);
                    row.string("k", permission_key(permission));
                    row.string("label", permission.label());
                    row.string("state", match decisions[index] {
                        Some(Decision::Allow) => "allow",
                        Some(Decision::Block) => "block",
                        None => "default",
                    });
                });
            }
        });
        out.object("cookies", |c| {
            match &cookies {
                Some(Ok(count)) => { c.string("state", "count"); c.uint("n", *count as u64); }
                Some(Err(_)) => { c.string("state", "unavailable"); c.uint("n", 0); }
                None => { c.string("state", "none"); c.uint("n", 0); }
            }
            c.bool("clear", can_clear);
        });
        out.object("blocker", |b| {
            b.string("state", match blocking { Some(true) => "on", Some(false) => "off", None => "unavailable" });
            b.uint("n", u64::from(blocked));
            // The level in force on the site and whether its cookie notices are hidden (thread T3).
            b.string("level", level.key());
            b.bool("notices", notices);
        });
        out.string("tracking", tracking);
        out.uint("zoom", u64::from(zoom));
        out.bool("reset", has_origin && any_stored && !private);
        out.bool("blockedHere", blocked_here);
        if private {
            // Optional key, sent only for a private tab.
            out.bool("private", true);
        }
    });
    if snapshot.origin.is_some() && !snapshot.private {
        for index in 0..Permission::ALL.len() {
            page.act(PERMISSION_ROW + index as u32, (CHOICES.len() - 1) as u8);
        }
        if any_stored {
            page.act(ACT_RESET, 0);
        }
    }
    if zoom != crate::site_prefs::ZOOM_DEFAULT {
        page.act(ACT_ZOOM_RESET, 0);
    }
    if can_clear {
        page.act(ACT_CLEAR_COOKIES, 0);
    }
    if snapshot.blocking.is_some() && !snapshot.private {
        // Always on in a private tab: there is nothing to choose. The blocking level is one of
        // three; the cookie notices switch is the wanted state.
        page.act(ACT_BLOCKER, (SHIELD_LEVELS.len() - 1) as u8);
        page.act(ACT_NOTICES, 1);
    }
    page
}

/// Maps a validated page action to the native command id `apply` already handles. The blocker
/// choice is the wanted state (1 on, 0 off), so a repeated click never flips it back.
fn act_command(snapshot: &Snapshot, id: u32, n: u8) -> Option<usize> {
    match id {
        ACT_CLEAR_COOKIES => Some(CLEAR_COOKIES),
        ACT_RESET => Some(RESET_PERMISSIONS),
        ACT_ZOOM_RESET => (snapshot.zoom != crate::site_prefs::ZOOM_DEFAULT).then_some(RESET_ZOOM),
        // The level is the wanted one, so choosing the level already in force does nothing.
        ACT_BLOCKER => {
            let wanted = *SHIELD_LEVELS.get(usize::from(n))?;
            snapshot.level.is_some_and(|level| level != wanted).then_some(BLOCKER_LEVEL + usize::from(n))
        }
        ACT_NOTICES => {
            (n < 2 && snapshot.level.is_some() && snapshot.notices != (n == 1)).then_some(COOKIE_NOTICES + usize::from(n))
        }
        id if (PERMISSION_ROW..PERMISSION_ROW + Permission::ALL.len() as u32).contains(&id)
            && usize::from(n) < CHOICES.len() =>
        {
            Some(PERMISSION + (id - PERMISSION_ROW) as usize * CHOICES.len() + usize::from(n))
        }
        _ => None,
    }
}

/// The HTML page; `false` when the popup host could not take it (the caller shows the menu).
fn open_html(hwnd: HWND, app: &AppRef, anchor: POINT) -> bool {
    use super::html_popup::{self, ActOutcome, Anchor, ShowRefused};
    if !html_popup::page_ready(hwnd) {
        return false;
    }
    if html_popup::is_active(hwnd) {
        html_popup::cancel(hwnd, app);
    }
    let snapshot = gather(app);
    let page = html_page(&snapshot, App::active_permission_blocked(app));
    let showing = snapshot.tab.zip(snapshot.url.as_ref().map(Url::origin));
    let state = Rc::new(RefCell::new(snapshot));
    let on_act: html_popup::ActHandler = Rc::new(move |app, id, n| {
        // The page may have changed while the panel was open: act only on the site it showed.
        if App::active_site(app).map(|(tab, url)| (tab, url.origin())) != showing {
            return ActOutcome::Close;
        }
        let command = act_command(&state.borrow(), id, n);
        if let Some(command) = command {
            let snapshot = gather_light(&state.borrow());
            apply(app, &snapshot, command);
        }
        let fresh = gather(app);
        let page = html_page(&fresh, App::active_permission_blocked(app));
        *state.borrow_mut() = fresh;
        ActOutcome::Refresh(page)
    });
    match html_popup::show_page(
        hwnd,
        Anchor::Point(anchor, menu::Placement::BelowLeft),
        "site-info",
        super::window::popup_context(hwnd),
        page,
        Box::new(|_, _| {}),
        Some(on_act),
    ) {
        Ok(()) | Err(ShowRefused::Guarded(_)) => true,
        Err(ShowRefused::Unavailable(_)) => false,
    }
}

/// A copy of `shown` for `apply` (which may pump messages), without asking the browser again.
fn gather_light(shown: &Snapshot) -> Snapshot {
    Snapshot {
        tab: shown.tab,
        url: shown.url.clone(),
        security: shown.security,
        origin: shown.origin.clone(),
        decisions: shown.decisions,
        cookies: None,
        blocked: shown.blocked,
        blocking: shown.blocking,
        level: shown.level,
        notices: shown.notices,
        tracking: shown.tracking,
        zoom: shown.zoom,
        private: shown.private,
    }
}
// --- end popup surfaces ---

// --- popup host (lane M) ---
/// One showing of the panel. A choice runs, then the panel opens again with fresh state.
fn open_at(hwnd: HWND, app: &AppRef, anchor: POINT) {
    // --- popup surfaces (lane PP) ---
    if open_html(hwnd, app, anchor) {
        return;
    }
    // NATIVE FALLBACK (HTML chrome off or popup host not ready): the panel as a menu.
    // --- end popup surfaces ---
    let snapshot = gather(app);
    let menu = panel(&snapshot);
    menu::show(hwnd, anchor, menu::Placement::BelowLeft, menu, "site-info", move |app, chosen| {
        let Some(chosen) = chosen else {
            return;
        };
        // The page may have changed while the panel was open: act only on the site it showed.
        let showing = snapshot.tab.zip(snapshot.url.as_ref().map(Url::origin));
        if App::active_site(app).map(|(tab, url)| (tab, url.origin())) != showing {
            return;
        }
        apply(app, &snapshot, chosen.id);
        open_at(hwnd, app, anchor);
    });
}
// --- end popup host ---

fn gather(app: &AppRef) -> Snapshot {
    let site = App::active_site(app);
    let tab = site.as_ref().map(|(tab, _)| *tab);
    let url = site.map(|(_, url)| url);
    let origin = url
        .as_ref()
        .and_then(|url| permissions::origin_of(url.as_str()));
    let private = tab.is_some_and(|tab| App::tab_private(app, tab));
    let decisions = match (&origin, App::permissions_handle(app)) {
        // A private tab has no stored decisions, whatever the profile holds for the site.
        (Some(origin), Some(store)) if !private => store.borrow().site_decisions(origin),
        _ => [None; PERMISSION_COUNT],
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
    // The blocking level in force on the site and its cookie notices (thread T3).
    let (level, notices) = url
        .as_ref()
        .and_then(|url| url.host_str())
        .filter(|_| origin.is_some())
        .and_then(|host| App::blocker_state_for(app, &host.to_ascii_lowercase(), private))
        .unzip();
    Snapshot {
        tab,
        url,
        security: App::site_security(app),
        origin,
        decisions,
        cookies,
        blocked: App::active_blocked_count(app),
        blocking: App::blocker_active_for_site(app),
        level,
        notices: notices.unwrap_or(false),
        tracking,
        zoom: App::active_zoom_percent(app),
        private,
    }
}

fn panel(snapshot: &Snapshot) -> Menu {
    let host = snapshot
        .url
        .as_ref()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| "No site".to_owned());
    let caption = snapshot.url.as_ref().map_or_else(
        || "No page loaded".to_owned(),
        |url| url.origin().ascii_serialization(),
    );

    let mut entries = vec![Entry::SectionHeader("Connection".to_owned())];
    entries.push(Entry::Item(
        Item::new(
            INFO,
            snapshot.security.summary(),
            connection_icon(snapshot.security),
        )
        .glyph(connection_glyph(snapshot.security))
        .disabled(),
    ));
    entries.push(Entry::Item(
        Item::new(INFO, "Certificate details unavailable", Icon::None).disabled(),
    ));

    entries.push(Entry::SectionHeader("Permissions for this site".to_owned()));
    if snapshot.private {
        for permission in Permission::ALL {
            entries.push(Entry::Item(
                Item::new(
                    INFO,
                    format!("{}: Blocked in private tabs", permission.label()),
                    permission_icon(permission),
                )
                .glyph(permission_glyph(permission))
                .disabled(),
            ));
        }
    } else if snapshot.origin.is_some() {
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
                        Icon::None,
                    );
                    Entry::Item(item.checked(current))
                })
                .collect();
            entries.push(Entry::Item(
                Item::new(
                    INFO,
                    format!("{}: {state}", permission.label()),
                    permission_icon(permission),
                )
                .glyph(permission_glyph(permission))
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
        Item::new(INFO, cookie_label, Icon::Cookie).glyph("cookie").disabled(),
    ));
    if matches!(snapshot.cookies, Some(Ok(count)) if count > 0) {
        entries.push(Entry::Item(Item::new(
            CLEAR_COOKIES,
            "Clear cookies for this site",
            Icon::Close,
        )
        .glyph("trash")));
    }
    match snapshot.level {
        Some(level) if !snapshot.private => {
            let blocker_label = format!(
                "Blocking here: {} ({} blocked requests here)",
                SHIELD_LEVEL_LABELS[SHIELD_LEVELS.iter().position(|known| *known == level).unwrap_or(0)].to_lowercase(),
                snapshot.blocked
            );
            let choices = SHIELD_LEVELS
                .into_iter()
                .enumerate()
                .map(|(n, choice)| {
                    Entry::Item(Item::new(BLOCKER_LEVEL + n, SHIELD_LEVEL_LABELS[n], Icon::None).checked(choice == level))
                })
                .collect();
            entries.push(Entry::Item(
                Item::new(INFO, blocker_label, Icon::Shield).glyph("shield").submenu_with(choices),
            ));
            let hide_next = !snapshot.notices;
            entries.push(Entry::Item(
                Item::new(
                    COOKIE_NOTICES + usize::from(hide_next),
                    if snapshot.notices { "Cookie notices: hidden on this site" } else { "Cookie notices: shown on this site" },
                    Icon::Cookie,
                )
                .glyph("cookie")
                .checked(snapshot.notices),
            ));
        }
        Some(_) => entries.push(Entry::Item(
            Item::new(
                INFO,
                format!("Blocker: always on in private tabs ({} blocked requests here)", snapshot.blocked),
                Icon::Shield,
            )
            .glyph("shield")
            .disabled(),
        )),
        None => entries.push(Entry::Item(
            Item::new(INFO, "Blocker: unavailable", Icon::Shield).glyph("shield").disabled(),
        )),
    }
    let tracking_label = match snapshot.tracking {
        Some(level) => format!("Tracking prevention: {level}"),
        None => "Tracking prevention: unavailable".to_owned(),
    };
    entries.push(Entry::Item(
        Item::new(INFO, tracking_label, Icon::Shield).glyph("eye").disabled(),
    ));

    if snapshot.zoom != crate::site_prefs::ZOOM_DEFAULT {
        entries.push(Entry::Item(
            Item::new(
                RESET_ZOOM,
                format!("Reset zoom ({}%)", snapshot.zoom),
                Icon::Reset,
            )
            .glyph("reset"),
        ));
    }
    entries.push(Entry::Separator);
    let any_stored = snapshot.decisions.iter().any(Option::is_some);
    entries.push(Entry::Item(
        Item::new(
            RESET_PERMISSIONS,
            "Reset permissions for this site",
            Icon::Reset,
        )
        .glyph("reset")
        .disabled_if(snapshot.origin.is_none() || !any_stored || snapshot.private),
    ));

    Menu {
        header: Some(ProfileHeader {
            profile: host,
            caption,
            avatar: "•".to_owned(),
            accent: crate::theme::Rgba::hex(0x2B5FA8, 255),
        }),
        entries,
        footer: if snapshot.private {
            "Private tab: nothing here is saved".to_owned()
        } else {
            "Changes apply to the next request".to_owned()
        },
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
                Some(Err(error)) => show_error_message(&format!(
                    "Bowser could not clear cookies for this site: {error}"
                )),
                None => show_error_message(
                    "Bowser could not clear cookies: the browser did not answer.",
                ),
            }
        }
        id if !snapshot.private && (BLOCKER_LEVEL..BLOCKER_LEVEL + SHIELD_LEVELS.len()).contains(&id) => {
            if App::choose_site_level(app, SHIELD_LEVELS[id - BLOCKER_LEVEL]) == crate::blocker::SiteChoice::ConfiguredInSettings {
                show_error_message("This site is exempted by blocker_allow in settings.txt. Edit that file to change it.");
            }
        }
        id if !snapshot.private && (COOKIE_NOTICES..COOKIE_NOTICES + 2).contains(&id) => {
            App::choose_site_cookie_notices(app, id == COOKIE_NOTICES + 1);
        }
        RESET_ZOOM => App::zoom_active(app, crate::site_prefs::ZoomStep::Reset),
        RESET_PERMISSIONS if snapshot.private => {}
        RESET_PERMISSIONS => {
            let (Some(origin), Some(store)) =
                (snapshot.origin.as_ref(), App::permissions_handle(app))
            else {
                return;
            };
            if let Err(error) = store.borrow_mut().reset_site(origin) {
                show_error_message(&format!("Bowser could not save the permissions: {error}"));
            }
        }
        id if !snapshot.private
            && (PERMISSION..PERMISSION + Permission::ALL.len() * CHOICES.len()).contains(&id) =>
        {
            let offset = id - PERMISSION;
            let permission = Permission::ALL[offset / CHOICES.len()];
            let decision = CHOICES[offset % CHOICES.len()];
            let (Some(origin), Some(store)) =
                (snapshot.origin.as_ref(), App::permissions_handle(app))
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

/// Family icon (`ui/src/icons.ts`) the HTML popup draws for the connection row.
fn connection_glyph(state: SiteSecurity) -> &'static str {
    match state {
        SiteSecurity::Secure => "lock",
        SiteSecurity::NotSecure => "unlock",
        SiteSecurity::CertificateProblem => "warning",
        SiteSecurity::Internal | SiteSecurity::Unknown | SiteSecurity::NoSite => "info",
    }
}

/// Family icon the HTML popup draws for a permission row.
fn permission_glyph(permission: Permission) -> &'static str {
    match permission {
        Permission::Microphone => "mic",
        Permission::Camera => "camera",
        Permission::Geolocation => "location",
        Permission::Notifications => "bell",
        Permission::Popups => "external",
    }
}

fn permission_icon(permission: Permission) -> Icon {
    match permission {
        Permission::Microphone => Icon::Mic,
        Permission::Camera => Icon::Camera,
        Permission::Geolocation => Icon::Location,
        Permission::Notifications => Icon::Bell,
        Permission::Popups => Icon::Grid,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(decisions: [Option<Decision>; PERMISSION_COUNT], cookies: Option<Result<usize, String>>, blocking: Option<bool>) -> Snapshot {
        Snapshot {
            tab: Some(TabId(1)),
            url: Url::parse("https://en.wikipedia.org/wiki/Main_Page").ok(),
            security: SiteSecurity::Secure,
            origin: Some("https://en.wikipedia.org".to_owned()),
            decisions,
            cookies,
            blocked: 3,
            blocking,
            level: blocking.map(|on| if on { crate::blocker::Level::Ads } else { crate::blocker::Level::None }),
            notices: false,
            tracking: Some("balanced"),
            zoom: 100,
            private: false,
        }
    }

    #[test]
    fn a_private_tab_page_has_no_permission_or_blocker_controls() {
        let mut shown = snapshot([None; PERMISSION_COUNT], Some(Ok(2)), Some(true));
        shown.private = true;
        let page = html_page(&shown, true);
        let json = crate::win32::html_popup::Page::debug_json(page, 9);
        assert!(json.contains("\"perms\":[]"), "{json}");
        assert!(json.contains("\"reset\":false"), "{json}");
        assert!(json.contains("\"private\":true"), "{json}");
        // Clearing the tab's own (in-memory) cookies stays available; nothing else is declared.
        assert!(json.contains("\"clear\":true"), "{json}");
        let camera = PERMISSION_ROW + permission_index(Permission::Camera) as u32;
        // The command mapping itself is unchanged: the guard is that no act is declared for the
        // page, and `apply` ignores permission and reset commands for a private snapshot.
        assert!(act_command(&shown, camera, 0).is_some());
        let normal = html_page(&snapshot([None; PERMISSION_COUNT], Some(Ok(2)), Some(true)), false);
        let normal_json = crate::win32::html_popup::Page::debug_json(normal, 9);
        assert!(!normal_json.contains("\"private\""), "{normal_json}");
    }

    #[test]
    fn page_actions_map_to_the_stored_decisions_and_never_widen_the_policy() {
        let shown = snapshot([None; PERMISSION_COUNT], Some(Ok(2)), Some(true));
        let camera = PERMISSION_ROW + permission_index(Permission::Camera) as u32;
        // Allow / Block / Default for the camera row map to the same ids the menu used.
        let camera_base = PERMISSION + permission_index(Permission::Camera) * CHOICES.len();
        assert_eq!(act_command(&shown, camera, 0), Some(camera_base));
        assert_eq!(CHOICES[act_command(&shown, camera, 0).unwrap() - camera_base], Some(Decision::Allow));
        assert_eq!(CHOICES[act_command(&shown, camera, 1).unwrap() - camera_base], Some(Decision::Block));
        assert_eq!(CHOICES[act_command(&shown, camera, 2).unwrap() - camera_base], None, "Default stores nothing: blocked");
        assert_eq!(act_command(&shown, camera, 3), None);
        assert_eq!(act_command(&shown, 99, 0), None);
        // The popups row is the fifth permission row and maps like the others.
        let popups = PERMISSION_ROW + permission_index(Permission::Popups) as u32;
        let popups_base = PERMISSION + permission_index(Permission::Popups) * CHOICES.len();
        assert_eq!(CHOICES[act_command(&shown, popups, 0).unwrap() - popups_base], Some(Decision::Allow));
        assert_eq!(CHOICES[act_command(&shown, popups, 2).unwrap() - popups_base], None, "Default stores nothing: blocked");
        // The zoom reset exists only while zoomed.
        assert_eq!(act_command(&shown, ACT_ZOOM_RESET, 0), None, "already at 100%");
        let mut zoomed = snapshot([None; PERMISSION_COUNT], None, None);
        zoomed.zoom = 125;
        assert_eq!(act_command(&zoomed, ACT_ZOOM_RESET, 0), Some(RESET_ZOOM));
        // The blocking level is the wanted level: 0 none, 1 trackers, 2 trackers and ads. The
        // sample site is at the full level, so choosing it again does nothing.
        assert_eq!(act_command(&shown, ACT_BLOCKER, 2), None, "already at that level");
        assert_eq!(act_command(&shown, ACT_BLOCKER, 0), Some(BLOCKER_LEVEL));
        assert_eq!(act_command(&shown, ACT_BLOCKER, 1), Some(BLOCKER_LEVEL + 1));
        assert_eq!(act_command(&shown, ACT_BLOCKER, 3), None, "there are three levels");
        assert_eq!(act_command(&snapshot([None; PERMISSION_COUNT], None, None), ACT_BLOCKER, 0), None, "no blocker, no choice");
        // Cookie notices: the wanted state, hidden or shown.
        assert_eq!(act_command(&shown, ACT_NOTICES, 0), None, "already shown");
        assert_eq!(act_command(&shown, ACT_NOTICES, 1), Some(COOKIE_NOTICES + 1));
        let mut hidden = snapshot([None; PERMISSION_COUNT], Some(Ok(2)), Some(true));
        hidden.notices = true;
        assert_eq!(act_command(&hidden, ACT_NOTICES, 0), Some(COOKIE_NOTICES));
        assert_eq!(act_command(&hidden, ACT_NOTICES, 1), None);
        assert_eq!(act_command(&hidden, ACT_NOTICES, 2), None);
    }

    #[test]
    fn the_shield_page_reports_the_level_the_notice_choice_and_declares_both_controls() {
        let mut shown = snapshot([None; PERMISSION_COUNT], Some(Ok(2)), Some(true));
        shown.level = Some(crate::blocker::Level::Trackers);
        shown.notices = true;
        let json = crate::win32::html_popup::Page::debug_json(html_page(&shown, false), 5);
        assert!(json.contains("\"blocker\":{\"state\":\"on\",\"n\":3,\"level\":\"trackers\",\"notices\":true}"), "{json}");
        let off = crate::win32::html_popup::Page::debug_json(html_page(&snapshot([None; PERMISSION_COUNT], None, Some(false)), false), 5);
        assert!(off.contains("\"blocker\":{\"state\":\"off\",\"n\":3,\"level\":\"none\",\"notices\":false}"), "{off}");
        let none = crate::win32::html_popup::Page::debug_json(html_page(&snapshot([None; PERMISSION_COUNT], None, None), false), 5);
        assert!(none.contains("\"blocker\":{\"state\":\"unavailable\",\"n\":3,\"level\":\"none\",\"notices\":false}"), "{none}");
        // The wire level names are the ones the page and the settings use.
        for (level, key) in [(crate::blocker::Level::None, "none"), (crate::blocker::Level::Trackers, "trackers"), (crate::blocker::Level::Ads, "ads")] {
            assert_eq!(level.key(), key);
        }
        assert_eq!(SHIELD_LEVELS.map(crate::blocker::Level::key), ["none", "trackers", "ads"]);
    }

    #[test]
    fn the_shield_menu_offers_the_three_levels_and_the_notice_switch_for_a_normal_tab_only() {
        let mut shown = snapshot([None; PERMISSION_COUNT], Some(Ok(2)), Some(true));
        shown.level = Some(crate::blocker::Level::Trackers);
        let menu = panel(&shown);
        let labels: Vec<String> = menu
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Item(item) => Some(item.label.clone()),
                _ => None,
            })
            .collect();
        assert!(labels.iter().any(|label| label == "Blocking here: trackers (3 blocked requests here)"), "{labels:?}");
        assert!(labels.iter().any(|label| label == "Cookie notices: shown on this site"), "{labels:?}");
        shown.private = true;
        let private_labels: Vec<String> = panel(&shown)
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Item(item) => Some(item.label.clone()),
                _ => None,
            })
            .collect();
        assert!(private_labels.iter().any(|label| label.starts_with("Blocker: always on in private tabs")), "{private_labels:?}");
        assert!(!private_labels.iter().any(|label| label.starts_with("Cookie notices")), "no choice to make in a private tab");
    }

    #[test]
    fn page_declares_only_the_controls_it_shows() {
        let json = crate::win32::html_popup::Page::debug_json(
            html_page(&snapshot([Some(Decision::Allow), None, None, Some(Decision::Block), None], Some(Ok(0)), Some(true)), true),
            4,
        );
        assert!(json.starts_with("{\"v\":1,\"t\":\"siteinfo\",\"tok\":4,\"kind\":\"site-info\",\"host\":\"en.wikipedia.org\""), "{json}");
        assert!(json.contains("\"security\":\"secure\""));
        assert!(json.contains("{\"id\":2,\"k\":\"camera\",\"label\":\"Camera\",\"state\":\"default\"}"), "{json}");
        assert!(json.contains("{\"id\":1,\"k\":\"mic\",\"label\":\"Microphone\",\"state\":\"allow\"}"), "{json}");
        assert!(json.contains("\"cookies\":{\"state\":\"count\",\"n\":0,\"clear\":false}"), "{json}");
        assert!(json.contains("\"reset\":true"));
        assert!(json.contains("\"blockedHere\":true"));
        assert!(json.contains("{\"id\":5,\"k\":\"popups\",\"label\":\"Popups\",\"state\":\"default\"}"), "{json}");
        assert!(json.contains("\"zoom\":100"), "{json}");
        assert!(!json.contains("\"id\":13"), "no zoom control at 100%");
        // No http(s) origin: no permission rows at all.
        let mut internal = snapshot([None; PERMISSION_COUNT], None, None);
        internal.origin = None;
        internal.url = None;
        let json = crate::win32::html_popup::Page::debug_json(html_page(&internal, false), 1);
        assert!(json.contains("\"perms\":[]"), "{json}");
        assert!(json.contains("\"reset\":false"));
    }

    #[test]
    fn site_info_page_fixture_matches_the_ui_contract() {
        let mut shown = snapshot([None, Some(Decision::Allow), None, None, Some(Decision::Allow)], Some(Ok(5)), Some(true));
        shown.zoom = 125;
        let page = html_page(&shown, false);
        let siteinfo = crate::win32::html_popup::Page::debug_json(page, 7);
        let palette_items = super::super::command_palette::html_items(
            &[(3, "Wikipedia".into(), "https://en.wikipedia.org/".into())],
            &[("Docs".into(), "https://docs.example/".into(), "Work".into())],
            &[(1, "https://news.example/".into(), "News".into())],
            &crate::productivity::browser_commands(),
        );
        let palette = crate::win32::html_popup::Page::debug_json(super::super::command_palette::html_page(&palette_items), 8);
        let fixture = format!(
            "{{\"siteinfo\":{siteinfo},\"palette\":{palette},\"act\":{{\"v\":1,\"t\":\"act\",\"tok\":7,\"id\":2,\"n\":0}}}}\n"
        );
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/test/fixtures/popup-pages.json");
        if std::env::var_os("BOWSER_WRITE_FIXTURES").is_some() {
            std::fs::write(&path, &fixture).expect("write popup pages fixture");
        }
        assert_eq!(
            std::fs::read_to_string(&path).expect("popup pages fixture"),
            fixture,
            "set BOWSER_WRITE_FIXTURES=1 cargo test site_info_page_fixture --lib"
        );
        assert!(matches!(
            crate::chrome_host::protocol::parse_command(r#"{"v":1,"t":"act","tok":7,"id":2,"n":0}"#),
            Ok(crate::chrome_host::protocol::Command::PopupAct { tok: 7, id: 2, n: 0 })
        ));
    }
}
