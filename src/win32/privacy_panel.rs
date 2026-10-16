//! Profile-scoped Privacy dashboard, drawn with Bowser's themed menu components.

use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use windows::Win32::{
    Foundation::{HWND, POINT, RECT},
    UI::WindowsAndMessaging::{
        DispatchMessageW, GetWindowRect, MSG, PM_REMOVE, PeekMessageW, TranslateMessage, WM_QUIT,
    },
};

use super::{
    menu::{self, Entry, Icon, Item, Menu, ProfileHeader},
    show_error_message,
};
use crate::{
    app::{App, AppRef},
    browsing_data::{self, DataKind, DeleteReport, Range, Selection},
    permissions::{Decision, Permission},
    privacy_stats::PrivacyStats,
    site_security::SiteSecurity,
};

const CLEAR_STATS: usize = 8_800;
const CLEAR_DATA_BASE: usize = 8_900;
/// Ranges per kind in the menu ids (`browsing_data::Range::ALL` has five).
const CLEAR_RANGES: usize = 8;
const PERMISSION_BASE: usize = 10_000;
const PERMISSION_CHOICES: [Option<Decision>; 3] =
    [Some(Decision::Allow), Some(Decision::Block), None];
const PERMISSION_LABELS: [&str; 3] = ["Allow", "Block", "Clear choice (blocked by default)"];

pub(crate) struct Snapshot {
    pub(crate) profile: String,
    pub(crate) guest: bool,
    pub(crate) stats: PrivacyStats,
    pub(crate) session_total: u64,
    pub(crate) host: Option<String>,
    pub(crate) site_total: u64,
    pub(crate) top_sites: Vec<(String, u64)>,
    pub(crate) days: Vec<(String, u64)>,
    pub(crate) security: SiteSecurity,
    pub(crate) blocking: Option<bool>,
    pub(crate) tracking: Option<&'static str>,
    /// The level Settings will apply on the next start.
    pub(crate) tracking_setting: &'static str,
    pub(crate) efficiency_active: bool,
    pub(crate) smart_screen: Option<bool>,
    pub(crate) blocker_setting: bool,
    pub(crate) sleeping: bool,
    pub(crate) sleep_after: u64,
    pub(crate) permissions: Vec<(String, Permission, Decision)>,
    pub(crate) status: Option<String>,
}

pub(crate) fn open(hwnd: HWND, app: &AppRef) {
    // The dashboard is a section of the settings page, not a window of its own.
    if super::settings_page::usable(hwnd) {
        super::settings_page::open(hwnd, app, Some(crate::internal_page::Section::Privacy), false);
        return;
    }
    if super::html_dialog::html_only(hwnd)
        && let Some(env) = super::window::html_environment(hwnd)
    {
        super::html_dialog::open_privacy(hwnd, app, env);
        return;
    }
    let profile = crate::profile::active().ok();
    let mut status = None;
    loop {
        let snapshot = gather(app, profile.as_ref(), status.take());
        let Some(chosen) = menu::track(hwnd, anchor(hwnd), panel(&snapshot)) else {
            return;
        };
        if apply(hwnd, app, &snapshot, chosen.id) {
            status = Some(if chosen.id == CLEAR_STATS {
                "Statistics cleared".to_owned()
            } else {
                "Permission updated".to_owned()
            });
        }
    }
}

pub(crate) fn gather(
    app: &AppRef,
    profile: Option<&crate::profile::Profile>,
    status: Option<String>,
) -> Snapshot {
    let (stats, session_total) = App::privacy_stats_snapshot(app);
    let host = App::active_site(app).and_then(|(_, url)| url.host_str().map(str::to_owned));
    let site_total = host.as_deref().map_or(0, |host| stats.host_total(host));
    let top_sites = stats
        .top_sites(5)
        .into_iter()
        .map(|(host, count)| (host.to_owned(), count))
        .collect();
    let days = stats
        .days()
        .rev()
        .take(7)
        .map(|(day, count)| (day.to_owned(), count))
        .collect();
    let tracking = App::active_site(app).and_then(|(tab, _)| {
        App::call_engine(app, |engine| engine.tracking_prevention_level(tab)).flatten()
    });
    let smart_screen = App::active_site(app).and_then(|(tab, _)| {
        App::call_engine(app, |engine| engine.smart_screen_enabled(tab)).flatten()
    });
    let permissions = App::permissions_handle(app)
        .map(|store| store.borrow().all_decisions())
        .unwrap_or_default();
    let (blocker_setting, sleeping, sleep_after) = App::privacy_settings_snapshot(app);
    Snapshot {
        profile: profile.map_or_else(
            || "Bowser".to_owned(),
            |profile| profile.listed_name().to_owned(),
        ),
        guest: profile.is_some_and(crate::profile::Profile::is_guest),
        stats,
        session_total,
        host,
        site_total,
        top_sites,
        days,
        security: App::site_security(app),
        blocking: App::blocker_active_for_site(app),
        tracking,
        tracking_setting: App::tracking_setting(app),
        efficiency_active: App::efficiency_active(app),
        smart_screen,
        blocker_setting,
        sleeping,
        sleep_after,
        permissions,
        status,
    }
}

