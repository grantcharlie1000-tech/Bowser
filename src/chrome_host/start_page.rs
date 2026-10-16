//! The start page's wire and the pure checks behind its commands (docs/design/start-page.md).
//!
//! - `snap` with `p:"startpage"` and `d.kind:"page"` carries everything the page draws: the speed-dial
//!   groups, the folder open and its dials, the display settings, the dashboard widgets with their
//!   data, cached favicons and the answer to the last change. Chosen thumbnails follow in
//!   `d.kind:"thumbs"` messages, each under the page's inbound limit, so the first paint never waits
//!   for an image.
//! - `{t:"spage",a:...}` commands come back (the panel family: `a`, `id`, `parent`, `before`, `n`,
//!   `on`, `text`, `url`); [`map_command`] accepts each action only with exactly its own fields.

use super::json::{capped, Json};
use super::protocol::{favicon_data_uri, snap_json, PanelFields};
use crate::bookmarks::BookmarkStore;
use crate::start_page::{
    self as model, BookmarksView, DialKind, DialView, SpeedDialConfig, StartSettings, TopSite, WidgetKind, WidgetSet,
};

const MAX_TITLE_CHARS: usize = 120;
const MAX_HOST_CHARS: usize = 64;
const MAX_URL_CHARS: usize = 400;
/// Most hosts whose cached favicon rides along, and their total data-URI bytes.
const MAX_FAV_HOSTS: usize = 80;
const MAX_FAV_BYTES: usize = 100 * 1024;
/// Largest `page` snapshot (the page refuses inbound messages over 256 KiB).
pub const MAX_SNAPSHOT_BYTES: usize = 200 * 1024;
/// Largest `thumbs` message.
pub const MAX_THUMBS_BYTES: usize = 160 * 1024;
/// Longest title, note and address the page may send.
pub const MAX_TITLE_INPUT: usize = 200;
pub const MAX_TEXT_INPUT: usize = model::MAX_NOTE_CHARS;
pub const MAX_URL_INPUT: usize = model::MAX_URL_CHARS;
/// Most folders offered in the group and widget pickers.
const MAX_FOLDERS: usize = 200;

/// How Bowser answered the page's last change. Sent once, with the snapshot that follows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Ack {
    #[default]
    None,
    Failed(String),
}

/// The blocking statistics the privacy widget shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrivacySummary {
    pub today: u64,
    pub total: u64,
    pub top_host: String,
    pub top_count: u64,
}

/// Everything one `page` snapshot reports.
pub struct Snapshot<'a> {
    /// A guest window keeps nothing, so the page offers no editing.
    pub guest: bool,
    pub settings: &'a StartSettings,
    pub view: &'a DialView,
    pub store: &'a BookmarkStore,
    /// Which bookmarks have a chosen image (`thumbs`).
    pub config: &'a SpeedDialConfig,
    pub widgets: &'a WidgetSet,
    pub top: &'a [TopSite],
    pub privacy: Option<&'a PrivacySummary>,
    pub ack: Ack,
}

/// The snapshot, with cached favicons from the profile's favicon cache.
pub fn snapshot_json(s: &Snapshot<'_>) -> String {
    snapshot_json_with(s, |host| crate::favicon::load_favicon_bytes(host).ok().flatten())
}

/// [`snapshot_json`] with an injectable favicon lookup (host to cached PNG bytes).
pub fn snapshot_json_with(s: &Snapshot<'_>, favicon_of: impl Fn(&str) -> Option<Vec<u8>>) -> String {
    let budget = MAX_FAV_BYTES;
    // Hosts of the dials first, then the widget links, in the order they appear.
    let mut hosts: Vec<String> = Vec::new();
    let mut want = |host: &str| {
        let host = capped(host, MAX_HOST_CHARS);
        if !host.is_empty() && !hosts.iter().any(|seen| seen == host) {
            hosts.push(host.to_owned());
        }
    };
    for dial in &s.view.dials {
        if let DialKind::Link { host, .. } = &dial.kind {
            want(host);
        }
    }
    if s.settings.widgets {
        for widget in &s.widgets.items {
            match widget.kind {
                WidgetKind::TopSites => s.top.iter().for_each(|site| want(&site.host)),
                WidgetKind::Bookmarks => model::widget_links(s.store, widget.folder).iter().for_each(|link| want(&link.3)),
                _ => {}
            }
        }
    }
    let mut favs: Vec<(String, String)> = Vec::new();
    let mut total = 0;
    for host in &hosts {
        if favs.len() == MAX_FAV_HOSTS {
            break;
        }
        let Some(uri) = favicon_of(host).as_deref().and_then(favicon_data_uri) else { continue };
        let cost = uri.len() + host.len() + 6;
        if total + cost > budget {
            continue;
        }
        total += cost;
        favs.push((host.clone(), uri));
    }
    // One pass. Only an extreme page (a hundred long titles and addresses, two hundred folders) is over the
    // limit with its icons, and then it goes without them.
    let json = build(s, &favs);
    if json.len() > MAX_SNAPSHOT_BYTES && !favs.is_empty() { build(s, &[]) } else { json }
}

