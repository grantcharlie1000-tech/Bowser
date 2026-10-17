//! Bookmarks rail panel: a nested folder tree or a search list, with inline drafts for adding and
//! renaming, drag-to-reorder, and the keyboard and pointer rules.
//!
//! The view-model in this file is pure. It turns the bookmark store's items into rows and turns
//! input into [`Action`]s. The window layer runs each action against `BookmarkStore`, saves it,
//! and rebuilds the model. Nothing here touches a window or a file.

use std::collections::{HashMap, HashSet};

use super::panel_widgets::{self as w, Key, Metrics, Slot};
use crate::bookmarks::{Bookmark, BookmarkKind, BookmarkMatch};

/// Deepest folder level drawn. The store already refuses cycles, so this only bounds recursion.
const MAX_DEPTH: usize = 32;
const MAX_TITLE: usize = 256;
const MAX_URL: usize = 2048;
const PATH_SEPARATOR: &str = " \u{203A} ";
const ROOT_LABEL: &str = "Bookmarks";

/// One visible line of the panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub id: u64,
    pub parent: Option<u64>,
    pub depth: usize,
    pub folder: bool,
    pub expanded: bool,
    pub child_count: usize,
    pub title: String,
    /// Empty for folders.
    pub url: String,
    /// Host shown under the title, without `www.`.
    pub host: String,
    /// Key for the favicon cache, as stored with the bookmark.
    pub favicon_host: String,
    /// Folder breadcrumb for search results. Empty in the tree.
    pub path: String,
    /// Folder ids above this row, outermost first. Used to refuse dropping a folder into itself.
    pub ancestors: Vec<u64>,
}

fn row_for(item: &Bookmark, depth: usize, ancestors: Vec<u64>, expanded: bool, child_count: usize) -> Row {
    let (url, favicon_host) = match &item.kind {
        BookmarkKind::Link {
            url, favicon_host, ..
        } => (url.clone(), favicon_host.clone()),
        BookmarkKind::Folder => (String::new(), String::new()),
    };
    let host = w::host_label(&url);
    Row {
        id: item.id,
        parent: item.parent,
        depth,
        folder: matches!(item.kind, BookmarkKind::Folder),
        expanded,
        child_count,
        title: item.title.clone(),
        url,
        host,
        favicon_host,
        path: String::new(),
        ancestors,
    }
}

/// The visible tree. A folder's children appear only while it is in `expanded`. Siblings keep
/// the order they have in `items`, which is the saved order.
pub(crate) fn tree_rows(items: &[Bookmark], expanded: &HashSet<u64>) -> Vec<Row> {
    let mut children: HashMap<Option<u64>, Vec<&Bookmark>> = HashMap::new();
    for item in items {
        children.entry(item.parent).or_default().push(item);
    }
    let mut rows = Vec::new();
    let mut ancestors = Vec::new();
    push_children(&children, expanded, None, 0, &mut ancestors, &mut rows);
    rows
}

fn push_children(
    children: &HashMap<Option<u64>, Vec<&Bookmark>>,
    expanded: &HashSet<u64>,
    parent: Option<u64>,
    depth: usize,
    ancestors: &mut Vec<u64>,
    rows: &mut Vec<Row>,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let Some(list) = children.get(&parent) else {
        return;
    };
    for item in list {
        let folder = matches!(item.kind, BookmarkKind::Folder);
        let open = folder && expanded.contains(&item.id);
        let count = children.get(&Some(item.id)).map_or(0, Vec::len);
        rows.push(row_for(item, depth, ancestors.clone(), open, count));
        if open {
            ancestors.push(item.id);
            push_children(children, expanded, Some(item.id), depth + 1, ancestors, rows);
            ancestors.pop();
        }
    }
}

/// Flat search results for links, each with the folder path it lives in.
pub(crate) fn search_rows(matches: &[BookmarkMatch]) -> Vec<Row> {
    matches
        .iter()
        .filter(|m| matches!(m.bookmark.kind, BookmarkKind::Link { .. }))
        .map(|m| {
            let mut row = row_for(&m.bookmark, 0, Vec::new(), false, 0);
            row.path = m.folder_path.join(PATH_SEPARATOR);
            row
        })
        .collect()
}

/// A folder the user can file a bookmark into. `id` is `None` for the top level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FolderChoice {
    pub id: Option<u64>,
    pub label: String,
    pub depth: usize,
}

/// Every folder in tree order, with the top level first. Folders in `exclude` and their
/// descendants are left out, so a folder cannot be filed into itself.
pub(crate) fn folder_choices(items: &[Bookmark], exclude: Option<u64>) -> Vec<FolderChoice> {
    let mut out = vec![FolderChoice {
        id: None,
        label: ROOT_LABEL.into(),
        depth: 0,
    }];
    let mut children: HashMap<Option<u64>, Vec<&Bookmark>> = HashMap::new();
    for item in items {
        children.entry(item.parent).or_default().push(item);
    }
    let excluded = exclude.map(|id| descendants(items, id)).unwrap_or_default();
    push_folders(&children, &excluded, None, 1, &mut out);
    out
}

fn push_folders(
    children: &HashMap<Option<u64>, Vec<&Bookmark>>,
    excluded: &HashSet<u64>,
    parent: Option<u64>,
    depth: usize,
    out: &mut Vec<FolderChoice>,
) {
    if depth > MAX_DEPTH {
        return;
    }
    for item in children.get(&parent).into_iter().flatten() {
        if !matches!(item.kind, BookmarkKind::Folder) || excluded.contains(&item.id) {
            continue;
        }
        out.push(FolderChoice {
            id: Some(item.id),
            label: item.title.clone(),
            depth,
        });
        push_folders(children, excluded, Some(item.id), depth + 1, out);
    }
}

/// `id` and every item below it.
pub(crate) fn descendants(items: &[Bookmark], id: u64) -> HashSet<u64> {
    let mut set = HashSet::from([id]);
    let mut grew = true;
    while grew {
        grew = false;
        for item in items {
            if item.parent.is_some_and(|p| set.contains(&p)) && set.insert(item.id) {
                grew = true;
            }
        }
    }
    set
}

/// Ids of the children of `parent`, in saved order.
pub(crate) fn child_ids(items: &[Bookmark], parent: Option<u64>) -> Vec<u64> {
    items
        .iter()
        .filter(|i| i.parent == parent)
        .map(|i| i.id)
        .collect()
}

/// What a draft creates or changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Intent {
    Create,
    Edit(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DraftKind {
    Folder,
    Link,
}

/// The inline editor's contents, shared by add, rename and add-current-page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Draft {
    pub intent: Intent,
    pub kind: DraftKind,
    pub parent: Option<u64>,
    pub title: String,
    pub url: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Problem {
    EmptyTitle,
    LongTitle,
    BadUrl,
}

impl Problem {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::EmptyTitle => "Give it a name.",
            Self::LongTitle => "Keep the name under 256 characters.",
            Self::BadUrl => "Enter a full http:// or https:// address.",
        }
    }
}

/// Checks a draft before it reaches the store. Titles are trimmed. URLs must be absolute http or
/// https addresses without credentials, which is the same rule the store applies.
pub(crate) fn check_draft(draft: &Draft) -> Result<(String, String), Problem> {
    let title = draft.title.trim();
    if title.is_empty() {
        return Err(Problem::EmptyTitle);
    }
    if title.chars().count() > MAX_TITLE {
        return Err(Problem::LongTitle);
    }
    let url = match draft.kind {
        DraftKind::Folder => String::new(),
        DraftKind::Link => valid_bookmark_url(draft.url.trim()).ok_or(Problem::BadUrl)?,
    };
    Ok((title.to_owned(), url))
}

pub(crate) fn valid_bookmark_url(text: &str) -> Option<String> {
    if text.is_empty() || text.len() > MAX_URL {
        return None;
    }
    let url = url::Url::parse(text).ok()?;
    if matches!(url.scheme(), "http" | "https")
        && url.username().is_empty()
        && url.password().is_none()
        && url.host_str().is_some()
    {
        Some(url.to_string())
    } else {
        None
    }
}

/// True when `draft` is a link whose address another bookmark already uses. The bookmark being
/// edited does not count against itself. An address that does not validate is left to
/// `check_draft`, which reports it.
pub(crate) fn address_taken(items: &[Bookmark], draft: &Draft) -> bool {
    if draft.kind != DraftKind::Link {
        return false;
    }
    let Some(url) = valid_bookmark_url(draft.url.trim()) else {
        return false;
    };
    let own = match draft.intent {
        Intent::Edit(id) => Some(id),
        Intent::Create => None,
    };
    items.iter().any(|item| {
        Some(item.id) != own
            && matches!(&item.kind, BookmarkKind::Link { url: existing, .. } if *existing == url)
    })
}

/// What the panel shows when there is nothing to list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Empty {
    NoBookmarks,
    NoMatches,
}

