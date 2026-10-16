//! What the start page does with the profile's files (docs/design/start-page.md): builds the page's
//! snapshot and carries out each validated command. Pure over a folder of files, so every command is
//! tested end to end (command, file, snapshot) without a window; `win32/start_ui.rs` only adds what
//! needs a window (opening an address, the image picker, the Notes panel).

use std::{
    fs, io,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use crate::{
    bookmarks::{BookmarkKind, BookmarkStore},
    chrome_host::start_page::{self as wire, Ack, Action, PrivacySummary, Snapshot},
    privacy_stats::{self, PrivacyStats},
    productivity,
};

use super::{
    self as model, SpeedDialConfig, StartSettings, WidgetKind, WidgetSet, MAX_WIDGET_ROWS,
};

/// The profile files the page reads and writes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Paths {
    /// The profile folder: bookmarks, `speeddial.txt`, `start-widgets.txt`, thumbnails.
    pub dir: PathBuf,
    pub history: PathBuf,
    pub privacy: PathBuf,
}

impl Paths {
    pub fn in_dir(dir: &Path) -> Self {
        Self { dir: dir.to_owned(), history: dir.join("history.txt"), privacy: dir.join(privacy_stats::FILE_NAME) }
    }
}

/// Everything the page edits, read from disk.
#[derive(Clone, Debug, Default)]
pub struct Loaded {
    pub store: BookmarkStore,
    pub config: SpeedDialConfig,
    pub widgets: WidgetSet,
}

pub fn load(paths: &Paths) -> Loaded {
    Loaded {
        store: BookmarkStore::load(&paths.dir).unwrap_or_default(),
        config: SpeedDialConfig::load(&paths.dir),
        widgets: WidgetSet::load(&paths.dir),
    }
}

/// Size and modification time of each file a snapshot depends on, so an unchanged page is not rebuilt.
pub type Stamp = Vec<Option<(u64, u128)>>;

pub fn stamp(paths: &Paths) -> Stamp {
    [
        paths.dir.join("bookmarks-v2.txt"),
        paths.dir.join(model::FILE_NAME),
        paths.dir.join(model::WIDGETS_FILE),
        paths.history.clone(),
        paths.privacy.clone(),
    ]
    .iter()
    .map(|path| {
        let meta = fs::metadata(path).ok()?;
        let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_nanos();
        Some((meta.len(), modified))
    })
    .collect()
}

/// The page snapshot and the follow-up messages that carry chosen thumbnails.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Page {
    pub json: String,
    pub thumbs: Vec<String>,
}

/// Builds the page for `settings`, showing folder `requested` (or the group the settings name).
/// `paths` is `None` for a window with no profile folder (a guest): empty dials, default widgets.
pub fn page(paths: Option<&Paths>, guest: bool, settings: &StartSettings, requested: Option<u64>, ack: Ack) -> Page {
    page_with(paths, guest, settings, requested, ack, |host| crate::favicon::load_favicon_bytes(host).ok().flatten())
}