fn build(s: &Snapshot<'_>, favs: &[(String, String)]) -> String {
    let view = s.view;
    let settings = s.settings;
    snap_json("startpage", |out| {
        out.string("kind", "page");
        out.bool("guest", s.guest);
        out.object("settings", |o| {
            o.uint("columns", u64::from(settings.columns));
            o.uint("thumbSize", u64::from(settings.thumb_size));
            o.string("titles", settings.titles.key());
            o.bool("addButton", settings.add_button);
            o.bool("dragReorder", settings.drag_reorder);
            o.bool("widgets", settings.widgets);
            o.uint("widgetColumns", u64::from(settings.widget_columns));
            o.string("widgetHeight", settings.widget_height.key());
            o.string("widgetBg", settings.widget_bg.key());
            o.string("widgetColor", &settings.widget_color);
            o.bool("widgetDrag", settings.widget_drag);
        });
        out.array("groups", |a| {
            for group in &view.groups {
                a.object(|o| {
                    o.uint("id", group.id);
                    o.string("title", capped(&group.title, MAX_TITLE_CHARS));
                    o.uint("n", group.count as u64);
                });
            }
        });
        out.uint("group", view.group);
        out.uint("folder", view.folder);
        out.array("path", |a| {
            for crumb in &view.path {
                a.object(|o| {
                    o.uint("id", crumb.id);
                    o.string("title", capped(&crumb.title, MAX_TITLE_CHARS));
                });
            }
        });
        out.array("dials", |a| {
            for dial in &view.dials {
                a.object(|o| {
                    o.uint("id", dial.id);
                    o.string("title", capped(&dial.title, MAX_TITLE_CHARS));
                    match &dial.kind {
                        DialKind::Link { url, host } => {
                            o.string("kind", "link");
                            o.string("url", capped(url, MAX_URL_CHARS));
                            o.string("host", capped(host, MAX_HOST_CHARS));
                            o.uint("n", 0);
                        }
                        DialKind::Folder { count } => {
                            o.string("kind", "folder");
                            o.string("url", "");
                            o.string("host", "");
                            o.uint("n", *count as u64);
                        }
                    }
                    o.bool("thumb", dial.has_thumb);
                });
            }
        });
        out.bool("more", view.more);
        out.array("pickable", |a| {
            for choice in view.choices.iter().take(MAX_FOLDERS) {
                a.object(|o| {
                    o.uint("id", choice.id);
                    o.string("path", capped(&choice.path, MAX_TITLE_CHARS));
                });
            }
        });
        out.array("folders", |a| {
            for item in s.store.items().iter().filter(|i| matches!(i.kind, crate::bookmarks::BookmarkKind::Folder)).take(MAX_FOLDERS) {
                a.object(|o| {
                    o.uint("id", item.id);
                    o.string("title", capped(&item.title, MAX_TITLE_CHARS));
                });
            }
        });
        out.array("widgets", |a| {
            if !settings.widgets {
                return;
            }
            for widget in &s.widgets.items {
                a.object(|o| {
                    o.uint("id", u64::from(widget.id));
                    o.string("kind", widget.kind.key());
                    match widget.kind {
                        WidgetKind::TopSites => o.array("sites", |rows| {
                            for site in s.top.iter().take(model::MAX_WIDGET_ROWS) {
                                rows.object(|row| {
                                    row.string("host", capped(&site.host, MAX_HOST_CHARS));
                                    row.string("title", capped(site.title.trim(), MAX_TITLE_CHARS));
                                    row.string("url", capped(&site.url, MAX_URL_CHARS));
                                });
                            }
                        }),
                        WidgetKind::Bookmarks => {
                            o.uint("folder", widget.folder);
                            o.string("view", widget.view.key());
                            o.array("links", |rows| {
                                for (id, title, url, host) in model::widget_links(s.store, widget.folder) {
                                    rows.object(|row| {
                                        row.uint("id", id);
                                        row.string("title", capped(&title, MAX_TITLE_CHARS));
                                        row.string("url", capped(&url, MAX_URL_CHARS));
                                        row.string("host", capped(&host, MAX_HOST_CHARS));
                                        row.bool("thumb", s.config.thumbs.contains_key(&id));
                                    });
                                }
                            });
                        }
                        WidgetKind::Notes => o.string("text", capped(&widget.text, MAX_TEXT_INPUT)),
                        WidgetKind::Date => {}
                        WidgetKind::Privacy => {
                            let stats = s.privacy.cloned().unwrap_or_default();
                            o.uint("today", stats.today);
                            o.uint("total", stats.total);
                            o.string("topHost", capped(&stats.top_host, MAX_HOST_CHARS));
                            o.uint("topCount", stats.top_count);
                        }
                        WidgetKind::Webpage => o.string("url", capped(&widget.url, MAX_URL_INPUT)),
                    }
                });
            }
        });
        out.object("favs", |o| {
            for (host, uri) in favs {
                o.string(host, uri);
            }
        });
        match &s.ack {
            Ack::None => {
                out.string("ack", "none");
                out.string("error", "");
            }
            Ack::Failed(error) => {
                out.string("ack", "failed");
                out.string("error", capped(error, 200));
            }
        }
    })
    .unwrap_or_else(|| Json::object(|_| {}))
}

