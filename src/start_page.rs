//! The start page's model (docs/design/start-page.md): its settings, the speed-dial groups kept
//! beside the bookmark tree, the dashboard widgets, and the pure functions that turn them into what
//! the page shows. Nothing here touches a window, the engine or the network.
//!
//! - Speed dials are bookmarks. A *group* is a bookmark folder flagged in `speeddial.txt`; its links
//!   are the dials and its sub-folders open as sub-folders of the page. Adding, editing, removing and
//!   reordering a dial is the bookmark store's own add, update, delete and reorder, so the Bookmarks
//!   panel and the start page always agree. A dial's tile is its cached favicon, or an image the user
//!   chose (re-encoded small, `speeddial-thumbs/<id>.jpg`).
//! - Widgets are listed, ordered and configured in `start-widgets.txt`.
//! - The settings are flat `settings.txt` keys (`start_*`, `homepage`, `startup`, `startup_pages`),
//!   parsed and written by [`StartSettings`], which `settings.rs` holds as one field.

use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

use crate::bookmarks::{Bookmark, BookmarkKind, BookmarkStore};

pub const FILE_NAME: &str = "speeddial.txt";
pub const WIDGETS_FILE: &str = "start-widgets.txt";
pub const THUMB_DIR: &str = "speeddial-thumbs";
const HEADER: &str = "bowser-speeddial 1";
const WIDGETS_HEADER: &str = "bowser-start-widgets 1";
const MAX_FILE_BYTES: u64 = 128 * 1024;

pub const MIN_COLUMNS: u8 = 1;
pub const MAX_COLUMNS: u8 = 12;
pub const DEFAULT_COLUMNS: u8 = 6;
pub const MIN_THUMB_SIZE: u8 = 1;
pub const MAX_THUMB_SIZE: u8 = 5;
pub const DEFAULT_THUMB_SIZE: u8 = 3;
pub const MIN_WIDGET_COLUMNS: u8 = 1;
pub const MAX_WIDGET_COLUMNS: u8 = 4;
pub const DEFAULT_WIDGET_COLUMNS: u8 = 2;
/// Most speed-dial groups.
pub const MAX_GROUPS: usize = 24;
/// Most dials the page lists for one folder.
pub const MAX_DIALS: usize = 120;
/// Most pages `startup_pages` holds.
pub const MAX_STARTUP_PAGES: usize = 8;
/// Longest address kept for the homepage and a startup page or the webpage widget.
pub const MAX_URL_CHARS: usize = 2048;
/// Largest custom thumbnail kept (after the re-encode) and sent to the page.
pub const MAX_THUMB_BYTES: usize = 96 * 1024;
/// Most widgets, and the longest sticky note.
pub const MAX_WIDGETS: usize = 12;
pub const MAX_NOTE_CHARS: usize = 2000;
/// Most rows a widget lists.
pub const MAX_WIDGET_ROWS: usize = 12;

// ----- settings -----

/// `startup`: what a new Bowser window shows first.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Startup {
    #[default]
    StartPage,
    LastSession,
    Homepage,
    SpecificPages,
}

impl Startup {
    pub const ALL: [Self; 4] = [Self::StartPage, Self::LastSession, Self::Homepage, Self::SpecificPages];

    pub fn key(self) -> &'static str {
        match self {
            Self::StartPage => "start_page",
            Self::LastSession => "last_session",
            Self::Homepage => "homepage",
            Self::SpecificPages => "specific_pages",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.key() == value.trim().to_ascii_lowercase())
    }
}

/// `start_group_reopen`: which group the page opens on.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GroupReopen {
    #[default]
    Last,
    First,
}

impl GroupReopen {
    pub fn key(self) -> &'static str {
        match self {
            Self::Last => "last",
            Self::First => "first",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "last" => Some(Self::Last),
            "first" => Some(Self::First),
            _ => None,
        }
    }
}

/// `start_titles`: when a dial shows its title. `Auto` shows it under tiles without a chosen image
/// and on hover or focus for the rest.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TitleMode {
    #[default]
    Always,
    Never,
    Auto,
}

impl TitleMode {
    pub fn key(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::Never => "never",
            Self::Auto => "auto",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "always" => Some(Self::Always),
            "never" => Some(Self::Never),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }
}

/// `start_widget_height`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WidgetHeight {
    #[default]
    Regular,
    Tall,
}

impl WidgetHeight {
    pub fn key(self) -> &'static str {
        match self {
            Self::Regular => "regular",
            Self::Tall => "tall",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "regular" => Some(Self::Regular),
            "tall" => Some(Self::Tall),
            _ => None,
        }
    }
}

/// `start_widget_bg`: how a widget's background is drawn.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WidgetBackground {
    Solid,
    #[default]
    Translucent,
    Transparent,
    Custom,
}