/// [`page`] with an injectable favicon lookup.
pub fn page_with(
    paths: Option<&Paths>,
    guest: bool,
    settings: &StartSettings,
    requested: Option<u64>,
    ack: Ack,
    favicon_of: impl Fn(&str) -> Option<Vec<u8>>,
) -> Page {
    let loaded = match paths {
        Some(paths) if !guest => load(paths),
        _ => Loaded::default(),
    };
    let view = model::dial_view(&loaded.store, &loaded.config, settings.group_reopen, requested);
    let widgets_on = settings.widgets;
    let has = |kind: WidgetKind| widgets_on && loaded.widgets.items.iter().any(|w| w.kind == kind);
    let top = match paths {
        Some(paths) if has(WidgetKind::TopSites) && !guest => model::top_sites(&productivity::read_history(&paths.history), MAX_WIDGET_ROWS),
        _ => Vec::new(),
    };
    let privacy = match paths {
        Some(paths) if has(WidgetKind::Privacy) && !guest => {
            let stats = PrivacyStats::load(&paths.privacy);
            let worst = stats.top_sites(1).into_iter().next().map(|(host, count)| (host.to_owned(), count));
            Some(PrivacySummary {
                today: stats.day_total(&privacy_stats::today_utc()),
                total: stats.all_time_total(),
                top_host: worst.as_ref().map(|w| w.0.clone()).unwrap_or_default(),
                top_count: worst.map_or(0, |w| w.1),
            })
        }
        _ => None,
    };
    let json = wire::snapshot_json_with(
        &Snapshot { guest, settings, view: &view, store: &loaded.store, config: &loaded.config, widgets: &loaded.widgets, top: &top, privacy: privacy.as_ref(), ack },
        favicon_of,
    );
    let thumbs = match paths {
        Some(paths) if !guest => {
            // The dials' chosen images, then the bookmarks widgets' (a bookmark has one image wherever it shows).
            let mut ids: Vec<u64> = view.dials.iter().filter(|dial| dial.has_thumb).map(|dial| dial.id).collect();
            if widgets_on {
                for widget in loaded.widgets.items.iter().filter(|w| w.kind == WidgetKind::Bookmarks) {
                    for (id, ..) in model::widget_links(&loaded.store, widget.folder) {
                        if loaded.config.thumbs.contains_key(&id) && !ids.contains(&id) {
                            ids.push(id);
                        }
                    }
                }
            }
            let items: Vec<(u64, String)> = ids
                .into_iter()
                .filter_map(|id| model::thumb_data_uri(&loaded.config, &paths.dir, id).map(|uri| (id, uri)))
                .collect();
            wire::thumbs_messages(&items)
        }
        _ => Vec::new(),
    };
    Page { json, thumbs }
}

/// What a command needs a window for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    /// Nothing more: the files changed (or did not need to) and the page gets a fresh snapshot.
    None,
    /// Open this address in the shown tab, or a new one.
    Open { url: url::Url, new_tab: bool },
    /// Ask for an image for dial `id`, then [`set_thumbnail`].
    PickThumb(u64),
    /// Add this text to the Notes panel.
    SaveNote { title: String, body: String },
    /// Open Settings at the Start page section.
    Settings,
}

/// The words for an error from the stores, in the page's voice.
pub fn friendly(error: &io::Error) -> String {
    let text = error.to_string();
    match text.as_str() {
        "bookmark already exists" => "That address is already in your bookmarks.".to_owned(),
        "too many bookmarks" => "Bookmarks are full.".to_owned(),
        "invalid bookmark title" => "That name is not allowed.".to_owned(),
        "too many groups" => format!("A start page holds at most {} groups.", model::MAX_GROUPS),
        "too many widgets" => format!("A start page holds at most {} widgets.", model::MAX_WIDGETS),
        "a speed dial needs an http or https address" => "Enter an http or https address.".to_owned(),
        "already part of a group" => "That folder is already inside a group.".to_owned(),
        "not a speed dial folder" | "not a speed dial" | "no such speed dial" | "no such place" | "not a bookmark folder" => "That is not on the start page any more.".to_owned(),
        _ => "That change could not be saved.".to_owned(),
    }
}

/// Whether `id` is a link bookmark (anywhere in the tree): the bookmarks widget lists those too.
fn is_link(store: &BookmarkStore, id: u64) -> bool {
    store.items().iter().any(|item| item.id == id && matches!(item.kind, BookmarkKind::Link { .. }))
}

fn find_link(store: &BookmarkStore, config: &SpeedDialConfig, id: u64) -> Option<url::Url> {
    let item = store.items().iter().find(|item| item.id == id)?;
    let BookmarkKind::Link { url, .. } = &item.kind else { return None };
    // Only what the page lists: a link inside a group.
    model::group_of(store, config, item.parent?)?;
    url::Url::parse(url).ok().filter(|u| matches!(u.scheme(), "http" | "https"))
}