impl Empty {
    pub(crate) fn text(self) -> &'static str {
        match self {
            Self::NoBookmarks => "No bookmarks yet. Press the heart in the rail to save a page.",
            Self::NoMatches => "No matches. Try a shorter word or check the spelling.",
        }
    }
}

pub(crate) fn empty_state(total_items: usize, rows: usize, query: &str) -> Option<Empty> {
    if rows > 0 {
        None
    } else if query.trim().is_empty() && total_items == 0 {
        Some(Empty::NoBookmarks)
    } else if !query.trim().is_empty() {
        Some(Empty::NoMatches)
    } else {
        None
    }
}

/// Where a dragged bookmark would land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DropPlan {
    /// Row under the pointer, which the indicator is drawn against.
    pub row: usize,
    pub slot: Slot,
    pub parent: Option<u64>,
    pub index: usize,
}

/// Something the window must do to the store. The model never mutates the store itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Nothing,
    /// The view changed and needs a repaint.
    Redraw,
    Open { url: String, new_tab: bool },
    Save(Draft),
    Delete(u64),
    Move { id: u64, parent: Option<u64>, index: usize },
    /// Escape with nothing open: the host should close the panel.
    Dismiss,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Model {
    items: Vec<Bookmark>,
    pub query: String,
    pub expanded: HashSet<u64>,
    pub rows: Vec<Row>,
    pub selected: Option<u64>,
    /// Row under the pointer, for the hover actions.
    pub hover: Option<usize>,
    pub scroll: i32,
    pub draft: Option<Draft>,
    /// Bookmark being dragged, set once the pointer has moved past the drag threshold.
    pub dragging: Option<u64>,
}

impl Model {
    /// Rebuilds the rows from a fresh snapshot. `search` runs the store's search for the current
    /// query and is only called when there is one.
    pub(crate) fn rebuild(
        &mut self,
        items: &[Bookmark],
        search: impl FnOnce(&str) -> Vec<BookmarkMatch>,
    ) {
        self.items = items.to_vec();
        self.rows = if self.query.trim().is_empty() {
            tree_rows(&self.items, &self.expanded)
        } else {
            search_rows(&search(self.query.trim()))
        };
        if self.selected.is_some_and(|id| !self.rows.iter().any(|r| r.id == id)) {
            self.selected = None;
        }
        self.hover = None;
    }

    pub(crate) fn selected_index(&self) -> Option<usize> {
        self.selected.and_then(|id| self.rows.iter().position(|r| r.id == id))
    }

    pub(crate) fn empty(&self) -> Option<Empty> {
        empty_state(self.items.len(), self.rows.len(), &self.query)
    }

    /// Keeps the selected row in view after movement or a layout change.
    pub(crate) fn reveal_selection(&mut self, m: Metrics) {
        if let Some(index) = self.selected_index() {
            self.scroll = w::scroll_to_show(self.scroll, index, self.rows.len(), m);
        }
    }

    pub(crate) fn set_query(&mut self, query: &str) {
        self.query = query.to_owned();
        self.scroll = 0;
    }

    pub(crate) fn toggle_folder(&mut self, id: u64) {
        if !self.expanded.remove(&id) {
            self.expanded.insert(id);
        }
    }

    pub(crate) fn key(&mut self, key: Key, new_tab: bool, m: Metrics) -> Action {
        if self.draft.is_some() {
            return match key {
                Key::Escape => {
                    self.draft = None;
                    Action::Redraw
                }
                _ => Action::Nothing,
            };
        }
        match key {
            Key::Escape => {
                if !self.query.is_empty() {
                    self.set_query("");
                    Action::Redraw
                } else {
                    Action::Dismiss
                }
            }
            Key::Up | Key::Down | Key::Home | Key::End | Key::PageUp | Key::PageDown => {
                let current = self.selected_index();
                let page = (m.height / m.row.max(1)).max(1) as usize;
                match w::moved_selection(key, current, self.rows.len(), page) {
                    Some(index) => {
                        self.selected = Some(self.rows[index].id);
                        self.reveal_selection(m);
                        Action::Redraw
                    }
                    None => Action::Nothing,
                }
            }
            Key::Left | Key::Right => {
                let Some(row) = self.selected.and_then(|id| self.row(id)).cloned() else {
                    return Action::Nothing;
                };
                if row.folder && (row.expanded == (key == Key::Left)) {
                    self.toggle_folder(row.id);
                    self.rebuild_tree();
                    Action::Redraw
                } else {
                    Action::Nothing
                }
            }
            Key::Enter => match self.selected.and_then(|id| self.row(id)).cloned() {
                Some(row) if row.folder => {
                    self.toggle_folder(row.id);
                    self.rebuild_tree();
                    Action::Redraw
                }
                Some(row) => Action::Open {
                    url: row.url,
                    new_tab,
                },
                None => Action::Nothing,
            },
            Key::Delete => self
                .selected
                .map_or(Action::Nothing, Action::Delete),
            Key::F2 => match self.selected {
                Some(id) => self.open_edit(id),
                None => Action::Nothing,
            },
        }
    }

    /// Pointer click on a row. Folders toggle, links open. Middle click and Ctrl-click pass
    /// `new_tab`.
    pub(crate) fn click(&mut self, index: usize, new_tab: bool, m: Metrics) -> Action {
        let Some(row) = self.rows.get(index).cloned() else {
            return Action::Nothing;
        };
        self.selected = Some(row.id);
        self.reveal_selection(m);
        if row.folder {
            if new_tab {
                return Action::Redraw;
            }
            self.toggle_folder(row.id);
            self.rebuild_tree();
            Action::Redraw
        } else {
            Action::Open {
                url: row.url,
                new_tab,
            }
        }
    }

    fn row(&self, id: u64) -> Option<&Row> {
        self.rows.iter().find(|r| r.id == id)
    }

    fn rebuild_tree(&mut self) {
        self.rows = tree_rows(&self.items, &self.expanded);
        if self.selected.is_some_and(|id| !self.rows.iter().any(|r| r.id == id)) {
            self.selected = None;
        }
        self.hover = None;
    }

    /// A new folder under the selected folder, or under the top level.
    pub(crate) fn new_folder(&mut self) -> Action {
        let parent = self.target_folder();
        self.draft = Some(Draft {
            intent: Intent::Create,
            kind: DraftKind::Folder,
            parent,
            title: "New folder".into(),
            url: String::new(),
        });
        Action::Redraw
    }

    /// A bookmark for the page in the current tab. The caller supplies the page's title and URL.
    pub(crate) fn add_current(&mut self, title: &str, url: &str) -> Action {
        let parent = self.target_folder();
        self.draft = Some(Draft {
            intent: Intent::Create,
            kind: DraftKind::Link,
            parent,
            title: title.to_owned(),
            url: url.to_owned(),
        });
        Action::Redraw
    }

    fn target_folder(&self) -> Option<u64> {
        self.selected
            .and_then(|id| self.row(id))
            .filter(|r| r.folder)
            .map(|r| r.id)
    }

    fn open_edit(&mut self, id: u64) -> Action {
        let Some(item) = self.items.iter().find(|i| i.id == id) else {
            return Action::Nothing;
        };
        let (kind, url) = match &item.kind {
            BookmarkKind::Folder => (DraftKind::Folder, String::new()),
            BookmarkKind::Link { url, .. } => (DraftKind::Link, url.clone()),
        };
        self.draft = Some(Draft {
            intent: Intent::Edit(id),
            kind,
            parent: item.parent,
            title: item.title.clone(),
            url,
        });
        Action::Redraw
    }

    /// Edit a row from its hover button.
    pub(crate) fn edit(&mut self, id: u64) -> Action {
        self.selected = Some(id);
        self.open_edit(id)
    }

    /// Checks the open draft. On success the draft closes and the save is returned for the store.
    /// On failure the draft stays open, and the message says what to fix.
    pub(crate) fn commit_draft(&mut self) -> Result<Action, Problem> {
        let Some(draft) = self.draft.clone() else {
            return Ok(Action::Nothing);
        };
        let (title, url) = check_draft(&draft)?;
        let saved = Draft { title, url, ..draft };
        self.draft = None;
        Ok(Action::Save(saved))
    }

    /// Where a bookmark dragged to `y` (viewport-relative) would land, or `None` for a drop onto
    /// itself, onto its own folder, or into a descendant.
    pub(crate) fn drop_plan(&self, dragged: u64, y: i32, m: Metrics) -> Option<DropPlan> {
        let row_index = w::row_at(y, self.scroll, self.rows.len(), m)?;
        let row = &self.rows[row_index];
        if row.id == dragged || row.ancestors.contains(&dragged) {
            return None;
        }
        let y_in_row = y + self.scroll - row_index as i32 * m.row;
        let slot = w::slot_in_row(y_in_row, m.row, row.folder);
        let (parent, index) = match slot {
            Slot::Into => {
                let siblings = child_ids(&self.items, Some(row.id));
                (Some(row.id), w::insert_index(&siblings, dragged, None, Slot::Into))
            }
            Slot::Before | Slot::After => {
                let siblings = child_ids(&self.items, row.parent);
                (
                    row.parent,
                    w::insert_index(&siblings, dragged, Some(row.id), slot),
                )
            }
        };
        Some(DropPlan {
            row: row_index,
            slot,
            parent,
            index,
        })
    }