impl WidgetBackground {
    pub fn key(self) -> &'static str {
        match self {
            Self::Solid => "solid",
            Self::Translucent => "translucent",
            Self::Transparent => "transparent",
            Self::Custom => "custom",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "solid" => Some(Self::Solid),
            "translucent" => Some(Self::Translucent),
            "transparent" => Some(Self::Transparent),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

/// Every start-page and startup setting, as flat `settings.txt` keys.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartSettings {
    /// `start_group_reopen=last|first` (default last).
    pub group_reopen: GroupReopen,
    /// `start_titles=always|never|auto` (default always).
    pub titles: TitleMode,
    /// `start_add_button` (default true): the "add a site" tile.
    pub add_button: bool,
    /// `start_columns`, 1 to 12 (default 6): the most dials in a row.
    pub columns: u8,
    /// `start_thumb_size`, 1 to 5 (default 3).
    pub thumb_size: u8,
    /// `start_drag_reorder` (default true).
    pub drag_reorder: bool,
    /// `homepage`: an http(s) address, or empty for none. The rail Home button opens it.
    pub homepage: String,
    /// `startup=start_page|last_session|homepage|specific_pages` (default start_page).
    pub startup: Startup,
    /// `startup_pages`: up to eight http(s) addresses separated by spaces.
    pub startup_pages: Vec<String>,
    /// `start_widgets` (default false): the dashboard under the speed dials.
    pub widgets: bool,
    /// `start_widget_columns`, 1 to 4 (default 2).
    pub widget_columns: u8,
    /// `start_widget_height=regular|tall`.
    pub widget_height: WidgetHeight,
    /// `start_widget_bg=solid|translucent|transparent|custom`.
    pub widget_bg: WidgetBackground,
    /// `start_widget_color`: `#rrggbb`, used when `widget_bg` is custom.
    pub widget_color: String,
    /// `start_widget_drag` (default true): widgets can be dragged to rearrange.
    pub widget_drag: bool,
}

impl Default for StartSettings {
    fn default() -> Self {
        Self {
            group_reopen: GroupReopen::Last,
            titles: TitleMode::Always,
            add_button: true,
            columns: DEFAULT_COLUMNS,
            thumb_size: DEFAULT_THUMB_SIZE,
            drag_reorder: true,
            homepage: String::new(),
            startup: Startup::StartPage,
            startup_pages: Vec::new(),
            widgets: false,
            widget_columns: DEFAULT_WIDGET_COLUMNS,
            widget_height: WidgetHeight::Regular,
            widget_bg: WidgetBackground::Translucent,
            widget_color: "#1f2933".to_owned(),
            widget_drag: true,
        }
    }
}

/// Every key [`StartSettings`] owns, in the order [`StartSettings::pairs`] writes them.
pub const SETTING_KEYS: [&str; 15] = [
    "start_group_reopen",
    "start_titles",
    "start_add_button",
    "start_columns",
    "start_thumb_size",
    "start_drag_reorder",
    "homepage",
    "startup",
    "startup_pages",
    "start_widgets",
    "start_widget_columns",
    "start_widget_height",
    "start_widget_bg",
    "start_widget_color",
    "start_widget_drag",
];

fn flag(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn flag_text(on: bool) -> &'static str {
    if on { "true" } else { "false" }
}

/// An http(s) address for the homepage, a startup page or the webpage widget: no whitespace or
/// control characters, at most [`MAX_URL_CHARS`], a host, and no credentials. `None` for anything else.
pub fn web_address(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > MAX_URL_CHARS || text.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let url = url::Url::parse(text).ok()?;
    let web = matches!(url.scheme(), "http" | "https");
    (web && url.host_str().is_some_and(|host| !host.is_empty()) && url.username().is_empty() && url.password().is_none())
        .then(|| url.to_string())
}

fn parse_color(value: &str) -> Option<String> {
    let value = value.trim().to_ascii_lowercase();
    let hex = value.strip_prefix('#')?;
    (hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then_some(value)
}

fn parse_pages(value: &str) -> Option<Vec<String>> {
    let mut pages = Vec::new();
    for part in value.split_whitespace() {
        if pages.len() == MAX_STARTUP_PAGES {
            return None;
        }
        pages.push(web_address(part)?);
    }
    Some(pages)
}

impl StartSettings {
    /// Applies one `key=value`, tolerantly (a value that does not parse is ignored, as
    /// `Settings::parse` does for every key). Returns whether `key` is one of ours.
    pub fn parse_key(&mut self, key: &str, value: &str) -> bool {
        match key {
            "start_group_reopen" => {
                if let Some(mode) = GroupReopen::parse(value) {
                    self.group_reopen = mode;
                }
            }
            "start_titles" => {
                if let Some(mode) = TitleMode::parse(value) {
                    self.titles = mode;
                }
            }
            "start_add_button" => {
                if let Some(on) = flag(value) {
                    self.add_button = on;
                }
            }
            "start_columns" => {
                if let Ok(n) = value.trim().parse::<u8>() {
                    self.columns = n.clamp(MIN_COLUMNS, MAX_COLUMNS);
                }
            }
            "start_thumb_size" => {
                if let Ok(n) = value.trim().parse::<u8>() {
                    self.thumb_size = n.clamp(MIN_THUMB_SIZE, MAX_THUMB_SIZE);
                }
            }
            "start_drag_reorder" => {
                if let Some(on) = flag(value) {
                    self.drag_reorder = on;
                }
            }
            "homepage" => {
                if value.trim().is_empty() {
                    self.homepage.clear();
                } else if let Some(url) = web_address(value) {
                    self.homepage = url;
                }
            }
            "startup" => {
                if let Some(mode) = Startup::parse(value) {
                    self.startup = mode;
                }
            }
            "startup_pages" => {
                if let Some(pages) = parse_pages(value) {
                    self.startup_pages = pages;
                }
            }
            "start_widgets" => {
                if let Some(on) = flag(value) {
                    self.widgets = on;
                }
            }
            "start_widget_columns" => {
                if let Ok(n) = value.trim().parse::<u8>() {
                    self.widget_columns = n.clamp(MIN_WIDGET_COLUMNS, MAX_WIDGET_COLUMNS);
                }
            }
            "start_widget_height" => {
                if let Some(height) = WidgetHeight::parse(value) {
                    self.widget_height = height;
                }
            }
            "start_widget_bg" => {
                if let Some(bg) = WidgetBackground::parse(value) {
                    self.widget_bg = bg;
                }
            }
            "start_widget_color" => {
                if let Some(color) = parse_color(value) {
                    self.widget_color = color;
                }
            }
            "start_widget_drag" => {
                if let Some(on) = flag(value) {
                    self.widget_drag = on;
                }
            }
            _ => return false,
        }
        true
    }

    /// Like [`parse_key`](Self::parse_key) but strict, for the settings page's `save` payload: a value
    /// that does not parse (or an out-of-range number) is refused. `None` for a key that is not ours.
    pub fn apply_strict(&mut self, key: &str, value: &str) -> Option<bool> {
        match key {
            "start_group_reopen" => self.group_reopen = GroupReopen::parse(value).filter(|m| m.key() == value)?,
            "start_titles" => self.titles = TitleMode::parse(value).filter(|m| m.key() == value)?,
            "start_add_button" => self.add_button = strict_flag(value)?,
            "start_columns" => self.columns = strict_range(value, MIN_COLUMNS, MAX_COLUMNS)?,
            "start_thumb_size" => self.thumb_size = strict_range(value, MIN_THUMB_SIZE, MAX_THUMB_SIZE)?,
            "start_drag_reorder" => self.drag_reorder = strict_flag(value)?,
            "homepage" => self.homepage = if value.is_empty() { String::new() } else { web_address(value)? },
            "startup" => self.startup = Startup::parse(value).filter(|m| m.key() == value)?,
            "startup_pages" => self.startup_pages = parse_pages(value)?,
            "start_widgets" => self.widgets = strict_flag(value)?,
            "start_widget_columns" => self.widget_columns = strict_range(value, MIN_WIDGET_COLUMNS, MAX_WIDGET_COLUMNS)?,
            "start_widget_height" => self.widget_height = WidgetHeight::parse(value).filter(|m| m.key() == value)?,
            "start_widget_bg" => self.widget_bg = WidgetBackground::parse(value).filter(|m| m.key() == value)?,
            "start_widget_color" => self.widget_color = parse_color(value).filter(|c| c == value)?,
            "start_widget_drag" => self.widget_drag = strict_flag(value)?,
            _ => return None,
        }
        Some(true)
    }

    /// The `key=value` pairs `settings.txt` stores, in [`SETTING_KEYS`] order.
    pub fn pairs(&self) -> Vec<(&'static str, String)> {
        vec![
            ("start_group_reopen", self.group_reopen.key().to_owned()),
            ("start_titles", self.titles.key().to_owned()),
            ("start_add_button", flag_text(self.add_button).to_owned()),
            ("start_columns", self.columns.to_string()),
            ("start_thumb_size", self.thumb_size.to_string()),
            ("start_drag_reorder", flag_text(self.drag_reorder).to_owned()),
            ("homepage", self.homepage.clone()),
            ("startup", self.startup.key().to_owned()),
            ("startup_pages", self.startup_pages.join(" ")),
            ("start_widgets", flag_text(self.widgets).to_owned()),
            ("start_widget_columns", self.widget_columns.to_string()),
            ("start_widget_height", self.widget_height.key().to_owned()),
            ("start_widget_bg", self.widget_bg.key().to_owned()),
            ("start_widget_color", self.widget_color.clone()),
            ("start_widget_drag", flag_text(self.widget_drag).to_owned()),
        ]
    }

    /// The address the rail Home button opens, when a homepage is set.
    pub fn home_target(&self) -> Option<url::Url> {
        (!self.homepage.is_empty()).then(|| url::Url::parse(&self.homepage).ok()).flatten()
    }
}

fn strict_flag(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn strict_range(value: &str, min: u8, max: u8) -> Option<u8> {
    let n = value.parse::<u8>().ok()?;
    ((min..=max).contains(&n) && n.to_string() == value).then_some(n)
}

/// What a new window does first, decided from the settings and what the launch already carries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StartupPlan {
    /// The blank first tab shows the start page (nothing to do).
    ShowStartPage,
    /// Open this address in the first tab.
    Homepage(url::Url),
    /// Open these addresses, one tab each, the first in the first tab.
    Pages(Vec<url::Url>),
}

/// The first-window plan. `launch_has_target` is true when a command-line address, a benchmark URL or a
/// restored session already decided what the window shows: then the setting stays out of the way.
/// `last_session` is handled where the session is opened (`SessionStore::open_with`), so here it is the
/// start page, which is what an empty or unrestorable session shows.
pub fn startup_plan(settings: &StartSettings, launch_has_target: bool) -> StartupPlan {
    if launch_has_target {
        return StartupPlan::ShowStartPage;
    }
    match settings.startup {
        Startup::StartPage | Startup::LastSession => StartupPlan::ShowStartPage,
        Startup::Homepage => settings.home_target().map_or(StartupPlan::ShowStartPage, StartupPlan::Homepage),
        Startup::SpecificPages => {
            let pages: Vec<url::Url> = settings.startup_pages.iter().filter_map(|page| url::Url::parse(page).ok()).collect();
            if pages.is_empty() { StartupPlan::ShowStartPage } else { StartupPlan::Pages(pages) }
        }
    }
}

// ----- speed-dial groups -----

/// The speed-dial configuration kept beside the bookmark tree.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpeedDialConfig {
    /// Bookmark folder ids flagged as groups, in tab order.
    pub groups: Vec<u64>,
    /// The group the page last showed.
    pub last: Option<u64>,
    /// Bookmark id to the file name of its chosen thumbnail inside [`THUMB_DIR`].
    pub thumbs: HashMap<u64, String>,
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n").replace('\r', "\\r")
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn read_small(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    if meta.len() > MAX_FILE_BYTES {
        return None;
    }
    String::from_utf8(fs::read(path).ok()?).ok()
}

fn atomic_write(path: &Path, text: &str) -> io::Result<()> {
    if text.len() as u64 > MAX_FILE_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "start page data exceeds its size limit"));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("tmp");
    fs::write(&temp, text)?;
    fs::rename(&temp, path).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })
}