fn panel(snapshot: &Snapshot) -> Menu {
    let mut entries = vec![
        Entry::SectionHeader("Blocked requests".to_owned()),
        Entry::Item(
            Item::new(
                0,
                format!("All time: {}", snapshot.stats.all_time_total()),
                Icon::Shield,
            )
            .disabled(),
        ),
        Entry::Item(
            Item::new(
                0,
                format!("This session: {}", snapshot.session_total),
                Icon::Info,
            )
            .disabled(),
        ),
        Entry::Item(
            Item::new(
                0,
                format!("Current site (if tracked): {}", snapshot.site_total),
                Icon::Info,
            )
            .disabled_if(snapshot.host.is_none()),
        ),
        Entry::Item(
            Item::new(
                0,
                "Counts include all blocked requests; Bowser does not separate ads from trackers",
                Icon::Info,
            )
            .disabled(),
        ),
    ];
    entries.push(Entry::SectionHeader(
        "Top tracked sites with blocked requests".to_owned(),
    ));
    entries.push(Entry::Item(
        Item::new(
            0,
            "Per-site history tracks up to 128 host names",
            Icon::Info,
        )
        .disabled(),
    ));
    if snapshot.top_sites.is_empty() {
        entries.push(Entry::Item(
            Item::new(0, "No blocked requests recorded", Icon::Info).disabled(),
        ));
    } else {
        entries.extend(snapshot.top_sites.iter().map(|(host, count)| {
            Entry::Item(Item::new(0, format!("{host}: {count}"), Icon::Shield).disabled())
        }));
    }
    entries.push(Entry::SectionHeader(
        "Daily totals (last 7 recorded days)".to_owned(),
    ));
    if snapshot.days.is_empty() {
        entries.push(Entry::Item(
            Item::new(0, "No daily counts recorded", Icon::Info).disabled(),
        ));
    } else {
        entries.extend(snapshot.days.iter().map(|(day, count)| {
            Entry::Item(Item::new(0, format!("{day}: {count}"), Icon::Info).disabled())
        }));
    }
    entries.push(Entry::Item(Item::new(
        CLEAR_STATS,
        "Clear statistics…",
        Icon::Reset,
    )));

    entries.push(Entry::SectionHeader("Protection for this site".to_owned()));
    entries.push(Entry::Item(
        Item::new(
            0,
            format!("Connection: {}", snapshot.security.summary()),
            Icon::Lock,
        )
        .disabled(),
    ));
    let blocker_text = match snapshot.blocking {
        Some(true) => "Blocker: on for this site".to_owned(),
        Some(false) => "Blocker: off for this site".to_owned(),
        None if snapshot.blocker_setting => "Blocker: unavailable".to_owned(),
        None => "Blocker: off".to_owned(),
    };
    entries.push(Entry::Item(
        Item::new(0, blocker_text, Icon::Shield).disabled(),
    ));
    entries.push(Entry::Item(
        Item::new(
            0,
            format!(
                "Tracking prevention: {}",
                snapshot.tracking.unwrap_or("unavailable")
            ),
            Icon::Shield,
        )
        .disabled(),
    ));
    let smart_screen_label = match snapshot.smart_screen {
        Some(true) => "SmartScreen: enabled in Bowser (Windows policy can override)".to_owned(),
        Some(false) => "SmartScreen: off".to_owned(),
        None => "SmartScreen: unavailable in this runtime".to_owned(),
    };
    entries.push(Entry::Item(
        Item::new(0, smart_screen_label, Icon::Shield).disabled(),
    ));
    entries.push(Entry::Item(
        Item::new(
            0,
            if snapshot.sleeping {
                format!("Sleeping tabs: after {} seconds", snapshot.sleep_after)
            } else {
                "Sleeping tabs: off".to_owned()
            },
            Icon::Info,
        )
        .disabled(),
    ));

    entries.push(Entry::SectionHeader("Stored permissions".to_owned()));
    if snapshot.permissions.is_empty() {
        entries.push(Entry::Item(
            Item::new(0, "No stored site permissions", Icon::Info).disabled(),
        ));
    } else {
        for permission in Permission::ALL {
            entries.push(Entry::SectionHeader(permission.label().to_owned()));
            for (index, (origin, kind, decision)) in snapshot.permissions.iter().enumerate() {
                if *kind != permission {
                    continue;
                }
                let label = match decision {
                    Decision::Allow => "allowed",
                    Decision::Block => "blocked",
                };
                let choices = PERMISSION_CHOICES
                    .iter()
                    .enumerate()
                    .map(|(choice, value)| {
                        Entry::Item(
                            Item::new(
                                PERMISSION_BASE + index * PERMISSION_CHOICES.len() + choice,
                                PERMISSION_LABELS[choice],
                                if *value == Some(*decision) {
                                    Icon::Check
                                } else {
                                    Icon::None
                                },
                            )
                            .checked(*value == Some(*decision)),
                        )
                    })
                    .collect();
                entries.push(Entry::Item(
                    Item::new(0, format!("{origin}: {label}"), Icon::Person).submenu_with(choices),
                ));
            }
        }
    }

    entries.push(Entry::SectionHeader("Delete browsing data".to_owned()));
    for (kind_index, kind) in DataKind::ALL.into_iter().enumerate() {
        let ranges = Range::ALL
            .into_iter()
            .enumerate()
            .map(|(range_index, range)| {
                let label = match range {
                    Range::Hour => "Past hour",
                    Range::Day => "Past day",
                    Range::Week => "Past week",
                    Range::FourWeeks => "Past 4 weeks",
                    Range::Everything => "Everything",
                };
                Entry::Item(Item::new(
                    CLEAR_DATA_BASE + kind_index * CLEAR_RANGES + range_index,
                    label,
                    Icon::Reset,
                ))
            })
            .collect();
        let label = {
            let mut text = kind.label().to_owned();
            if let Some(first) = text.get_mut(..1) {
                first.make_ascii_uppercase();
            }
            text
        };
        entries.push(Entry::Item(
            Item::new(0, label, Icon::Cookie).submenu_with(ranges),
        ));
    }
    if let Some(status) = &snapshot.status {
        entries.push(Entry::Separator);
        entries.push(Entry::Item(Item::new(0, status, Icon::Info).disabled()));
    }
    Menu {
        header: Some(ProfileHeader {
            profile: snapshot.profile.clone(),
            caption: if snapshot.guest {
                "Guest profile · ephemeral"
            } else {
                "Privacy dashboard"
            }
            .to_owned(),
            avatar: "•".to_owned(),
            accent: crate::theme::Rgba::hex(0x2B5FA8, 255),
        }),
        entries,
        footer: "Statistics store host names only".to_owned(),
    }
}