    /// Turns a finished drag into the store move. The index already excludes the dragged item.
    pub(crate) fn finish_drag(&mut self, dragged: u64, y: i32, m: Metrics) -> Action {
        self.dragging = None;
        match self.drop_plan(dragged, y, m) {
            Some(plan) => Action::Move {
                id: dragged,
                parent: plan.parent,
                index: plan.index,
            },
            None => Action::Redraw,
        }
    }

    /// Moves the scroll position in pixels, clamped to the content.
    pub(crate) fn scroll_by(&mut self, delta: i32, m: Metrics) {
        self.scroll = w::clamp_scroll(self.scroll + delta, self.rows.len(), m);
    }
}

/// The row action buttons in a row rectangle. Index 0 is edit and index 1 is delete. They sit at
/// the right edge and are `size` square with `gap` between them.
pub(crate) fn row_buttons(row: windows::Win32::Foundation::RECT, size: i32, gap: i32, inset: i32) -> [windows::Win32::Foundation::RECT; 2] {
    let rect = |right: i32| windows::Win32::Foundation::RECT {
        left: right - size,
        top: row.top + (row.bottom - row.top - size) / 2,
        right,
        bottom: row.top + (row.bottom - row.top - size) / 2 + size,
    };
    let delete = rect(row.right - inset);
    let edit = rect(delete.left - gap);
    [edit, delete]
}

// ---- Window layer ---------------------------------------------------------------------------

use std::{cell::RefCell, path::PathBuf};

use windows::Win32::{
    Foundation::RECT,
    Graphics::Gdi::{DeleteObject, HDC, HFONT, SelectObject, SetBkMode, TRANSPARENT},
    UI::Input::KeyboardAndMouse::{
        GetKeyState, ReleaseCapture, SetCapture, VK_APPS, VK_CONTROL, VK_F10,
    },
};

use super::{
    menu::{self, Icon},
    panel_widgets::{EditEvent, EditField, EditSink, IconCache, PointerSink, Surface},
    smooth,
};
use crate::{
    app::{App, AppRef},
    bookmarks::BookmarkStore,
    profile::{self, Profile},
    theme::{Palette, Rgba},
    win32::{ask_yes_no_owned, show_error_message, side_panel::{self, PanelContent, PanelId}},
};

/// Field ids the sink receives from the panel's EDITs.
const SEARCH: usize = 0;
const TITLE: usize = 1;
const URL: usize = 2;

/// Drag starts after the pointer has moved this far, in DIP.
const DRAG_DIP: i32 = 4;
const ROW_DIP: i32 = 44;
const TOOLBAR_DIP: i32 = 28;
const SEARCH_DIP: i32 = 30;
const FIELD_DIP: i32 = 28;
const BUTTON_DIP: i32 = 26;
const PAD_DIP: i32 = 12;
const GAP_DIP: i32 = 6;
const INDENT_DIP: i32 = 16;
const ACTION_DIP: i32 = 22;
const LINE_DIP: i32 = 18;

thread_local! {
    /// Guest bookmarks live for the session only and are never written to disk.
    static GUEST: RefCell<Option<BookmarkStore>> = const { RefCell::new(None) };
}

/// Registers the bookmarks panel with the host. Call once at startup, on the UI thread.
pub(crate) fn register(app: &AppRef) {
    let app = app.clone();
    side_panel::register_content(PanelId::Bookmarks, move || {
        Box::new(Content::new(app.clone()))
    });
}

/// Clickable things in the panel, in content coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Btn {
    NewFolder,
    AddCurrent,
    DraftFolder,
    DraftSave,
    DraftCancel,
    RowEdit(u64),
    RowDelete(u64),
}

/// Positions of the fixed controls and the list, derived from the current draft and status.
struct Geometry {
    search: RECT,
    buttons: Vec<(Btn, RECT)>,
    draft_title: Option<RECT>,
    draft_url: Option<RECT>,
    status: Option<RECT>,
    list_top: i32,
    row: i32,
    pad: i32,
    indent: i32,
}

#[derive(Clone, Copy, Debug)]
struct Press {
    row: u64,
    /// Where the button went down, in content coordinates.
    x: i32,
    y: i32,
    /// Latest pointer position, in content coordinates.
    pointer_y: i32,
    dragging: bool,
}

pub(crate) struct Content {
    app: AppRef,
    profile: Option<Profile>,
    profile_dir: Option<PathBuf>,
    store: BookmarkStore,
    /// Set when the file could not be read. Saving is refused so the file is not overwritten.
    read_only: bool,
    model: Model,
    surface: Surface,
    font: HFONT,
    font_dpi: u32,
    fields: Option<Fields>,
    /// Whether the draft's text has been copied into the EDITs. It is copied once per draft, so
    /// a repaint never overwrites what the user is typing.
    draft_shown: bool,
    icons: IconCache,
    viewport: i32,
    press: Option<Press>,
    /// Problem from the last action, shown in the status line until the next action.
    message: Option<String>,
}

impl Drop for Content {
    fn drop(&mut self) {
        self.fields = None;
        if !self.surface.host.is_invalid() {
            w::unregister_right_click(self.surface.host);
        }
        if !self.font.is_invalid() {
            unsafe {
                let _ = DeleteObject(self.font.into());
            }
        }
    }
}

struct Fields {
    search: EditField,
    title: EditField,
    url: EditField,
}

impl Content {
    fn new(app: AppRef) -> Self {
        let profile = profile::active().ok();
        let (store, profile_dir, read_only) = match &profile {
            Some(profile) if profile.is_guest() => (
                GUEST.with(|g| g.borrow().clone()).unwrap_or_default(),
                None,
                false,
            ),
            Some(profile) => {
                let dir = profile.bookmarks_v2_file().parent().map(PathBuf::from);
                match dir.as_deref().map(BookmarkStore::load) {
                    Some(Ok(store)) => (store, dir, false),
                    Some(Err(error)) => {
                        show_error_message(&format!("Bookmarks could not be read: {error}. Changes will not be saved."));
                        (BookmarkStore::new(), dir, true)
                    }
                    None => (BookmarkStore::new(), None, true),
                }
            }
            None => (BookmarkStore::new(), None, true),
        };
        let mut content = Self {
            app,
            profile,
            profile_dir,
            store,
            read_only,
            model: Model::default(),
            surface: Surface::default(),
            font: HFONT::default(),
            font_dpi: 0,
            fields: None,
            draft_shown: false,
            icons: IconCache::default(),
            viewport: 0,
            press: None,
            message: None,
        };
        content.reload();
        content
    }

    fn px(&self, dip: i32) -> i32 {
        self.surface.px(dip)
    }

    /// Creates the EDITs and the font once the host window is known.
    fn ensure_controls(&mut self) {
        if self.surface.host.is_invalid() {
            return;
        }
        if self.font.is_invalid() || self.font_dpi != self.surface.dpi {
            let old_font = self.font;
            let new_font = w::panel_font(self.surface.dpi);
            self.font = new_font;
            self.font_dpi = self.surface.dpi;
            if let Some(fields) = &self.fields {
                fields.search.set_font(new_font);
                fields.title.set_font(new_font);
                fields.url.set_font(new_font);
            }
            if !old_font.is_invalid() { unsafe { let _ = DeleteObject(old_font.into()); } }
        }
        if self.fields.is_none() {
            let sink: *mut dyn EditSink = self as *mut Content;
            self.fields = Some(Fields {
                search: EditField::new(self.surface.host, sink, SEARCH, self.font),
                title: EditField::new(self.surface.host, sink, TITLE, self.font),
                url: EditField::new(self.surface.host, sink, URL, self.font),
            });
            if let Some(fields) = &self.fields {
                fields.search.set_cue("Search bookmarks");
                fields.title.set_cue("Name");
                fields.url.set_cue("Address, such as https://example.com");
            }
            let pointer: *mut dyn PointerSink = self as *mut Content;
            w::register_right_click(self.surface.host, pointer);
        }
    }