/// A thumbnail file name is `<id>.jpg`, nothing else (so a hand-edited file cannot point elsewhere).
fn thumb_name_ok(id: u64, name: &str) -> bool {
    name == format!("{id}.jpg")
}

impl SpeedDialConfig {
    pub fn parse(text: &str) -> Self {
        let mut config = Self::default();
        let mut lines = text.lines();
        if lines.next() != Some(HEADER) {
            return config;
        }
        for line in lines {
            let mut fields = line.split('\t');
            match fields.next() {
                Some("group") => {
                    if let Some(id) = fields.next().and_then(|f| f.parse::<u64>().ok())
                        && !config.groups.contains(&id)
                        && config.groups.len() < MAX_GROUPS
                    {
                        config.groups.push(id);
                    }
                }
                Some("last") => config.last = fields.next().and_then(|f| f.parse().ok()),
                Some("thumb") => {
                    if let (Some(id), Some(name)) = (fields.next().and_then(|f| f.parse::<u64>().ok()), fields.next()) {
                        let name = unescape(name);
                        if thumb_name_ok(id, &name) {
                            config.thumbs.insert(id, name);
                        }
                    }
                }
                _ => {}
            }
        }
        config
    }

    pub fn serialize(&self) -> String {
        let mut out = format!("{HEADER}\n");
        for id in &self.groups {
            out.push_str(&format!("group\t{id}\n"));
        }
        if let Some(id) = self.last {
            out.push_str(&format!("last\t{id}\n"));
        }
        let mut thumbs: Vec<_> = self.thumbs.iter().collect();
        thumbs.sort();
        for (id, name) in thumbs {
            out.push_str(&format!("thumb\t{id}\t{}\n", escape(name)));
        }
        out
    }