pub(crate) fn apply(hwnd: HWND, app: &AppRef, snapshot: &Snapshot, id: usize) -> bool {
    if id == CLEAR_STATS {
        if !confirm(
            hwnd,
            "Clear privacy statistics?",
            "This removes saved counters and this session's blocked-request total.",
        ) {
            return false;
        }
        return match App::clear_privacy_statistics(app) {
            Ok(()) => true,
            Err(error) => {
                show_error_message(&format!(
                    "Bowser could not clear privacy statistics: {error}"
                ));
                false
            }
        };
    }
    if id >= PERMISSION_BASE {
        let index = (id - PERMISSION_BASE) / PERMISSION_CHOICES.len();
        let choice = (id - PERMISSION_BASE) % PERMISSION_CHOICES.len();
        let Some((origin, permission, _)) = snapshot.permissions.get(index) else {
            return false;
        };
        let Some(store) = App::permissions_handle(app) else {
            return false;
        };
        return match store
            .borrow_mut()
            .set(origin, *permission, PERMISSION_CHOICES[choice])
        {
            Ok(()) => true,
            Err(error) => {
                show_error_message(&format!("Bowser could not save the permission: {error}"));
                false
            }
        };
    }
    if (CLEAR_DATA_BASE..CLEAR_DATA_BASE + DataKind::ALL.len() * CLEAR_RANGES).contains(&id) {
        let offset = id - CLEAR_DATA_BASE;
        let kind = DataKind::ALL[offset / CLEAR_RANGES];
        let Some(range) = Range::ALL.get(offset % CLEAR_RANGES).copied() else {
            return false;
        };
        return clear_data(hwnd, app, kind, range);
    }
    false
}