    fn geometry(&self) -> Geometry {
        let p = |v| self.px(v);
        let pad = p(PAD_DIP);
        let gap = p(GAP_DIP);
        let width = self.surface_width();
        let search = RECT {
            left: pad,
            top: pad,
            right: width - pad,
            bottom: pad + p(SEARCH_DIP),
        };
        let toolbar_top = search.bottom + gap;
        let toolbar = |left: i32, right: i32| RECT {
            left,
            top: toolbar_top,
            right,
            bottom: toolbar_top + p(TOOLBAR_DIP),
        };
        let mut buttons = vec![
            (Btn::NewFolder, toolbar(pad, pad + p(104))),
            (Btn::AddCurrent, toolbar(pad + p(112), pad + p(112) + p(150))),
        ];
        let mut y = toolbar_top + p(TOOLBAR_DIP) + gap * 2;
        let (mut draft_title, mut draft_url, mut status_rect) = (None, None, None);
        if let Some(draft) = &self.model.draft {
            let field = |top: i32| RECT {
                left: pad,
                top,
                right: width - pad,
                bottom: top + p(FIELD_DIP),
            };
            draft_title = Some(field(y));
            y += p(FIELD_DIP) + gap;
            if draft.kind == DraftKind::Link {
                draft_url = Some(field(y));
                y += p(FIELD_DIP) + gap;
            }
            let folder = RECT {
                left: pad,
                top: y,
                right: width - pad,
                bottom: y + p(BUTTON_DIP),
            };
            buttons.push((Btn::DraftFolder, folder));
            y += p(BUTTON_DIP) + gap;
            buttons.push((
                Btn::DraftSave,
                RECT {
                    left: pad,
                    top: y,
                    right: pad + p(96),
                    bottom: y + p(BUTTON_DIP),
                },
            ));
            buttons.push((
                Btn::DraftCancel,
                RECT {
                    left: pad + p(104),
                    top: y,
                    right: pad + p(104) + p(96),
                    bottom: y + p(BUTTON_DIP),
                },
            ));
            y += p(BUTTON_DIP) + gap * 2;
        }
        if self.status_text().is_some() {
            status_rect = Some(RECT {
                left: pad,
                top: y,
                right: width - pad,
                bottom: y + p(LINE_DIP),
            });
            y += p(LINE_DIP) + gap;
        }
        Geometry {
            search,
            buttons,
            draft_title,
            draft_url,
            status: status_rect,
            list_top: y,
            row: p(ROW_DIP),
            pad,
            indent: p(INDENT_DIP),
        }
    }

    fn surface_width(&self) -> i32 {
        (self.surface.origin.right - self.surface.origin.left).max(0)
    }

    fn status_text(&self) -> Option<String> {
        if let Some(message) = &self.message {
            return Some(message.clone());
        }
        if self.read_only {
            return Some("Bookmarks could not be read. Changes are not saved.".into());
        }
        None
    }

    /// Metrics for the list: the row height and the height of the visible list.
    fn list_metrics(&self, geometry: &Geometry) -> w::Metrics {
        w::Metrics {
            row: geometry.row,
            height: (self.viewport - geometry.list_top).max(0),
        }
    }

    fn reload(&mut self) {
        self.model.rebuild(self.store.items(), |q| self.store.search(q));
        self.surface.invalidate();
    }

    fn row_rect(&self, geometry: &Geometry, index: usize) -> RECT {
        let m = self.list_metrics(geometry);
        let top = geometry.list_top + w::row_top(index, self.model.scroll, m);
        RECT {
            left: geometry.pad,
            top,
            right: self.surface_width() - geometry.pad,
            bottom: top + geometry.row,
        }
    }

    fn action_rects(&self, row: RECT) -> [RECT; 2] {
        let size = self.px(ACTION_DIP);
        let inset = self.px(6);
        row_buttons(row, size, self.px(GAP_DIP) / 2, inset)
    }

    /// The control under a host point, in content coordinates.
    fn hit(&self, x: i32, y: i32) -> Option<Target> {
        let geometry = self.geometry();
        if let Some((btn, _)) = geometry.buttons.iter().find(|(_, r)| w::contains(*r, x, y)) {
            return Some(Target::Button(*btn));
        }
        let m = self.list_metrics(&geometry);
        if y < geometry.list_top {
            return None;
        }
        let index = w::row_at(y - geometry.list_top, self.model.scroll, self.model.rows.len(), m)?;
        let row = self.row_rect(&geometry, index);
        let id = self.model.rows[index].id;
        if self.model.hover == Some(index) || self.model.selected == Some(id) {
            let [edit, delete] = self.action_rects(row);
            if w::contains(delete, x, y) {
                return Some(Target::Button(Btn::RowDelete(id)));
            }
            if w::contains(edit, x, y) {
                return Some(Target::Button(Btn::RowEdit(id)));
            }
        }
        Some(Target::Row(index))
    }

    /// Runs an action from the model or a control against the store.
    fn run(&mut self, action: Action) {
        match action {
            Action::Nothing => return,
            Action::Redraw => {}
            Action::Open { url, new_tab } => self.open(&url, new_tab),
            Action::Save(draft) => self.save_draft(draft),
            Action::Delete(id) => self.delete(id),
            Action::Move { id, parent, index } => {
                let result = self.store.move_item(id, parent, index);
                self.finish(result.map(|_| ()));
            }
            Action::Dismiss => {
                let owner = self.surface.owner();
                side_panel::close(owner);
                return;
            }
        }
        self.surface.invalidate();
    }

    fn open(&mut self, url: &str, new_tab: bool) {
        let Ok(url) = url::Url::parse(url) else {
            return;
        };
        // "Open bookmarks in a new tab" (bm_open_new_tab).
        if new_tab || App::settings(&self.app).address.bookmark_new_tab {
            App::new_tab(&self.app);
        }
        App::navigate_current(&self.app, &url);
    }

    fn save_draft(&mut self, draft: Draft) {
        let result: std::io::Result<()> = match draft.intent {
            Intent::Create => match draft.kind {
                DraftKind::Folder => self.store.add_folder(draft.parent, &draft.title).map(|_| ()),
                DraftKind::Link => self
                    .store
                    .add_bookmark(draft.parent, &draft.title, &draft.url)
                    .map(|_| ()),
            },
            Intent::Edit(id) => self.edit_existing(id, &draft),
        };
        self.finish(result);
    }

    /// Changes a link's title and address in place, so its id, position and created time stay. A
    /// folder only takes a new title.
    fn edit_existing(&mut self, id: u64, draft: &Draft) -> std::io::Result<()> {
        let is_link = matches!(
            self.store.items().iter().find(|i| i.id == id).map(|i| &i.kind),
            Some(BookmarkKind::Link { .. })
        );
        if is_link {
            self.store.update_bookmark(id, &draft.title, &draft.url).map(|_| ())
        } else {
            self.store.rename(id, &draft.title).map(|_| ())
        }
    }

    fn delete(&mut self, id: u64) {
        let Some(item) = self.store.items().iter().find(|i| i.id == id).cloned() else {
            return;
        };
        if matches!(item.kind, BookmarkKind::Folder) {
            let inside = descendants(self.store.items(), id).len() - 1;
            if inside > 0
                && !ask_yes_no_owned(self.surface.owner(), &format!(
                    "Delete \"{}\" and the {inside} item{} inside it?",
                    item.title,
                    if inside == 1 { "" } else { "s" }
                ))
            {
                return;
            }
        }
        self.store.delete(id);
        self.finish(Ok(()));
    }

    /// Saves after a successful change, or shows what went wrong. Then rebuilds the rows.
    fn finish(&mut self, result: std::io::Result<()>) {
        match result {
            Ok(()) => {
                self.message = None;
                self.persist();
            }
            Err(error) => self.message = Some(format!("That did not work: {error}")),
        }
        self.reload();
    }

    fn persist(&mut self) {
        if self.read_only {
            return;
        }
        match &self.profile {
            Some(profile) if profile.is_guest() => {
                GUEST.with(|g| *g.borrow_mut() = Some(self.store.clone()));
            }
            _ => {
                if let Some(dir) = &self.profile_dir
                    && let Err(error) = self.store.save(dir)
                {
                    self.message = Some(format!("Bookmarks could not be saved: {error}"));
                }
            }
        }
    }

    fn folder_choices(&self, editing: Option<u64>) -> Vec<FolderChoice> {
        folder_choices(self.store.items(), editing)
    }