    pub fn load(dir: &Path) -> Self {
        read_small(&dir.join(FILE_NAME)).map(|text| Self::parse(&text)).unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> io::Result<()> {
        atomic_write(&dir.join(FILE_NAME), &self.serialize())
    }
}

/// One group tab on the page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupTab {
    pub id: u64,
    pub title: String,
    pub count: usize,
}

/// A dial: a link, or a sub-folder that opens in place.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DialKind {
    Link { url: String, host: String },
    Folder { count: usize },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dial {
    pub id: u64,
    pub title: String,
    pub kind: DialKind,
    /// A chosen image exists for this dial.
    pub has_thumb: bool,
}

/// One step of the path from a group down to the folder shown.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Crumb {
    pub id: u64,
    pub title: String,
}

/// A bookmark folder that could become a group (not one already, not inside none).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FolderChoice {
    pub id: u64,
    pub path: String,
}

/// What the dials area shows: the group tabs, the folder open now and its dials.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DialView {
    pub groups: Vec<GroupTab>,
    /// The group the shown folder belongs to (0 when there are no groups).
    pub group: u64,
    /// The folder shown: the group itself, or a sub-folder of it.
    pub folder: u64,
    /// Group first, then each sub-folder down to `folder`.
    pub path: Vec<Crumb>,
    pub dials: Vec<Dial>,
    /// More dials than [`MAX_DIALS`] existed.
    pub more: bool,
    /// Folders the group picker offers.
    pub choices: Vec<FolderChoice>,
}

fn find(store: &BookmarkStore, id: u64) -> Option<&Bookmark> {
    store.items().iter().find(|item| item.id == id)
}

fn is_folder(item: &Bookmark) -> bool {
    matches!(item.kind, BookmarkKind::Folder)
}

/// The folders still flagged: ids that are gone, or no longer folders, drop out; order is kept.
pub fn live_groups(store: &BookmarkStore, config: &SpeedDialConfig) -> Vec<u64> {
    config.groups.iter().copied().filter(|id| find(store, *id).is_some_and(is_folder)).collect()
}