/// The chosen thumbnails as `thumbs` messages, each at most [`MAX_THUMBS_BYTES`]. `items` are
/// `(dial id, data URI)`; a single image over the limit is skipped.
pub fn thumbs_messages(items: &[(u64, String)]) -> Vec<String> {
    let mut messages = Vec::new();
    let mut chunk: Vec<&(u64, String)> = Vec::new();
    let mut size = 0;
    let flush = |chunk: &mut Vec<&(u64, String)>, messages: &mut Vec<String>| {
        if chunk.is_empty() {
            return;
        }
        let json = snap_json("startpage", |out| {
            out.string("kind", "thumbs");
            out.object("thumbs", |o| {
                for (id, uri) in chunk.iter().map(|item| (&item.0, &item.1)) {
                    o.string(&id.to_string(), uri);
                }
            });
        })
        .unwrap_or_default();
        messages.push(json);
        chunk.clear();
    };
    for item in items {
        let cost = item.1.len() + 24;
        if cost > MAX_THUMBS_BYTES {
            continue;
        }
        if size + cost > MAX_THUMBS_BYTES || chunk.len() >= 100 {
            flush(&mut chunk, &mut messages);
            size = 0;
        }
        size += cost;
        chunk.push(item);
    }
    flush(&mut chunk, &mut messages);
    messages
}

/// A validated command from the start page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Open dial `id` here, or in a new tab (Ctrl or middle click).
    Open { id: u64, new_tab: bool },
    /// Open an address from a widget (top site, bookmark): http(s) only.
    OpenUrl { url: String, new_tab: bool },
    /// Show folder `id` (a group, or a folder inside one).
    Show(u64),
    AddGroup(String),
    /// Flag existing bookmark folder `id` as a group.
    FlagGroup(u64),
    RenameGroup { id: u64, title: String },
    /// Stop showing group `id` (the bookmark folder stays).
    RemoveGroup(u64),
    MoveGroup { id: u64, before: Option<u64> },
    Add { folder: u64, title: String, url: String },
    AddFolder { folder: u64, title: String },
    Edit { id: u64, title: String, url: String },
    Remove(u64),
    Move { id: u64, before: Option<u64> },
    MoveInto { id: u64, folder: u64 },
    /// Choose an image for dial `id` (native picker).
    Thumb(u64),
    ThumbClear(u64),
    WidgetAdd(WidgetKind),
    WidgetRemove(u32),
    WidgetMove { id: u32, before: Option<u32> },
    /// The bookmarks widget lists folder `folder` (0 = the root).
    WidgetFolder { id: u32, folder: u64 },
    WidgetView { id: u32, view: BookmarksView },
    /// The webpage widget embeds `url` (empty clears it).
    WidgetUrl { id: u32, url: String },
    /// The sticky note's text, saved as it is edited.
    WidgetNote { id: u32, text: String },
    /// Save the sticky note as a note in the Notes panel.
    WidgetSaveNote(u32),
    /// Open Settings at the start page section.
    Settings,
    /// The page wants a fresh snapshot.
    Refresh,
}

fn narrow(value: u64) -> Option<u32> {
    u32::try_from(value).ok()
}