    /// Starts an add-folder draft, or an add-link draft for the current tab's page.
    fn add_current_page(&mut self) {
        let tab = App::active_tab_id(&self.app);
        let Some(url) = App::tab_committed_url(&self.app, tab) else {
            self.message = Some("This tab has no page to bookmark yet.".into());
            return;
        };
        let title = App::tab_title(&self.app, tab)
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| url.host_str().unwrap_or("Bookmark").to_owned());
        let action = self.model.add_current(&title, url.as_str());
        self.run(action);
    }

    /// Context menu for a row, or for the empty space when `row` is `None`.
    fn show_menu(&mut self, row: Option<usize>, x: i32, y: i32) {
        let owner = self.surface.owner();
        let target = row.and_then(|i| self.model.rows.get(i).cloned());
        let mut entries = Vec::new();
        if let Some(row) = &target {
            if row.folder {
                entries.push(w::item(10, "Open folder", Icon::None));
            } else {
                entries.push(w::item(1, "Open", Icon::None));
                entries.push(w::item(2, "Open in new tab", Icon::None));
            }
            entries.push(w::divider());
            entries.push(w::item(3, "Edit", Icon::None));
            let choices = self.folder_choices(Some(row.id));
            let mut moves = Vec::new();
            for (k, choice) in choices.iter().enumerate() {
                let indent = "  ".repeat(choice.depth);
                moves.push(w::item(1000 + k, format!("{indent}{}", choice.label), Icon::None));
            }
            if !moves.is_empty() {
                entries.push(menu::Entry::Item(
                    menu::Item::new(4, "Move to", Icon::None).submenu_with(moves),
                ));
            }
            entries.push(w::item(5, "Delete", Icon::Close));
            entries.push(w::divider());
        }
        entries.push(w::item(6, "New folder", Icon::Plus));
        entries.push(w::item(7, "Add current page", Icon::Check));
        let Some(chosen) = w::context_menu(owner, windows::Win32::Foundation::POINT { x, y }, entries) else {
            return;
        };
        let action = match (chosen, &target) {
            (1, Some(row)) => Action::Open { url: row.url.clone(), new_tab: false },
            (2, Some(row)) => Action::Open { url: row.url.clone(), new_tab: true },
            (3, Some(row)) => self.model.edit(row.id),
            (5, Some(row)) => Action::Delete(row.id),
            (6, _) => self.model.new_folder(),
            (7, _) => {
                self.add_current_page();
                return;
            }
            (id, Some(row)) if (1000..2000).contains(&id) => {
                let choices = self.folder_choices(Some(row.id));
                let Some(choice) = choices.get(id - 1000) else {
                    return;
                };
                let siblings = child_ids(self.store.items(), choice.id);
                let index = w::insert_index(&siblings, row.id, None, w::Slot::Into);
                Action::Move {
                    id: row.id,
                    parent: choice.id,
                    index,
                }
            }
            _ => Action::Redraw,
        };
        self.run(action);
    }
}

/// What a point in the panel refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Button(Btn),
    Row(usize),
}

impl EditSink for Content {
    fn edit_event(&mut self, field: usize, event: EditEvent) -> bool {
        match (field, event) {
            (SEARCH, EditEvent::Changed) => {
                let text = self
                    .fields
                    .as_ref()
                    .map(|f| f.search.text())
                    .unwrap_or_default();
                if text != self.model.query {
                    self.model.set_query(&text);
                    self.reload();
                }
                false
            }
            (SEARCH, EditEvent::Key(Key::Escape)) => {
                if self.model.query.is_empty() {
                    let owner = self.surface.owner();
                    side_panel::close(owner);
                } else {
                    if let Some(fields) = &self.fields {
                        fields.search.set_text("");
                    }
                    self.model.set_query("");
                    self.reload();
                }
                true
            }
            (SEARCH, EditEvent::Key(key @ (Key::Up | Key::Down | Key::PageUp | Key::PageDown | Key::Enter))) => {
                let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) < 0 };
                let m = self.list_metrics(&self.geometry());
                let action = self.model.key(key, ctrl, m);
                self.run(action);
                true
            }
            (SEARCH | TITLE | URL, EditEvent::Tab { back }) => {
                self.focus_next_field(field, back);
                true
            }
            (TITLE | URL, EditEvent::Key(Key::Enter)) => {
                self.commit();
                true
            }
            (TITLE | URL, EditEvent::Key(Key::Escape)) => {
                self.model.draft = None;
                self.surface.invalidate();
                self.surface.focus_host();
                true
            }
            _ => false,
        }
    }
}

impl Content {
    /// Tab moves between the search box, the draft title and the draft address. Past the last
    /// field it returns focus to the list.
    fn focus_next_field(&mut self, field: usize, back: bool) {
        let Some(fields) = &self.fields else {
            return;
        };
        let has_url = self.model.draft.as_ref().is_some_and(|d| d.kind == DraftKind::Link);
        let order: Vec<usize> = match (self.model.draft.is_some(), has_url) {
            (true, true) => vec![SEARCH, TITLE, URL],
            (true, false) => vec![SEARCH, TITLE],
            _ => vec![SEARCH],
        };
        let Some(current) = order.iter().position(|f| *f == field) else {
            return;
        };
        // Past either end of the order, focus goes to the list.
        let next = if back {
            current.checked_sub(1)
        } else {
            Some(current + 1).filter(|i| *i < order.len())
        };
        match next.map(|i| order[i]) {
            Some(SEARCH) => fields.search.focus(),
            Some(TITLE) => fields.title.focus(),
            Some(_) => fields.url.focus(),
            None => self.surface.focus_host(),
        }
    }

    fn commit(&mut self) {
        let title = self.fields.as_ref().map(|f| f.title.text()).unwrap_or_default();
        let url = self.fields.as_ref().map(|f| f.url.text()).unwrap_or_default();
        if let Some(draft) = self.model.draft.as_mut() {
            draft.title = title;
            if draft.kind == DraftKind::Link {
                draft.url = url;
            }
        }
        // The store refuses a second bookmark for the same address. Say so here and keep the draft
        // open, instead of reporting the store's raw error after the draft has closed.
        if let Some(draft) = self.model.draft.as_ref()
            && address_taken(self.store.items(), draft)
        {
            self.message = Some("That address is already bookmarked.".into());
            self.surface.invalidate();
            return;
        }
        match self.model.commit_draft() {
            Ok(action) => {
                self.message = None;
                self.run(action);
                self.surface.focus_host();
            }
            Err(problem) => {
                self.message = Some(problem.message().into());
                self.surface.invalidate();
            }
        }
    }
}

impl PointerSink for Content {
    fn right_click(&mut self, x: i32, y: i32) {
        let (cx, cy) = self.surface.to_content(x, y);
        let row = match self.hit(cx, cy) {
            Some(Target::Row(index)) => Some(index),
            _ => None,
        };
        let screen = self.surface.cursor();
        self.show_menu(row, screen.x, screen.y);
    }
}

impl PanelContent for Content {
    fn on_host_created(&mut self, host: windows::Win32::Foundation::HWND) {
        self.surface.attach(host);
    }

    fn paint(&mut self, hdc: HDC, rect: RECT, palette: &Palette) {
        let palette = &w::opaque(palette);
        self.surface.learn(hdc, rect, palette);
        if w::too_narrow(&self.surface) {
            return;
        }
        self.viewport = rect.bottom - rect.top;
        self.ensure_controls();
        self.draw(hdc, palette);
    }

    fn on_mouse_move(&mut self, x: i32, y: i32) {
        let (cx, cy) = self.surface.to_content(x, y);
        let geometry = self.geometry();
        let m = self.list_metrics(&geometry);
        let slop = self.px(DRAG_DIP);
        if let Some(press) = self.press.as_mut() {
            press.pointer_y = cy;
            if !press.dragging && ((cx - press.x).abs() > slop || (cy - press.y).abs() > slop) {
                press.dragging = true;
                self.model.dragging = Some(press.row);
            }
            if press.dragging {
                self.surface.invalidate();
                return;
            }
        }
        let hover = if cy >= geometry.list_top {
            w::row_at(cy - geometry.list_top, self.model.scroll, self.model.rows.len(), m)
        } else {
            None
        };
        if hover != self.model.hover {
            self.model.hover = hover;
            self.surface.invalidate();
        }
    }

    fn on_mouse_down(&mut self, x: i32, y: i32) {
        self.surface.focus_host();
        let (cx, cy) = self.surface.to_content(x, y);
        match self.hit(cx, cy) {
            Some(Target::Button(btn)) => {
                // A button may open a menu or a dialog. Capture is held only for a row, where a drag
                // can follow, because a capture held across a modal loop fights the mouse.
                self.press = None;
                self.press_button(btn);
            }
            Some(Target::Row(index)) => {
                unsafe {
                    let _ = SetCapture(self.surface.host);
                }
                let id = self.model.rows[index].id;
                self.model.selected = Some(id);
                self.press = Some(Press {
                    row: id,
                    x: cx,
                    y: cy,
                    pointer_y: cy,
                    dragging: false,
                });
                self.surface.invalidate();
            }
            None => self.press = None,
        }
    }