/// The group `folder` belongs to (itself when it is one), walking up the tree.
pub fn group_of(store: &BookmarkStore, config: &SpeedDialConfig, folder: u64) -> Option<u64> {
    let mut at = find(store, folder).filter(|item| is_folder(item))?;
    for _ in 0..64 {
        if config.groups.contains(&at.id) {
            return Some(at.id);
        }
        at = find(store, at.parent?)?;
    }
    None
}

fn folder_path(store: &BookmarkStore, id: u64) -> String {
    let mut parts = Vec::new();
    let mut at = find(store, id);
    for _ in 0..16 {
        let Some(item) = at else { break };
        parts.push(item.title.clone());
        at = item.parent.and_then(|p| find(store, p));
    }
    parts.reverse();
    parts.join(" / ")
}

/// Builds what the page shows. `requested` is the folder the page asked for (a group or a folder inside
/// one); anything else falls back to the group `reopen` names.
pub fn dial_view(store: &BookmarkStore, config: &SpeedDialConfig, reopen: GroupReopen, requested: Option<u64>) -> DialView {
    let groups = live_groups(store, config);
    let tabs: Vec<GroupTab> = groups
        .iter()
        .filter_map(|id| {
            let item = find(store, *id)?;
            Some(GroupTab { id: *id, title: item.title.clone(), count: store.children(Some(*id)).len() })
        })
        .collect();
    let choices: Vec<FolderChoice> = store
        .items()
        .iter()
        .filter(|item| is_folder(item) && !groups.contains(&item.id))
        .take(200)
        .map(|item| FolderChoice { id: item.id, path: folder_path(store, item.id) })
        .collect();
    let want = requested.filter(|id| group_of(store, config, *id).is_some_and(|g| groups.contains(&g)));
    let remembered = config.last.filter(|id| groups.contains(id));
    let folder = want.or(match reopen {
        GroupReopen::Last => remembered.or(groups.first().copied()),
        GroupReopen::First => groups.first().copied(),
    });
    let Some(folder) = folder else {
        return DialView { groups: tabs, choices, ..DialView::default() };
    };
    let group = group_of(store, config, folder).unwrap_or(folder);
    let mut path = Vec::new();
    let mut at = find(store, folder);
    for _ in 0..16 {
        let Some(item) = at else { break };
        path.push(Crumb { id: item.id, title: item.title.clone() });
        if item.id == group {
            break;
        }
        at = item.parent.and_then(|p| find(store, p));
    }
    path.reverse();
    let children = store.children(Some(folder));
    let more = children.len() > MAX_DIALS;
    let dials = children
        .into_iter()
        .take(MAX_DIALS)
        .map(|item| Dial {
            id: item.id,
            title: item.title.clone(),
            has_thumb: config.thumbs.contains_key(&item.id),
            kind: match &item.kind {
                BookmarkKind::Folder => DialKind::Folder { count: store.children(Some(item.id)).len() },
                BookmarkKind::Link { url, favicon_host, .. } => DialKind::Link { url: url.clone(), host: favicon_host.clone() },
            },
        })
        .collect();
    DialView { groups: tabs, group, folder, path, dials, more, choices }
}

fn bad(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.to_owned())
}

/// Whether `folder` is inside a flagged group (or is one): the only folders the page edits.
fn editable_folder(store: &BookmarkStore, config: &SpeedDialConfig, folder: u64) -> bool {
    group_of(store, config, folder).is_some()
}

/// Adds a link to `folder`. Refuses a folder outside every group and an address that is not http(s).
pub fn add_dial(store: &mut BookmarkStore, config: &SpeedDialConfig, folder: u64, title: &str, url: &str) -> io::Result<u64> {
    if !editable_folder(store, config, folder) {
        return Err(bad("not a speed dial folder"));
    }
    let url = web_address(url).ok_or_else(|| bad("a speed dial needs an http or https address"))?;
    let title = title.trim();
    let title = if title.is_empty() { url::Url::parse(&url).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_default() } else { title.to_owned() };
    store.add_bookmark(Some(folder), &title, &url)
}

/// Adds a sub-folder to `folder`.
pub fn add_subfolder(store: &mut BookmarkStore, config: &SpeedDialConfig, folder: u64, title: &str) -> io::Result<u64> {
    if !editable_folder(store, config, folder) {
        return Err(bad("not a speed dial folder"));
    }
    store.add_folder(Some(folder), title)
}

/// Edits a dial: a link changes title and address, a folder only its title.
pub fn edit_dial(store: &mut BookmarkStore, config: &SpeedDialConfig, id: u64, title: &str, url: &str) -> io::Result<()> {
    let item = find(store, id).ok_or_else(|| bad("no such speed dial"))?;
    let parent = item.parent.ok_or_else(|| bad("not a speed dial"))?;
    if !editable_folder(store, config, parent) {
        return Err(bad("not a speed dial"));
    }
    let changed = if is_folder(item) {
        store.rename(id, title)?
    } else {
        let url = web_address(url).ok_or_else(|| bad("a speed dial needs an http or https address"))?;
        store.update_bookmark(id, title, &url)?
    };
    if changed { Ok(()) } else { Err(bad("no such speed dial")) }
}