fn clear_data(hwnd: HWND, app: &AppRef, kind: DataKind, range: Range) -> bool {
    let Some(selection) = Selection::from_bits(u32::from(kind.bit())) else {
        return false;
    };
    let passwords = kind == DataKind::Passwords;
    let message = if passwords {
        "Delete every saved password in this profile? Passwords have no dates, so the time range does not apply. This cannot be undone."
            .to_owned()
    } else {
        format!(
            "Delete {} from {} in this profile? Saved passwords are not included.",
            kind.label(),
            range.label()
        )
    };
    if !confirm(hwnd, "Delete browsing data?", &message) {
        return false;
    }
    // The same plan the settings page runs; the explicit confirmation above is the one the plan asks
    // for when saved passwords are chosen.
    let plan = match browsing_data::plan(selection, range, passwords) {
        Ok(plan) => plan,
        Err(error) => {
            show_result(hwnd, "Could not start", &error.to_string());
            return false;
        }
    };
    let result = Rc::new(RefCell::new(None));
    let completion = result.clone();
    App::delete_browsing_data(
        app,
        plan,
        Box::new(move |report| {
            *completion.borrow_mut() = Some(report);
        }),
    );
    match wait_for_result(&result, Duration::from_secs(60)) {
        Some(report) => show_result(
            hwnd,
            if report.ok { "Browsing data deleted" } else { "Could not delete everything" },
            &report.text,
        ),
        None => show_result(
            hwnd,
            "Still working",
            "WebView2 has not reported completion yet. The request may continue in the background.",
        ),
    }
    false
}

fn confirm(hwnd: HWND, title: &str, message: &str) -> bool {
    menu::track(
        hwnd,
        anchor(hwnd),
        Menu {
            header: Some(ProfileHeader {
                profile: title.to_owned(),
                caption: message.to_owned(),
                avatar: "?".to_owned(),
                accent: crate::theme::Rgba::hex(0xA33F5A, 255),
            }),
            entries: vec![
                Entry::Item(Item::new(1, "Clear", Icon::Reset)),
                Entry::Item(Item::new(2, "Cancel", Icon::Close)),
            ],
            footer: "This applies only to the active profile".to_owned(),
        },
    )
    .is_some_and(|chosen| chosen.id == 1)
}

fn show_result(hwnd: HWND, title: &str, message: &str) {
    let _ = menu::track(
        hwnd,
        anchor(hwnd),
        Menu {
            header: Some(ProfileHeader {
                profile: title.to_owned(),
                caption: message.to_owned(),
                avatar: "i".to_owned(),
                accent: crate::theme::Rgba::hex(0x2B5FA8, 255),
            }),
            entries: vec![Entry::Item(Item::new(1, "Close", Icon::Check))],
            footer: "Bowser".to_owned(),
        },
    );
}

fn wait_for_result(
    result: &Rc<RefCell<Option<DeleteReport>>>,
    timeout: Duration,
) -> Option<DeleteReport> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(result) = result.borrow_mut().take() {
            return Some(result);
        }
        let mut message = MSG::default();
        unsafe {
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                if message.message == WM_QUIT {
                    return None;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        std::thread::sleep(Duration::from_millis(8));
    }
    result.borrow_mut().take()
}

fn anchor(hwnd: HWND) -> POINT {
    let mut rect = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut rect);
    }
    POINT {
        x: rect.right - 28,
        y: rect.top + 42,
    }
}