    fn on_mouse_up(&mut self, x: i32, y: i32) {
        unsafe {
            let _ = ReleaseCapture();
        }
        let Some(press) = self.press.take() else {
            return;
        };
        let (_, cy) = self.surface.to_content(x, y);
        let geometry = self.geometry();
        if press.dragging {
            let m = self.list_metrics(&geometry);
            let content_y = cy - geometry.list_top;
            let action = self.model.finish_drag(press.row, content_y, m);
            self.run(action);
            return;
        }
        let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) < 0 };
        if let Some(index) = self.model.rows.iter().position(|r| r.id == press.row) {
            let action = self.model.click(index, ctrl, self.list_metrics(&geometry));
            self.run(action);
        }
    }

    fn on_mouse_wheel(&mut self, delta: i16) {
        let geometry = self.geometry();
        let m = self.list_metrics(&geometry);
        let step = -(geometry.row * 3 * delta.signum() as i32);
        self.model.scroll_by(step, m);
        self.surface.invalidate();
    }

    fn on_char(&mut self, ch: char) {
        if ch.is_control() {
            return;
        }
        if let Some(fields) = &self.fields {
            fields.search.focus();
            w::type_into(fields.search.hwnd, ch);
        }
    }

    fn on_key(&mut self, key: u32, shift: bool, ctrl: bool) {
        if key == windows::Win32::UI::Input::KeyboardAndMouse::VK_TAB.0 as u32 {
            if let Some(fields) = &self.fields {
                fields.search.focus();
            }
            return;
        }
        if key == VK_APPS.0 as u32 || (key == VK_F10.0 as u32 && shift) {
            let index = self.model.selected_index();
            let geometry = self.geometry();
            let (x, y) = match index {
                Some(i) => {
                    let row = self.row_rect(&geometry, i);
                    (row.left + self.px(24), row.top + row.bottom)
                }
                None => (geometry.pad, geometry.list_top),
            };
            let (x, y) = (x + self.surface.origin.left, y + self.surface.origin.top);
            let mut screen = windows::Win32::Foundation::POINT { x, y };
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::ClientToScreen(self.surface.host, &mut screen);
            }
            self.show_menu(index, screen.x, screen.y);
            return;
        }
        if let Some(k) = w::key_from_vk(key as u16) {
            let m = self.list_metrics(&self.geometry());
            let action = self.model.key(k, ctrl, m);
            self.run(action);
        }
    }

    fn on_focus(&mut self, focused: bool) {
        if focused {
            self.message = None;
            self.reload();
        }
    }

    fn title(&self) -> &str {
        "Bookmarks"
    }

    fn preferred_width(&self) -> i32 {
        360
    }
}

impl Content {
    /// Button presses that do not depend on a row.
    fn press_button(&mut self, btn: Btn) {
        match btn {
            Btn::NewFolder => {
                let action = self.model.new_folder();
                self.run(action);
            }
            Btn::AddCurrent => self.add_current_page(),
            Btn::DraftFolder => self.pick_draft_folder(),
            Btn::DraftSave => self.commit(),
            Btn::DraftCancel => {
                self.model.draft = None;
                self.surface.invalidate();
            }
            Btn::RowEdit(id) => {
                let action = self.model.edit(id);
                self.run(action);
            }
            Btn::RowDelete(id) => self.run(Action::Delete(id)),
        }
    }

    /// Folder picker for the draft. The choice is stored in the draft's parent.
    fn pick_draft_folder(&mut self) {
        let editing = match self.model.draft.as_ref().map(|d| d.intent.clone()) {
            Some(Intent::Edit(id)) if self.model.draft.as_ref().is_some_and(|d| d.kind == DraftKind::Folder) => Some(id),
            _ => None,
        };
        let choices = self.folder_choices(editing);
        let entries: Vec<menu::Entry> = choices
            .iter()
            .enumerate()
            .map(|(k, c)| {
                let indent = "  ".repeat(c.depth);
                w::item(2000 + k, format!("{indent}{}", c.label), Icon::None)
            })
            .collect();
        let owner = self.surface.owner();
        let cursor = self.surface.cursor();
        if let Some(id) = w::context_menu(owner, cursor, entries)
            && let Some(choice) = choices.get(id.wrapping_sub(2000))
            && let Some(draft) = self.model.draft.as_mut()
        {
            draft.parent = choice.id;
            self.surface.invalidate();
        }
    }

    fn draw(&mut self, dc: HDC, palette: &Palette) {
        let geometry = self.geometry();
        let old = unsafe { SelectObject(dc, self.font.into()) };
        unsafe {
            SetBkMode(dc, TRANSPARENT);
        }
        // Keep EDIT placement in step with the layout.
        let surface = self.surface;
        if let Some(fields) = self.fields.as_mut() {
            fields.search.place(Some(surface.to_host(geometry.search)));
            fields.title.place(geometry.draft_title.map(|r| surface.to_host(r)));
            fields.url.place(geometry.draft_url.map(|r| surface.to_host(r)));
            match &self.model.draft {
                Some(draft) if !self.draft_shown => {
                    fields.title.set_text(&draft.title);
                    fields.url.set_text(&draft.url);
                    self.draft_shown = true;
                }
                Some(_) => {}
                None => self.draft_shown = false,
            }
        }
        for (btn, rect) in &geometry.buttons {
            let label = self.button_label(*btn);
            let primary = matches!(btn, Btn::DraftSave);
            let fill = if primary { palette.progress_fill } else { palette.tab_inactive };
            let text = if primary { Rgba::hex(0xFFFFFF, 0xFF) } else { palette.tab_text_active };
            unsafe { draw_button(dc, self.surface.to_host(*rect), &label, fill, text, self.px(8)) };
        }
        if let Some(text) = self.status_text() {
            if let Some(rect) = geometry.status {
                let colour = if self.message.is_some() { w::danger(palette) } else { palette.tab_text_inactive };
                unsafe { w::draw_text(dc, self.surface.to_host(rect), &text, colour, false, true) };
            }
        }
        self.draw_list(dc, palette, &geometry);
        unsafe {
            SelectObject(dc, old);
        }
    }

    fn button_label(&self, btn: Btn) -> String {
        match btn {
            Btn::NewFolder => "New folder".into(),
            Btn::AddCurrent => "Add current page".into(),
            Btn::DraftFolder => {
                let name = self
                    .model
                    .draft
                    .as_ref()
                    .and_then(|d| d.parent)
                    .and_then(|id| self.store.items().iter().find(|i| i.id == id))
                    .map_or("Bookmarks", |i| i.title.as_str());
                format!("Folder: {name}  \u{25BE}")
            }
            Btn::DraftSave => match self.model.draft.as_ref().map(|d| &d.intent) {
                Some(Intent::Edit(_)) => "Save".into(),
                _ => "Add".into(),
            },
            Btn::DraftCancel => "Cancel".into(),
            Btn::RowEdit(_) | Btn::RowDelete(_) => String::new(),
        }
    }