/// Deletes a dial (a bookmark, or a sub-folder with what it holds) and forgets the thumbnails of
/// everything that went with it. Returns how many bookmark items were removed.
pub fn remove_dial(store: &mut BookmarkStore, config: &mut SpeedDialConfig, dir: Option<&Path>, id: u64) -> io::Result<usize> {
    let item = find(store, id).ok_or_else(|| bad("no such speed dial"))?;
    let parent = item.parent.ok_or_else(|| bad("a group is removed with its own command"))?;
    if !editable_folder(store, config, parent) {
        return Err(bad("not a speed dial"));
    }
    let before: Vec<u64> = store.items().iter().map(|i| i.id).collect();
    let removed = store.delete(id);
    let after: Vec<u64> = store.items().iter().map(|i| i.id).collect();
    for gone in before.into_iter().filter(|b| !after.contains(b)) {
        forget_thumb(config, dir, gone);
    }
    Ok(removed)
}

/// Moves dial `id` inside its folder to just before sibling `before` (the end when `None`).
pub fn reorder_dial(store: &mut BookmarkStore, config: &SpeedDialConfig, id: u64, before: Option<u64>) -> io::Result<()> {
    let item = find(store, id).ok_or_else(|| bad("no such speed dial"))?;
    let parent = item.parent.ok_or_else(|| bad("not a speed dial"))?;
    if !editable_folder(store, config, parent) {
        return Err(bad("not a speed dial"));
    }
    let siblings: Vec<u64> = store.children(Some(parent)).iter().map(|i| i.id).filter(|s| *s != id).collect();
    let index = match before {
        None => siblings.len(),
        Some(target) => siblings.iter().position(|s| *s == target).ok_or_else(|| bad("no such place"))?,
    };
    store.move_item(id, Some(parent), index).map(|_| ())
}

/// Moves dial `id` into sub-folder or group `folder` (a drop on a folder tile), at the end.
pub fn move_dial_into(store: &mut BookmarkStore, config: &SpeedDialConfig, id: u64, folder: u64) -> io::Result<()> {
    let item = find(store, id).ok_or_else(|| bad("no such speed dial"))?;
    let parent = item.parent.ok_or_else(|| bad("not a speed dial"))?;
    if !editable_folder(store, config, parent) || !editable_folder(store, config, folder) || id == folder {
        return Err(bad("not a speed dial folder"));
    }
    let end = store.children(Some(folder)).len();
    store.move_item(id, Some(folder), end).map(|_| ())
}

/// Starts a new group: a new root bookmark folder, flagged.
pub fn add_group(store: &mut BookmarkStore, config: &mut SpeedDialConfig, title: &str) -> io::Result<u64> {
    if config.groups.len() >= MAX_GROUPS {
        return Err(bad("too many groups"));
    }
    let id = store.add_folder(None, title)?;
    config.groups.push(id);
    config.last = Some(id);
    Ok(id)
}

/// Flags an existing bookmark folder as a group.
pub fn flag_group(store: &BookmarkStore, config: &mut SpeedDialConfig, folder: u64) -> io::Result<()> {
    if !find(store, folder).is_some_and(is_folder) {
        return Err(bad("not a bookmark folder"));
    }
    if config.groups.contains(&folder) {
        return Ok(());
    }
    if config.groups.len() >= MAX_GROUPS {
        return Err(bad("too many groups"));
    }
    // A folder inside a group is already on the page; flagging it too would show it twice.
    if group_of(store, config, folder).is_some() {
        return Err(bad("already part of a group"));
    }
    config.groups.push(folder);
    config.last = Some(folder);
    Ok(())
}

/// Stops showing group `group`; the bookmark folder and its bookmarks stay where they are.
pub fn unflag_group(config: &mut SpeedDialConfig, group: u64) -> bool {
    let before = config.groups.len();
    config.groups.retain(|g| *g != group);
    if config.last == Some(group) {
        config.last = None;
    }
    config.groups.len() != before
}

/// Moves group `group` before group `before` (the end when `None`).
pub fn reorder_group(config: &mut SpeedDialConfig, group: u64, before: Option<u64>) -> bool {
    let Some(from) = config.groups.iter().position(|g| *g == group) else { return false };
    config.groups.remove(from);
    let index = match before {
        None => config.groups.len(),
        Some(target) => match config.groups.iter().position(|g| *g == target) {
            Some(index) => index,
            None => {
                config.groups.insert(from, group);
                return false;
            }
        },
    };
    config.groups.insert(index, group);
    true
}

// ----- thumbnails -----

/// The path of dial `id`'s thumbnail file.
pub fn thumb_path(dir: &Path, id: u64) -> PathBuf {
    dir.join(THUMB_DIR).join(format!("{id}.jpg"))
}

/// Records a chosen image for dial `id` (already re-encoded small: at most [`MAX_THUMB_BYTES`]).
pub fn set_thumb(config: &mut SpeedDialConfig, dir: &Path, id: u64, jpeg: &[u8]) -> io::Result<()> {
    if jpeg.len() > MAX_THUMB_BYTES || jpeg.len() < 4 || jpeg[..2] != [0xFF, 0xD8] {
        return Err(bad("the image is not a small JPEG"));
    }
    let path = thumb_path(dir, id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, jpeg)?;
    config.thumbs.insert(id, format!("{id}.jpg"));
    Ok(())
}