/// Carries out `action` on the files in `paths`. `requested` is the folder the page shows (the page's
/// own state, kept by the caller): `show`, a new group and a removed group move it. Returns what the
/// window still has to do, or the words for why the change was refused.
pub fn apply(paths: &Paths, requested: &mut Option<u64>, action: Action) -> Result<Effect, String> {
    let dir = paths.dir.as_path();
    let mut loaded = load(paths);
    let err = |e: io::Error| friendly(&e);
    let Loaded { store, config, widgets } = &mut loaded;
    match action {
        Action::Open { id, new_tab } => {
            let url = find_link(store, config, id).ok_or_else(|| "That is not on the start page any more.".to_owned())?;
            return Ok(Effect::Open { url, new_tab });
        }
        Action::OpenUrl { url, new_tab } => {
            let url = url::Url::parse(&url).map_err(|_| "Enter an http or https address.".to_owned())?;
            return Ok(Effect::Open { url, new_tab });
        }
        Action::Show(id) => {
            let group = model::group_of(store, config, id).ok_or_else(|| "That is not on the start page any more.".to_owned())?;
            *requested = Some(id);
            if group == id && config.last != Some(id) {
                config.last = Some(id);
                config.save(dir).map_err(err)?;
            }
        }
        Action::AddGroup(title) => {
            let id = model::add_group(store, config, &title).map_err(err)?;
            store.save(dir).map_err(err)?;
            config.save(dir).map_err(err)?;
            *requested = Some(id);
        }
        Action::FlagGroup(id) => {
            model::flag_group(store, config, id).map_err(err)?;
            config.save(dir).map_err(err)?;
            *requested = Some(id);
        }
        Action::RenameGroup { id, title } => {
            if !config.groups.contains(&id) {
                return Err("That is not on the start page any more.".to_owned());
            }
            if !store.rename(id, &title).map_err(err)? {
                return Err("That is not on the start page any more.".to_owned());
            }
            store.save(dir).map_err(err)?;
        }
        Action::RemoveGroup(id) => {
            if !model::unflag_group(config, id) {
                return Err("That is not on the start page any more.".to_owned());
            }
            config.save(dir).map_err(err)?;
            if requested.is_some_and(|folder| model::group_of(store, config, folder).is_none()) {
                *requested = None;
            }
        }
        Action::MoveGroup { id, before } => {
            if !model::reorder_group(config, id, before) {
                return Err("That is not on the start page any more.".to_owned());
            }
            config.save(dir).map_err(err)?;
        }
        Action::Add { folder, title, url } => {
            model::add_dial(store, config, folder, &title, &url).map_err(err)?;
            store.save(dir).map_err(err)?;
        }
        Action::AddFolder { folder, title } => {
            model::add_subfolder(store, config, folder, &title).map_err(err)?;
            store.save(dir).map_err(err)?;
        }
        Action::Edit { id, title, url } => {
            model::edit_dial(store, config, id, &title, &url).map_err(err)?;
            store.save(dir).map_err(err)?;
        }
        Action::Remove(id) => {
            model::remove_dial(store, config, Some(dir), id).map_err(err)?;
            store.save(dir).map_err(err)?;
            config.save(dir).map_err(err)?;
            // The folder shown may have just been removed with its parent.
            if requested.is_some_and(|folder| model::group_of(store, config, folder).is_none()) {
                *requested = None;
            }
        }
        Action::Move { id, before } => {
            model::reorder_dial(store, config, id, before).map_err(err)?;
            store.save(dir).map_err(err)?;
        }
        Action::MoveInto { id, folder } => {
            model::move_dial_into(store, config, id, folder).map_err(err)?;
            store.save(dir).map_err(err)?;
        }
        Action::Thumb(id) => {
            if !is_link(store, id) {
                return Err("That is not on the start page any more.".to_owned());
            }
            return Ok(Effect::PickThumb(id));
        }
        Action::ThumbClear(id) => {
            if model::forget_thumb(config, Some(dir), id) {
                config.save(dir).map_err(err)?;
            }
        }
        Action::WidgetAdd(kind) => {
            widgets.add(kind).map_err(err)?;
            widgets.save(dir).map_err(err)?;
        }
        Action::WidgetRemove(id) => {
            if !widgets.remove(id) {
                return Err("That widget is gone.".to_owned());
            }
            widgets.save(dir).map_err(err)?;
        }
        Action::WidgetMove { id, before } => {
            if !widgets.reorder(id, before) {
                return Err("That widget is gone.".to_owned());
            }
            widgets.save(dir).map_err(err)?;
        }
        Action::WidgetFolder { id, folder } => {
            if folder != 0 && !store.items().iter().any(|item| item.id == folder && matches!(item.kind, BookmarkKind::Folder)) {
                return Err("That folder is gone.".to_owned());
            }
            let widget = widgets.get_mut(id).filter(|w| w.kind == WidgetKind::Bookmarks).ok_or_else(|| "That widget is gone.".to_owned())?;
            widget.folder = folder;
            widgets.save(dir).map_err(err)?;
        }
        Action::WidgetView { id, view } => {
            let widget = widgets.get_mut(id).filter(|w| w.kind == WidgetKind::Bookmarks).ok_or_else(|| "That widget is gone.".to_owned())?;
            widget.view = view;
            widgets.save(dir).map_err(err)?;
        }
        Action::WidgetUrl { id, url } => {
            let widget = widgets.get_mut(id).filter(|w| w.kind == WidgetKind::Webpage).ok_or_else(|| "That widget is gone.".to_owned())?;
            widget.url = url;
            widgets.save(dir).map_err(err)?;
        }
        Action::WidgetNote { id, text } => {
            let widget = widgets.get_mut(id).filter(|w| w.kind == WidgetKind::Notes).ok_or_else(|| "That widget is gone.".to_owned())?;
            if widget.text == text {
                return Ok(Effect::None);
            }
            widget.text = text;
            widgets.save(dir).map_err(err)?;
        }
        Action::WidgetSaveNote(id) => {
            let widget = widgets.items.iter().find(|w| w.id == id && w.kind == WidgetKind::Notes).ok_or_else(|| "That widget is gone.".to_owned())?;
            if widget.text.trim().is_empty() {
                return Err("The note is empty.".to_owned());
            }
            return Ok(Effect::SaveNote { title: model::note_title(&widget.text), body: widget.text.clone() });
        }
        Action::Settings => return Ok(Effect::Settings),
        Action::Refresh => {}
    }
    Ok(Effect::None)
}

/// Keeps a re-encoded image (a small JPEG) as dial `id`'s tile.
pub fn set_thumbnail(paths: &Paths, id: u64, jpeg: &[u8]) -> Result<(), String> {
    let mut loaded = load(paths);
    if !is_link(&loaded.store, id) {
        return Err("That is not on the start page any more.".to_owned());
    }
    model::set_thumb(&mut loaded.config, &paths.dir, id, jpeg).map_err(|_| "That image could not be used. Choose a smaller picture.".to_owned())?;
    loaded.config.save(&paths.dir).map_err(|e| friendly(&e))
}

/// Adds a sticky note's text to the profile's notes (the Notes panel's own file).
pub fn save_note(notes_file: &Path, title: &str, body: &str) -> Result<(), String> {
    let existing = productivity::read_note_list(notes_file);
    let next = existing
        .iter()
        .map(|n| n.id)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
        .max(std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_micros() as u64);
    productivity::add_note(notes_file, productivity::Note { id: next, title: title.to_owned(), body: body.to_owned() }).map_err(|e| {
        if e.to_string().contains("too many notes") { "Notes are full.".to_owned() } else { "That note could not be saved.".to_owned() }
    })
}

#[cfg(test)]
mod tests;