    fn draw_list(&mut self, dc: HDC, palette: &Palette, geometry: &Geometry) {
        let m = self.list_metrics(geometry);
        if let Some(empty) = self.model.empty() {
            let rect = RECT {
                left: geometry.pad,
                top: geometry.list_top + geometry.pad,
                right: self.surface_width() - geometry.pad,
                bottom: geometry.list_top + self.px(64),
            };
            unsafe { w::draw_text(dc, self.surface.to_host(rect), empty.text(), palette.tab_text_inactive, false, false) };
            return;
        }
        let range = w::visible_range(self.model.scroll, self.model.rows.len(), m);
        let icon = self.px(16);
        for index in range {
            let row = self.model.rows[index].clone();
            let rect = self.row_rect(geometry, index);
            let selected = self.model.selected == Some(row.id);
            let hovered = self.model.hover == Some(index);
            let body = self.surface.to_host(rect);
            if selected {
                unsafe { smooth::fill_round_rect(dc, body, self.px(10), palette.tab_active) };
            } else if hovered {
                unsafe { smooth::fill_round_rect(dc, body, self.px(10), palette.tab_hover) };
            }
            let mut x = rect.left + 6 + row.depth as i32 * geometry.indent;
            if row.folder {
                let chevron = [
                    windows::Win32::Foundation::POINT { x: x + 3, y: rect.top + 15 },
                    windows::Win32::Foundation::POINT { x: x + 8, y: rect.top + 20 },
                    windows::Win32::Foundation::POINT { x: x + 3, y: rect.top + 25 },
                ];
                let points = if row.expanded {
                    [
                        windows::Win32::Foundation::POINT { x: x + 1, y: rect.top + 17 },
                        windows::Win32::Foundation::POINT { x: x + 6, y: rect.top + 22 },
                        windows::Win32::Foundation::POINT { x: x + 11, y: rect.top + 17 },
                    ]
                } else {
                    chevron
                };
                unsafe {
                    let points: Vec<_> = points
                        .iter()
                        .map(|p| windows::Win32::Foundation::POINT {
                            x: self.surface.origin.left + p.x,
                            y: self.surface.origin.top + p.y,
                        })
                        .collect();
                    smooth::stroke_polygon(dc, &points, palette.tab_text_inactive, 2);
                }
                x += self.px(14);
                let folder = RECT { left: x, top: rect.top + 13, right: x + self.px(16), bottom: rect.top + 13 + self.px(12) };
                unsafe { smooth::fill_round_rect(dc, self.surface.to_host(folder), self.px(4), palette.progress_fill) };
            } else {
                let tile = RECT { left: x, top: rect.top + 12, right: x + icon, bottom: rect.top + 12 + icon };
                let host_tile = self.surface.to_host(tile);
                match self.icons.get(&row.favicon_host, icon as u32) {
                    Some(bitmap) => unsafe { w::blit_favicon(dc, host_tile, &bitmap) },
                    None => unsafe { w::letter_tile_at(dc, host_tile, &row.title, &row.favicon_host) },
                }
            }
            let text_left = if row.folder { x + self.px(24) } else { x + icon + self.px(8) };
            let [edit, delete] = self.action_rects(rect);
            let text_right = if selected || hovered { edit.left - self.px(6) } else { rect.right - self.px(6) };
            let title_rect = RECT { left: text_left, top: rect.top + self.px(5), right: text_right, bottom: rect.top + self.px(23) };
            unsafe { w::draw_text(dc, self.surface.to_host(title_rect), &row.title, palette.tab_text_active, false, true) };
            let detail = if row.folder {
                match row.child_count {
                    0 => "Empty folder".to_owned(),
                    1 => "1 item".to_owned(),
                    n => format!("{n} items"),
                }
            } else if !row.path.is_empty() {
                row.path.clone()
            } else {
                row.host.clone()
            };
            let detail_rect = RECT { left: text_left, top: rect.top + self.px(23), right: text_right, bottom: rect.top + self.px(39) };
            unsafe { w::draw_text(dc, self.surface.to_host(detail_rect), &detail, palette.tab_text_inactive, false, true) };
            if selected || hovered {
                unsafe {
                    draw_button(dc, self.surface.to_host(edit), "\u{270E}", palette.tab_inactive, palette.tab_text_active, self.px(6));
                    draw_button(dc, self.surface.to_host(delete), "\u{00D7}", palette.tab_inactive, w::danger(palette), self.px(6));
                }
            }
        }
        // Drop indicator while dragging.
        if let Some(press) = self.press.filter(|p| p.dragging) {
            let pointer = press.pointer_y - geometry.list_top;
            if let Some(plan) = self.model.drop_plan(press.row, pointer, m) {
                let row = self.row_rect(geometry, plan.row);
                let accent = palette.progress_fill;
                let host_row = self.surface.to_host(row);
                match plan.slot {
                    w::Slot::Into => unsafe { smooth::stroke_round_rect(dc, host_row, self.px(10), accent, 2) },
                    w::Slot::Before => unsafe { smooth::fill_round_rect(dc, RECT { left: host_row.left, top: host_row.top - 1, right: host_row.right, bottom: host_row.top + 2 }, 2, accent) },
                    w::Slot::After => unsafe { smooth::fill_round_rect(dc, RECT { left: host_row.left, top: host_row.bottom - 2, right: host_row.right, bottom: host_row.bottom + 1 }, 2, accent) },
                }
            }
        }
    }

}