/// Forgets dial `id`'s chosen image and deletes its file.
pub fn forget_thumb(config: &mut SpeedDialConfig, dir: Option<&Path>, id: u64) -> bool {
    let had = config.thumbs.remove(&id).is_some();
    if let Some(dir) = dir {
        let _ = fs::remove_file(thumb_path(dir, id));
    }
    had
}

/// Deletes the chosen images, and their `thumb` lines, of bookmarks no longer in `store`. Bookmarks
/// are deleted from many places (the Bookmarks panel and bar, the star, the palette), so
/// [`BookmarkStore::save`] calls this; without thumbnails it costs one failed directory open.
/// `speeddial.txt` is only rewritten when it was read and named a deleted bookmark.
pub fn forget_thumbs_of_deleted(dir: &Path, store: &BookmarkStore) {
    let Ok(entries) = fs::read_dir(dir.join(THUMB_DIR)) else { return };
    let live: std::collections::HashSet<u64> = store.items().iter().map(|item| item.id).collect();
    let mut gone = false;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(id) = name.to_str().and_then(|n| n.strip_suffix(".jpg")).and_then(|n| n.parse::<u64>().ok()) else { continue };
        if !live.contains(&id) && fs::remove_file(entry.path()).is_ok() {
            gone = true;
        }
    }
    if !gone {
        return;
    }
    let Some(text) = read_small(&dir.join(FILE_NAME)) else { return };
    let mut config = SpeedDialConfig::parse(&text);
    let before = config.thumbs.len();
    config.thumbs.retain(|id, _| live.contains(id));
    if config.thumbs.len() != before {
        let _ = config.save(dir);
    }
}

/// Dial `id`'s chosen image as a `data:` URI, when it exists and is a small JPEG.
pub fn thumb_data_uri(config: &SpeedDialConfig, dir: &Path, id: u64) -> Option<String> {
    config.thumbs.get(&id)?;
    let bytes = fs::read(thumb_path(dir, id)).ok()?;
    if bytes.len() > MAX_THUMB_BYTES || bytes.len() < 4 || bytes[..2] != [0xFF, 0xD8] {
        return None;
    }
    Some(format!("data:image/jpeg;base64,{}", crate::chrome_host::protocol::base64_encode(&bytes)))
}

// ----- widgets -----

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WidgetKind {
    TopSites,
    Bookmarks,
    Notes,
    Date,
    Privacy,
    Webpage,
}

impl WidgetKind {
    pub const ALL: [Self; 6] = [Self::TopSites, Self::Bookmarks, Self::Notes, Self::Date, Self::Privacy, Self::Webpage];

    pub fn key(self) -> &'static str {
        match self {
            Self::TopSites => "topsites",
            Self::Bookmarks => "bookmarks",
            Self::Notes => "notes",
            Self::Date => "date",
            Self::Privacy => "privacy",
            Self::Webpage => "webpage",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.key() == value)
    }
}

/// How the bookmarks widget draws its links.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BookmarksView {
    /// A list: favicon and title.
    #[default]
    List,
    /// Tiles: favicon with the title beneath, as the speed dials draw.
    Tiles,
}

impl BookmarksView {
    pub fn key(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Tiles => "tiles",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "list" => Some(Self::List),
            "tiles" => Some(Self::Tiles),
            _ => None,
        }
    }
}

/// One widget and its configuration. Only the fields its kind uses are meaningful.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Widget {
    pub id: u32,
    pub kind: WidgetKind,
    /// Bookmarks: the folder listed (0 = every bookmark in the root).
    pub folder: u64,
    /// Bookmarks: how it is drawn.
    pub view: BookmarksView,
    /// Webpage: the embedded address (http or https).
    pub url: String,
    /// Notes: the sticky note's text.
    pub text: String,
}