/// Maps `{t:"spage",a,...}` to an [`Action`]: each action with exactly its own fields.
pub fn map_command(f: &PanelFields) -> Option<Action> {
    let has = |name: &str| match name {
        "id" => f.id.is_some(),
        "parent" => f.parent.is_some(),
        "before" => f.before.is_some(),
        "n" => f.n.is_some(),
        "on" => f.on.is_some(),
        "text" => f.text.is_some(),
        "url" => f.url.is_some(),
        _ => false,
    };
    // Exactly the allowed fields may be present (`optional` ones may be absent).
    let only = |required: &[&str], optional: &[&str]| -> bool {
        ["id", "parent", "before", "n", "on", "text", "url"].into_iter().all(|name| {
            let present = has(name);
            if required.contains(&name) { present } else { !present || optional.contains(&name) }
        })
    };
    let text = |max: usize| f.text.as_deref().filter(|t| t.chars().count() <= max && !t.chars().any(|c| c.is_control() && c != '\n' && c != '\t'));
    let title = || text(MAX_TITLE_INPUT).map(str::trim).filter(|t| !t.is_empty());
    let web = |url: &Option<String>| url.as_deref().filter(|u| u.chars().count() <= MAX_URL_INPUT).and_then(model::web_address);
    Some(match f.a.as_str() {
        "open" if only(&["id"], &["on"]) => Action::Open { id: f.id?, new_tab: f.on.unwrap_or(false) },
        "openurl" if only(&["url"], &["on"]) => Action::OpenUrl { url: web(&f.url)?, new_tab: f.on.unwrap_or(false) },
        "show" if only(&["id"], &[]) => Action::Show(f.id?),
        "addgroup" if only(&["text"], &[]) => Action::AddGroup(title()?.to_owned()),
        "flaggroup" if only(&["id"], &[]) => Action::FlagGroup(f.id?),
        "renamegroup" if only(&["id", "text"], &[]) => Action::RenameGroup { id: f.id?, title: title()?.to_owned() },
        "removegroup" if only(&["id"], &[]) => Action::RemoveGroup(f.id?),
        "movegroup" if only(&["id"], &["before"]) => Action::MoveGroup { id: f.id?, before: f.before },
        "add" if only(&["parent", "url"], &["text"]) => {
            // An empty title is allowed: Bowser names the dial after its host.
            let title = match &f.text {
                None => String::new(),
                Some(_) => text(MAX_TITLE_INPUT)?.trim().to_owned(),
            };
            Action::Add { folder: f.parent?, title, url: web(&f.url)? }
        }
        "addfolder" if only(&["parent", "text"], &[]) => Action::AddFolder { folder: f.parent?, title: title()?.to_owned() },
        "edit" if only(&["id", "text"], &["url"]) => Action::Edit {
            id: f.id?,
            title: title()?.to_owned(),
            url: if f.url.is_some() { web(&f.url)? } else { String::new() },
        },
        "remove" if only(&["id"], &[]) => Action::Remove(f.id?),
        "move" if only(&["id"], &["before"]) => Action::Move { id: f.id?, before: f.before },
        "movein" if only(&["id", "parent"], &[]) => Action::MoveInto { id: f.id?, folder: f.parent? },
        "thumb" if only(&["id"], &[]) => Action::Thumb(f.id?),
        "thumbclear" if only(&["id"], &[]) => Action::ThumbClear(f.id?),
        "wadd" if only(&["text"], &[]) => Action::WidgetAdd(WidgetKind::parse(f.text.as_deref()?)?),
        "wremove" if only(&["id"], &[]) => Action::WidgetRemove(narrow(f.id?)?),
        "wmove" if only(&["id"], &["before"]) => Action::WidgetMove { id: narrow(f.id?)?, before: match f.before { Some(b) => Some(narrow(b)?), None => None } },
        "wfolder" if only(&["id", "parent"], &[]) => Action::WidgetFolder { id: narrow(f.id?)?, folder: f.parent? },
        "wview" if only(&["id", "text"], &[]) => Action::WidgetView { id: narrow(f.id?)?, view: BookmarksView::parse(f.text.as_deref()?)? },
        "wurl" if only(&["id", "url"], &[]) => {
            let url = f.url.as_deref()?;
            Action::WidgetUrl { id: narrow(f.id?)?, url: if url.is_empty() { String::new() } else { web(&f.url)? } }
        }
        "wnote" if only(&["id", "text"], &[]) => Action::WidgetNote { id: narrow(f.id?)?, text: text(MAX_TEXT_INPUT)?.to_owned() },
        "wsavenote" if only(&["id"], &[]) => Action::WidgetSaveNote(narrow(f.id?)?),
        "settings" if only(&[], &[]) => Action::Settings,
        "refresh" if only(&[], &[]) => Action::Refresh,
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