/// Draws a rounded button with a centred label.
unsafe fn draw_button(dc: HDC, rect: RECT, label: &str, fill: Rgba, text: Rgba, radius: i32) {
    unsafe {
        smooth::fill_round_rect(dc, rect, radius * 2, fill);
        w::draw_text(dc, rect, label, text, true, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bookmarks::BookmarkStore;

    fn folder(id: u64, parent: Option<u64>, title: &str) -> Bookmark {
        Bookmark {
            id,
            parent,
            title: title.into(),
            kind: BookmarkKind::Folder,
        }
    }

    fn link(id: u64, parent: Option<u64>, title: &str, url: &str) -> Bookmark {
        Bookmark {
            id,
            parent,
            title: title.into(),
            kind: BookmarkKind::Link {
                url: url.into(),
                favicon_host: w::host_label(url),
                created: 0,
            },
        }
    }

    fn sample() -> Vec<Bookmark> {
        vec![
            folder(1, None, "Work"),
            link(2, None, "Top link", "https://example.com/top"),
            folder(3, Some(1), "Docs"),
            link(4, Some(1), "Rust", "https://www.rust-lang.org/"),
            link(5, Some(3), "Std", "https://doc.rust-lang.org/std/"),
        ]
    }

    fn m() -> Metrics {
        Metrics { row: 20, height: 60 }
    }

    fn open(items: &[Bookmark], expanded: &[u64]) -> Model {
        let mut model = Model {
            expanded: expanded.iter().copied().collect(),
            ..Model::default()
        };
        model.rebuild(items, |_| Vec::new());
        model
    }

    #[test]
    fn collapsed_folders_hide_their_children() {
        let model = open(&sample(), &[]);
        let ids: Vec<u64> = model.rows.iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![1, 2]);
        assert_eq!(model.rows[0].child_count, 2);
    }

    #[test]
    fn expanded_folders_show_nested_children_with_depth_and_ancestors() {
        let model = open(&sample(), &[1, 3]);
        let ids: Vec<u64> = model.rows.iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![1, 3, 5, 4, 2]);
        let std_row = &model.rows[2];
        assert_eq!(std_row.depth, 2);
        assert_eq!(std_row.ancestors, vec![1, 3]);
        assert_eq!(std_row.host, "doc.rust-lang.org");
        assert_eq!(model.rows[3].host, "rust-lang.org");
    }

    #[test]
    fn search_lists_links_only_with_their_folder_path() {
        let items = sample();
        let store_matches = vec![
            BookmarkMatch {
                bookmark: items[4].clone(),
                folder_path: vec!["Work".into(), "Docs".into()],
            },
            BookmarkMatch {
                bookmark: items[2].clone(),
                folder_path: vec![],
            },
        ];
        let rows = search_rows(&store_matches);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].path, "Work \u{203A} Docs");
    }

    #[test]
    fn rebuild_uses_search_when_a_query_is_set() {
        let mut model = Model::default();
        model.set_query("  std ");
        let items = sample();
        model.rebuild(&items, |q| {
            assert_eq!(q, "std");
            vec![BookmarkMatch {
                bookmark: items[4].clone(),
                folder_path: vec!["Work".into(), "Docs".into()],
            }]
        });
        assert_eq!(model.rows.len(), 1);
        assert_eq!(model.rows[0].id, 5);
    }

    #[test]
    fn folder_choices_skip_a_folder_and_its_descendants() {
        let items = sample();
        let all: Vec<Option<u64>> = folder_choices(&items, None).iter().map(|c| c.id).collect();
        assert_eq!(all, vec![None, Some(1), Some(3)]);
        let without_work: Vec<Option<u64>> =
            folder_choices(&items, Some(1)).iter().map(|c| c.id).collect();
        assert_eq!(without_work, vec![None]);
        assert_eq!(folder_choices(&items, None)[2].depth, 2);
    }

    #[test]
    fn descendants_include_the_root_and_every_level() {
        let items = sample();
        let set = descendants(&items, 1);
        assert_eq!(set, HashSet::from([1, 3, 4, 5]));
    }

    #[test]
    fn arrow_keys_move_and_clamp_within_the_visible_rows() {
        let mut model = open(&sample(), &[1]);
        // Visible rows with Work expanded: 1 (Work), 3 (Docs), 4 (Rust), 2 (top link).
        assert_eq!(model.key(Key::Down, false, m()), Action::Redraw);
        assert_eq!(model.selected, Some(1));
        model.key(Key::Down, false, m());
        model.key(Key::Down, false, m());
        assert_eq!(model.selected, Some(4));
        model.key(Key::Home, false, m());
        assert_eq!(model.selected, Some(1));
        model.key(Key::End, false, m());
        assert_eq!(model.selected, Some(2));
    }

    #[test]
    fn enter_toggles_folders_and_opens_links_in_the_right_tab() {
        let mut model = open(&sample(), &[]);
        model.selected = Some(1);
        assert_eq!(model.key(Key::Enter, false, m()), Action::Redraw);
        assert!(model.expanded.contains(&1));
        assert_eq!(model.rows.len(), 4);
        model.selected = Some(2);
        assert_eq!(
            model.key(Key::Enter, false, m()),
            Action::Open {
                url: "https://example.com/top".into(),
                new_tab: false
            }
        );
        assert_eq!(
            model.key(Key::Enter, true, m()),
            Action::Open {
                url: "https://example.com/top".into(),
                new_tab: true
            }
        );
    }

    #[test]
    fn left_and_right_collapse_and_expand_folders() {
        let mut model = open(&sample(), &[]);
        model.selected = Some(1);
        assert_eq!(model.key(Key::Left, false, m()), Action::Nothing);
        assert_eq!(model.key(Key::Right, false, m()), Action::Redraw);
        assert_eq!(model.rows.len(), 4);
        assert_eq!(model.key(Key::Left, false, m()), Action::Redraw);
        assert_eq!(model.rows.len(), 2);
    }

    #[test]
    fn delete_and_f2_act_on_the_selected_row() {
        let mut model = open(&sample(), &[]);
        assert_eq!(model.key(Key::Delete, false, m()), Action::Nothing);
        model.selected = Some(2);
        assert_eq!(model.key(Key::Delete, false, m()), Action::Delete(2));
        match model.key(Key::F2, false, m()) {
            Action::Redraw => {}
            other => panic!("expected a draft, got {other:?}"),
        }
        let draft = model.draft.clone().expect("F2 opens the editor");
        assert_eq!(draft.intent, Intent::Edit(2));
        assert_eq!(draft.url, "https://example.com/top");
    }

    #[test]
    fn escape_closes_the_draft_then_clears_the_search_then_dismisses() {
        let mut model = open(&sample(), &[]);
        model.new_folder();
        assert_eq!(model.key(Key::Escape, false, m()), Action::Redraw);
        assert!(model.draft.is_none());
        model.set_query("rust");
        assert_eq!(model.key(Key::Escape, false, m()), Action::Redraw);
        assert!(model.query.is_empty());
        assert_eq!(model.key(Key::Escape, false, m()), Action::Dismiss);
    }

    #[test]
    fn new_folder_goes_inside_the_selected_folder_or_to_the_top() {
        let mut model = open(&sample(), &[1]);
        model.new_folder();
        assert_eq!(model.draft.as_ref().unwrap().parent, None);
        model.draft = None;
        model.selected = Some(1);
        model.new_folder();
        let draft = model.draft.clone().unwrap();
        assert_eq!(draft.parent, Some(1));
        assert_eq!(draft.kind, DraftKind::Folder);
    }

    #[test]
    fn drafts_are_trimmed_and_checked_before_saving() {
        let mut draft = Draft {
            intent: Intent::Create,
            kind: DraftKind::Link,
            parent: None,
            title: "  Spec  ".into(),
            url: " https://example.com/spec ".into(),
        };
        assert_eq!(
            check_draft(&draft),
            Ok(("Spec".into(), "https://example.com/spec".into()))
        );
        draft.title = "   ".into();
        assert_eq!(check_draft(&draft), Err(Problem::EmptyTitle));
        draft.title = "ok".into();
        draft.url = "javascript:alert(1)".into();
        assert_eq!(check_draft(&draft), Err(Problem::BadUrl));
        draft.url = "https://user:pw@example.com/".into();
        assert_eq!(check_draft(&draft), Err(Problem::BadUrl));
        draft.url = "https://example.com/".into();
        draft.title = "x".repeat(257);
        assert_eq!(check_draft(&draft), Err(Problem::LongTitle));
        draft.kind = DraftKind::Folder;
        draft.title = "Folder".into();
        draft.url = "not used".into();
        assert_eq!(check_draft(&draft), Ok(("Folder".into(), String::new())));
    }

    #[test]
    fn commit_keeps_the_draft_open_on_problems() {
        let mut model = open(&sample(), &[]);
        model.new_folder();
        model.draft.as_mut().unwrap().title = " ".into();
        assert_eq!(model.commit_draft(), Err(Problem::EmptyTitle));
        assert!(model.draft.is_some());
        model.draft.as_mut().unwrap().title = "Later".into();
        assert!(matches!(model.commit_draft(), Ok(Action::Save(d)) if d.title == "Later"));
        assert!(model.draft.is_none());
    }

    #[test]
    fn empty_states_distinguish_no_bookmarks_from_no_matches() {
        assert_eq!(empty_state(0, 0, ""), Some(Empty::NoBookmarks));
        assert_eq!(empty_state(3, 0, "zzz"), Some(Empty::NoMatches));
        assert_eq!(empty_state(3, 2, ""), None);
        assert_eq!(empty_state(0, 0, "  "), Some(Empty::NoBookmarks));
    }

    #[test]
    fn dropping_a_bookmark_before_or_after_a_row_uses_the_row_parent() {
        let model = open(&sample(), &[1]);
        // Rows: 1 (Work), 3 (Docs), 5, 4, 2. Dragging link 2 onto the top edge of row 0 (Work).
        let plan = model.drop_plan(2, 1, m()).unwrap();
        assert_eq!(plan.parent, None);
        assert_eq!(plan.slot, Slot::Before);
        assert_eq!(plan.index, 0);
        // The lower half of Rust (row 3, child of Work) inserts after it, inside Work.
        let y = 3 * 20 + 15;
        let mut model = open(&sample(), &[1, 3]);
        model.scroll = 0;
        let plan = model.drop_plan(2, y, Metrics { row: 20, height: 200 }).unwrap();
        assert_eq!(plan.row, 3);
        assert_eq!(plan.parent, Some(1));
        assert_eq!(plan.slot, Slot::After);
        assert_eq!(plan.index, 2);
    }

    #[test]
    fn dropping_into_a_folder_appends_and_skips_its_own_subtree() {
        let model = open(&sample(), &[1, 3]);
        let big = Metrics { row: 20, height: 200 };
        // Drop link 2 into the middle of folder Docs (row 1).
        let plan = model.drop_plan(2, 20 + 10, big).unwrap();
        assert_eq!(plan.slot, Slot::Into);
        assert_eq!(plan.parent, Some(3));
        assert_eq!(plan.index, 1);
        // Drop folder Work onto its own child Docs: refused.
        assert_eq!(model.drop_plan(1, 20 + 10, big), None);
        // Drop onto itself: refused.
        assert_eq!(model.drop_plan(3, 20 + 10, big), None);
    }

    #[test]
    fn finish_drag_reports_the_store_move_or_just_redraws() {
        let mut model = open(&sample(), &[1]);
        model.dragging = Some(2);
        let big = Metrics { row: 20, height: 200 };
        // Row 2 is Rust (child of Work). Its lower half inserts after it, inside Work.
        assert_eq!(
            model.finish_drag(2, 20 * 2 + 10, big),
            Action::Move {
                id: 2,
                parent: Some(1),
                index: 2
            }
        );
        assert_eq!(model.dragging, None);
        assert_eq!(model.finish_drag(1, 10, big), Action::Redraw);
    }

    #[test]
    fn row_buttons_sit_at_the_right_edge_with_edit_before_delete() {
        let row = windows::Win32::Foundation::RECT {
            left: 0,
            top: 0,
            right: 300,
            bottom: 40,
        };
        let [edit, delete] = row_buttons(row, 20, 4, 8);
        assert_eq!((delete.left, delete.right), (272, 292));
        assert_eq!((edit.left, edit.right), (248, 268));
        assert_eq!((delete.top, delete.bottom), (10, 30));
        assert!(w::contains(edit, 250, 15));
        assert!(!w::contains(edit, 270, 15));
    }

    #[test]
    fn store_items_round_trip_through_the_tree() {
        let mut store = BookmarkStore::new();
        let work = store.add_folder(None, "Work").unwrap();
        store
            .add_bookmark(Some(work), "Spec", "https://example.com/spec")
            .unwrap();
        let mut model = Model {
            expanded: HashSet::from([work]),
            ..Model::default()
        };
        model.rebuild(store.items(), |_| Vec::new());
        assert_eq!(model.rows.len(), 2);
        assert_eq!(model.rows[1].parent, Some(work));
        assert!(!child_ids(store.items(), Some(work)).is_empty());
    }

    fn draft(intent: Intent, kind: DraftKind, url: &str) -> Draft {
        Draft {
            intent,
            kind,
            parent: None,
            title: "T".into(),
            url: url.into(),
        }
    }

    #[test]
    fn a_new_link_cannot_reuse_an_address_and_an_edit_may_keep_its_own() {
        let items = vec![
            link(1, None, "A", "https://a.example/"),
            link(2, None, "B", "https://b.example/"),
        ];
        assert!(address_taken(&items, &draft(Intent::Create, DraftKind::Link, "https://a.example/")));
        assert!(!address_taken(&items, &draft(Intent::Create, DraftKind::Link, "https://c.example/")));
        // Editing A to keep its own address is fine; editing A to B's address is not.
        assert!(!address_taken(&items, &draft(Intent::Edit(1), DraftKind::Link, "https://a.example/")));
        assert!(address_taken(&items, &draft(Intent::Edit(1), DraftKind::Link, "https://b.example/")));
    }

    #[test]
    fn folders_and_invalid_addresses_are_left_to_the_draft_check() {
        let items = vec![link(1, None, "A", "https://a.example/")];
        assert!(!address_taken(&items, &draft(Intent::Create, DraftKind::Folder, "https://a.example/")));
        assert!(!address_taken(&items, &draft(Intent::Create, DraftKind::Link, "not a url")));
    }

    #[test]
    fn valid_bookmark_urls_are_absolute_http_only() {
        assert_eq!(
            valid_bookmark_url("https://Example.com"),
            Some("https://example.com/".into())
        );
        assert_eq!(valid_bookmark_url("ftp://example.com"), None);
        assert_eq!(valid_bookmark_url("example.com"), None);
        assert_eq!(valid_bookmark_url(""), None);
    }
}