impl Widget {
    fn new(id: u32, kind: WidgetKind) -> Self {
        Self { id, kind, folder: 0, view: BookmarksView::List, url: String::new(), text: String::new() }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WidgetSet {
    pub items: Vec<Widget>,
    next_id: u32,
}

impl Default for WidgetSet {
    /// What a profile with no `start-widgets.txt` shows: the date, top sites and privacy statistics.
    fn default() -> Self {
        let kinds = [WidgetKind::Date, WidgetKind::TopSites, WidgetKind::Privacy];
        Self { items: kinds.iter().enumerate().map(|(i, k)| Widget::new(i as u32 + 1, *k)).collect(), next_id: kinds.len() as u32 + 1 }
    }
}

impl WidgetSet {
    pub fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        if lines.next() != Some(WIDGETS_HEADER) {
            return None;
        }
        let mut set = Self { items: Vec::new(), next_id: 1 };
        for line in lines {
            let mut fields = line.split('\t');
            if fields.next() != Some("w") {
                continue;
            }
            let (Some(id), Some(kind)) = (fields.next().and_then(|f| f.parse::<u32>().ok()), fields.next().and_then(WidgetKind::parse)) else {
                continue;
            };
            if id == 0 || set.items.iter().any(|w| w.id == id) || set.items.len() >= MAX_WIDGETS {
                continue;
            }
            let mut widget = Widget::new(id, kind);
            widget.folder = fields.next().and_then(|f| f.parse().ok()).unwrap_or(0);
            widget.view = fields.next().and_then(BookmarksView::parse).unwrap_or_default();
            widget.url = fields.next().map(unescape).and_then(|u| web_address(&u)).unwrap_or_default();
            widget.text = fields.next().map(unescape).unwrap_or_default().chars().take(MAX_NOTE_CHARS).collect();
            set.next_id = set.next_id.max(id.saturating_add(1));
            set.items.push(widget);
        }
        Some(set)
    }

    pub fn serialize(&self) -> String {
        let mut out = format!("{WIDGETS_HEADER}\n");
        for w in &self.items {
            out.push_str(&format!("w\t{}\t{}\t{}\t{}\t{}\t{}\n", w.id, w.kind.key(), w.folder, w.view.key(), escape(&w.url), escape(&w.text)));
        }
        out
    }

    /// The saved set, or the defaults when there is no file (an unreadable file is the default too,
    /// and is not rewritten until the user changes something).
    pub fn load(dir: &Path) -> Self {
        read_small(&dir.join(WIDGETS_FILE)).and_then(|text| Self::parse(&text)).unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> io::Result<()> {
        atomic_write(&dir.join(WIDGETS_FILE), &self.serialize())
    }

    pub fn add(&mut self, kind: WidgetKind) -> io::Result<u32> {
        if self.items.len() >= MAX_WIDGETS {
            return Err(bad("too many widgets"));
        }
        let id = self.next_id.max(1);
        self.next_id = id.saturating_add(1);
        self.items.push(Widget::new(id, kind));
        Ok(id)
    }

    pub fn remove(&mut self, id: u32) -> bool {
        let before = self.items.len();
        self.items.retain(|w| w.id != id);
        self.items.len() != before
    }

    /// Moves widget `id` before widget `before` (the end when `None`).
    pub fn reorder(&mut self, id: u32, before: Option<u32>) -> bool {
        let Some(from) = self.items.iter().position(|w| w.id == id) else { return false };
        let item = self.items.remove(from);
        let index = match before {
            None => self.items.len(),
            Some(target) => match self.items.iter().position(|w| w.id == target) {
                Some(index) => index,
                None => {
                    self.items.insert(from, item);
                    return false;
                }
            },
        };
        self.items.insert(index, item);
        true
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut Widget> {
        self.items.iter_mut().find(|w| w.id == id)
    }
}

// ----- widget data -----

/// One of the most visited sites.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TopSite {
    pub host: String,
    pub title: String,
    pub url: String,
}

/// The most visited sites from history entries (`(timestamp, url, title)`, one line per address): hosts
/// ranked by how many of their pages were visited, ties by the newest visit. Each site links to the
/// host's most recently visited page.
pub fn top_sites(history: &[(u64, String, String)], limit: usize) -> Vec<TopSite> {
    struct Tally {
        count: usize,
        newest: u64,
        url: String,
        title: String,
    }
    // One hash lookup per row: a linear scan per row was quadratic over 5000 history rows.
    let mut tallies: std::collections::HashMap<String, Tally> = std::collections::HashMap::new();
    for (stamp, url, title) in history {
        let Ok(parsed) = url::Url::parse(url) else { continue };
        if !matches!(parsed.scheme(), "http" | "https") {
            continue;
        }
        let Some(host) = parsed.host_str() else { continue };
        let host = host.strip_prefix("www.").unwrap_or(host).to_ascii_lowercase();
        match tallies.get_mut(&host) {
            Some(tally) => {
                tally.count += 1;
                if *stamp >= tally.newest {
                    tally.newest = *stamp;
                    tally.url.clone_from(url);
                    tally.title.clone_from(title);
                }
            }
            None => {
                tallies.insert(host, Tally { count: 1, newest: *stamp, url: url.clone(), title: title.clone() });
            }
        }
    }
    // The sort is total (the host breaks every tie), so the map's order never shows.
    let mut hosts: Vec<(String, Tally)> = tallies.into_iter().collect();
    hosts.sort_by(|a, b| b.1.count.cmp(&a.1.count).then(b.1.newest.cmp(&a.1.newest)).then(a.0.cmp(&b.0)));
    hosts.into_iter().take(limit).map(|(host, t)| TopSite { host, title: t.title, url: t.url }).collect()
}

/// The links of bookmark folder `folder` (0 = the root), for the bookmarks widget.
pub fn widget_links(store: &BookmarkStore, folder: u64) -> Vec<(u64, String, String, String)> {
    let parent = (folder != 0).then_some(folder);
    store
        .children(parent)
        .into_iter()
        .filter_map(|item| match &item.kind {
            BookmarkKind::Link { url, favicon_host, .. } => Some((item.id, item.title.clone(), url.clone(), favicon_host.clone())),
            BookmarkKind::Folder => None,
        })
        .take(MAX_WIDGET_ROWS)
        .collect()
}

/// The title a sticky note gets when it is saved as a note: its first line, trimmed and shortened.
pub fn note_title(text: &str) -> String {
    let first = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("Sticky note");
    first.chars().take(60).collect()
}

pub mod service;

#[cfg(test)]
mod tests;
