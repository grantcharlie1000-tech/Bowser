//! The Bookmarks panel's wire: its snapshot (`{t:"snap",p:"bookmarks"}`) and its commands
//! (`{t:"bm",a:..}`), modelled on Chrome's and Edge's bookmark side panels.
//!
//! - Two roots, as in Chrome and Edge: "Bookmarks bar" (the top level, wire id `0`) and
//!   "Other bookmarks" (its folder id, or [`OTHER_WIRE`] before it exists).
//! - The page gets every folder, but the links of only the folders that are open (and of the bar),
//!   inside a byte budget: a large imported tree must never pass the page's message limit, which
//!   would leave the panel empty. Searching is done here and capped the same way.
//! - Mutations go through [`mutate`], which says why a change was refused in words a person can act
//!   on, and hands back what a delete removed so it can be undone.
use std::collections::HashSet;

use super::super::{json::{self, Json}, protocol::{self, PanelFields}};
use crate::bookmarks::{Bookmark, BookmarkKind, BookmarkStore, Removed, OTHER_WIRE};

/// Bytes of item JSON one snapshot carries (the page drops any message over 256 KiB).
const ITEM_BUDGET: usize = 150 * 1024;
/// Most hosts whose favicon rides along, and their total size.
const MAX_FAV_HOSTS: usize = 96;
const MAX_FAV_TOTAL_BYTES: usize = 72 * 1024;
/// Most search results listed.
pub(crate) const MAX_RESULTS: usize = 300;
/// A link's address as sent: enough for the host, the tooltip and the spoiler guard.
const MAX_URL_CHARS: usize = 400;
const MAX_TITLE_CHARS: usize = 300;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Search(String),
    /// Open (`on`) or close folder `id`'s list; the snapshot then carries (or drops) its links.
    Expand { id: u64, open: bool },
    Open { url: String, new_tab: bool },
    /// Open every bookmark in folder `id` (and its sub-folders) in new tabs.
    OpenAll(Vec<String>),
    Copy(String),
    Delete(u64),
    /// Put back what the last delete removed.
    Undo,
    Rename { id: u64, title: String },
    Update { id: u64, title: String, url: String },
    /// `parent` is a wire folder id: `0` the bar, [`OTHER_WIRE`] "Other bookmarks".
    AddFolder { parent: u64, title: String },
    AddLink { parent: u64, title: String, url: String },
    /// Bookmark the shown tab in `parent`.
    AddCurrent { parent: u64 },
    /// Move `id` into `parent` before `before` (`0`: at the end).
    Move { id: u64, parent: u64, before: u64 },
    /// Sort folder `parent`'s items by name, folders first.
    Sort { parent: u64 },
}

impl Action {
    /// Whether the action edits the store. Searching, expanding, opening and copying are read-only,
    /// so the panel must not rewrite (and fsync) `bookmarks-v2.txt` for them.
    pub(crate) fn mutates(&self) -> bool {
        !matches!(self, Action::Search(_) | Action::Expand { .. } | Action::Open { .. } | Action::OpenAll(_) | Action::Copy(_))
    }
}

/// What the snapshot shows besides the tree.
#[derive(Clone, Debug, Default)]
pub(crate) struct View<'a> {
    pub query: &'a str,
    /// `bm_panel_single_click`: panel items open with one click (else two).
    pub single_click: bool,
    /// Folders whose links are listed (the bar's always are).
    pub expanded: Option<&'a HashSet<u64>>,
    /// Why the last change was refused, or empty.
    pub error: &'a str,
    /// What the last delete removed, for its Undo ("" when there is nothing to undo).
    pub undo: &'a str,
    /// The shown tab is a page that can be bookmarked.
    pub can_add_current: bool,
}

/// `single_click` is the `bm_panel_single_click` option: panel items open with one click (else two).
#[cfg(test)]
pub(crate) fn snapshot(store: &BookmarkStore, query: &str, single_click: bool) -> String {
    snapshot_view(store, &View { query, single_click, ..View::default() }, |_| None)
}

/// The panel's snapshot. `favicon_of` maps a host to its cached PNG.
pub(crate) fn snapshot_view(store: &BookmarkStore, view: &View<'_>, favicon_of: impl Fn(&str) -> Option<Vec<u8>>) -> String {
    let searching = !view.query.trim().is_empty();
    let empty = HashSet::new();
    let expanded = view.expanded.unwrap_or(&empty);
    let mut hosts: Vec<String> = Vec::new();
    let mut total = 0usize;
    protocol::snap_json("bookmarks", |out| {
        let mut used = 0usize;
        out.array("items", |items| {
            if searching {
                let found = search(store, view.query);
                total = found.len();
                for item in found.into_iter().take(MAX_RESULTS) {
                    let path = store.folder_path(item);
                    let text = item_json(store, item, Some(&path));
                    if used + text.len() > ITEM_BUDGET {
                        break;
                    }
                    used += text.len();
                    note_host(&mut hosts, item);
                    items.raw(&text);
                }
                return;
            }
            // Every folder first (the tree's shape, the folder counts, the move targets), then the
            // links of the bar and of each open folder, in tree order, while the budget lasts.
            for item in store.items().iter().filter(|i| i.kind == BookmarkKind::Folder) {
                let text = item_json(store, item, None);
                if used + text.len() > ITEM_BUDGET {
                    break;
                }
                used += text.len();
                items.raw(&text);
            }
            let mut open: Vec<Option<u64>> = vec![None];
            open.extend(tree_order_folders(store).into_iter().filter(|id| expanded.contains(id) && ancestors_open(store, *id, expanded)).map(Some));
            'folders: for parent in open {
                for item in store.items().iter().filter(|i| i.parent == parent && i.kind != BookmarkKind::Folder) {
                    let text = item_json(store, item, None);
                    if used + text.len() > ITEM_BUDGET {
                        break 'folders;
                    }
                    used += text.len();
                    note_host(&mut hosts, item);
                    items.raw(&text);
                }
            }
        });
        out.string("query", view.query);
        out.null("currentFolder");
        out.bool("singleClick", view.single_click);
        out.uint("other", store.other_wire());
        out.array("expanded", |list| {
            let mut ids: Vec<u64> = expanded.iter().copied().filter(|id| store.items().iter().any(|i| i.id == *id)).collect();
            ids.sort_unstable();
            for id in ids {
                list.uint(id);
            }
        });
        out.uint("total", total as u64);
        out.string("error", json::capped(view.error, 240));
        out.string("undo", json::capped(view.undo, 160));
        out.bool("canAdd", view.can_add_current);
        out.object("favs", |favs| {
            let (mut count, mut bytes) = (0, 0);
            for host in &hosts {
                if count == MAX_FAV_HOSTS {
                    break;
                }
                let Some(uri) = favicon_of(host).as_deref().and_then(protocol::favicon_data_uri) else { continue };
                if bytes + uri.len() > MAX_FAV_TOTAL_BYTES {
                    continue;
                }
                bytes += uri.len();
                count += 1;
                favs.string(host, &uri);
            }
        });
    })
    .unwrap_or_else(|| Json::object(|_| {}))
}

fn note_host(hosts: &mut Vec<String>, item: &Bookmark) {
    if let BookmarkKind::Link { favicon_host, .. } = &item.kind {
        if !favicon_host.is_empty() && favicon_host.len() <= 253 && !hosts.contains(favicon_host) {
            hosts.push(favicon_host.clone());
        }
    }
}

fn item_json(store: &BookmarkStore, item: &Bookmark, path: Option<&Vec<String>>) -> String {
    Json::object(|out| {
        out.uint("id", item.id);
        if let Some(parent) = item.parent { out.uint("parent", parent); } else { out.null("parent"); }
        out.string("title", json::capped(&item.title, MAX_TITLE_CHARS));
        match &item.kind {
            BookmarkKind::Folder => {
                out.string("kind", "folder");
                out.uint("n", store.items().iter().filter(|i| i.parent == Some(item.id)).count() as u64);
            }
            BookmarkKind::Link { url, favicon_host, .. } => {
                out.string("kind", "link");
                out.string("url", json::capped(url, MAX_URL_CHARS));
                out.string("host", json::capped(favicon_host, 253));
            }
        }
        if let Some(path) = path { out.array("folderPath", |parts| for part in path { parts.string(json::capped(part, 80)); }); }
    })
}

/// Folder ids in tree order (parents before children).
fn tree_order_folders(store: &BookmarkStore) -> Vec<u64> {
    fn walk(store: &BookmarkStore, parent: Option<u64>, out: &mut Vec<u64>, depth: usize) {
        if depth > 64 {
            return;
        }
        for item in store.items().iter().filter(|i| i.parent == parent && i.kind == BookmarkKind::Folder) {
            out.push(item.id);
            walk(store, Some(item.id), out, depth + 1);
        }
    }
    let mut out = Vec::new();
    walk(store, None, &mut out, 0);
    out
}

fn ancestors_open(store: &BookmarkStore, id: u64, expanded: &HashSet<u64>) -> bool {
    let mut at = store.items().iter().find(|i| i.id == id).and_then(|i| i.parent);
    let mut steps = 0;
    while let Some(parent) = at {
        if !expanded.contains(&parent) || steps > store.items().len() {
            return false;
        }
        steps += 1;
        at = store.items().iter().find(|i| i.id == parent).and_then(|i| i.parent);
    }
    true
}

/// Every item whose name, address or folder path holds all of the query's words (case-insensitive),
/// links before folders and names that start with the query first.
pub(crate) fn search<'a>(store: &'a BookmarkStore, query: &str) -> Vec<&'a Bookmark> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let first = words[0].clone();
    let mut found: Vec<(u8, &Bookmark)> = store
        .items()
        .iter()
        .filter_map(|item| {
            let title = item.title.to_lowercase();
            let url = match &item.kind { BookmarkKind::Link { url, .. } => url.to_lowercase(), BookmarkKind::Folder => String::new() };
            let path = store.folder_path(item).join("/").to_lowercase();
            words.iter().all(|w| title.contains(w) || url.contains(w) || path.contains(w)).then(|| {
                let rank = u8::from(item.kind == BookmarkKind::Folder) * 2 + u8::from(!title.starts_with(&first));
                (rank, item)
            })
        })
        .collect();
    found.sort_by_key(|(rank, _)| *rank);
    found.into_iter().map(|(_, item)| item).collect()
}

pub(crate) fn map_command(store: &BookmarkStore, fields: PanelFields) -> Option<Action> {
    let only = |id: bool, parent: bool, before: bool, text: bool, url: bool, on: bool| {
        fields.id.is_some() == id && fields.parent.is_some() == parent
            && fields.before.is_some() == before && fields.text.is_some() == text
            && fields.url.is_some() == url && fields.n.is_none() && fields.on.is_some() == on
    };
    let exists = |id: u64| store.items().iter().any(|item| item.id == id);
    let folder_wire = |wire: u64| wire == 0 || wire == OTHER_WIRE || store.items().iter().any(|item| item.id == wire && item.kind == BookmarkKind::Folder);
    match fields.a.as_str() {
        "search" if only(false, false, false, true, false, false) => Some(Action::Search(fields.text?.chars().take(200).collect())),
        "expand" if only(true, false, false, false, false, true) => {
            let id = fields.id?;
            store.items().iter().any(|i| i.id == id && i.kind == BookmarkKind::Folder).then_some(Action::Expand { id, open: fields.on? })
        }
        "open" | "newtab" | "copy" if only(true, false, false, false, false, false) => {
            let id = fields.id?;
            let item = store.items().iter().find(|item| item.id == id)?;
            let BookmarkKind::Link { url, .. } = &item.kind else { return None; };
            Some(match fields.a.as_str() {
                "copy" => Action::Copy(url.clone()),
                a => Action::Open { url: url.clone(), new_tab: a == "newtab" },
            })
        }
        "openall" if only(true, false, false, false, false, false) => {
            let id = fields.id?;
            let item = store.items().iter().find(|item| item.id == id)?;
            matches!(item.kind, BookmarkKind::Folder).then_some(())?;
            let urls = links_under(store.items(), id);
            (!urls.is_empty()).then_some(Action::OpenAll(urls))
        }
        "delete" if only(true, false, false, false, false, false) => {
            let id = fields.id?;
            (exists(id) && !store.is_permanent(id)).then_some(Action::Delete(id))
        }
        "undo" if only(false, false, false, false, false, false) => Some(Action::Undo),
        "rename" if only(true, false, false, true, false, false) => {
            let id = fields.id?;
            exists(id).then_some(Action::Rename { id, title: fields.text? })
        }
        "update" if only(true, false, false, true, true, false) => {
            let id = fields.id?;
            store.items().iter().any(|item| item.id == id && matches!(&item.kind, BookmarkKind::Link { .. }))
                .then_some(Action::Update { id, title: fields.text?, url: fields.url? })
        }
        "addfolder" | "addlink" if only(false, true, false, true, fields.a == "addlink", false) => {
            let parent = fields.parent?;
            folder_wire(parent).then_some(())?;
            if fields.a == "addfolder" { Some(Action::AddFolder { parent, title: fields.text? }) }
            else { Some(Action::AddLink { parent, title: fields.text?, url: fields.url? }) }
        }
        "addcurrent" if only(false, true, false, false, false, false) => {
            let parent = fields.parent?;
            folder_wire(parent).then_some(Action::AddCurrent { parent })
        }
        "sort" if only(false, true, false, false, false, false) => {
            let parent = fields.parent?;
            folder_wire(parent).then_some(Action::Sort { parent })
        }
        "move" if only(true, true, true, false, false, false) => {
            let (id, parent, before) = (fields.id?, fields.parent?, fields.before?);
            (exists(id) && folder_wire(parent) && (before == 0 || exists(before))).then_some(Action::Move { id, parent, before })
        }
        _ => None,
    }
}

/// Applies a mutating action. `Ok` carries what a delete removed (for its Undo); `Err` says, in
/// words for the panel, why nothing changed. The store is left as it was on an error.
pub(crate) fn mutate(store: &mut BookmarkStore, action: Action) -> Result<Option<Removed>, String> {
    let before = store.clone();
    let result = mutate_inner(store, action);
    if result.is_err() {
        *store = before;
    }
    result
}

fn mutate_inner(store: &mut BookmarkStore, action: Action) -> Result<Option<Removed>, String> {
    let explain = |store: &BookmarkStore, error: std::io::Error, url: &str| explain(store, &error, url);
    match action {
        Action::Delete(id) => {
            if store.is_permanent(id) {
                return Err(format!("{} cannot be deleted.", crate::bookmarks::OTHER_TITLE));
            }
            let removed = store.take(id);
            Ok((!removed.is_empty()).then_some(removed))
        }
        Action::Rename { id, title } => {
            let snapshot = store.clone();
            store.rename(id, &title).map_err(|e| explain(&snapshot, e, ""))?;
            Ok(None)
        }
        Action::Update { id, title, url } => {
            let snapshot = store.clone();
            store.update_bookmark(id, &title, &url).map_err(|e| explain(&snapshot, e, &url))?;
            Ok(None)
        }
        Action::AddFolder { parent, title } => {
            let parent = store.resolve_parent(parent).map_err(|e| explain(&BookmarkStore::new(), e, ""))?;
            let snapshot = store.clone();
            store.add_folder(parent, &title).map_err(|e| explain(&snapshot, e, ""))?;
            Ok(None)
        }
        Action::AddLink { parent, title, url } => {
            let parent = store.resolve_parent(parent).map_err(|e| explain(&BookmarkStore::new(), e, ""))?;
            let snapshot = store.clone();
            store.add_bookmark(parent, &title, &url).map_err(|e| explain(&snapshot, e, &url))?;
            store.set_last_folder(parent);
            Ok(None)
        }
        Action::Move { id, parent, before } => {
            let parent = store.resolve_parent(parent).map_err(|e| explain(&BookmarkStore::new(), e, ""))?;
            let siblings: Vec<u64> = store.items().iter().filter(|i| i.parent == parent && i.id != id).map(|i| i.id).collect();
            let index = if before == 0 { siblings.len() } else { siblings.iter().position(|s| *s == before).ok_or("That place is gone; try again.")? };
            let snapshot = store.clone();
            store.move_item(id, parent, index).map_err(|e| explain(&snapshot, e, ""))?;
            Ok(None)
        }
        Action::Sort { parent } => {
            let parent = store.resolve_parent(parent).map_err(|e| explain(&BookmarkStore::new(), e, ""))?;
            store.sort_folder(parent);
            Ok(None)
        }
        _ => Ok(None),
    }
}

/// A store error in the panel's words.
pub(crate) fn explain(store: &BookmarkStore, error: &std::io::Error, url: &str) -> String {
    match error.to_string().as_str() {
        "invalid bookmark URL" => "Enter a web address that starts with http:// or https://.".to_owned(),
        "invalid bookmark title" => "Give it a name.".to_owned(),
        "bookmark already exists" => match store.find_url(url) {
            Some(item) => {
                let path = store.folder_path(item);
                let folder = path.last().map_or("Bookmarks bar".to_owned(), String::clone);
                format!("That page is already bookmarked as \u{201c}{}\u{201d} in {folder}.", json::capped(&item.title, 60))
            }
            None => "That page is already bookmarked.".to_owned(),
        },
        "too many bookmarks" => "Bowser keeps up to 5,000 bookmarks and folders. Delete some to add more.".to_owned(),
        "cannot move a folder into its descendant" => "A folder cannot go inside itself.".to_owned(),
        "this folder cannot be renamed" | "this folder cannot be moved" => format!("{} cannot be renamed or moved.", crate::bookmarks::OTHER_TITLE),
        "parent folder not found" => "That folder is gone.".to_owned(),
        other => format!("Bookmarks could not be changed: {other}."),
    }
}

/// Most bookmarks one "open all" opens.
pub(crate) const MAX_OPEN_ALL: usize = 100;

/// The addresses of every bookmark inside folder `folder`, in tree order, sub-folders included.
pub(crate) fn links_under(items: &[Bookmark], folder: u64) -> Vec<String> {
    let mut urls = Vec::new();
    let mut pending = vec![folder];
    while let Some(parent) = pending.pop() {
        let mut later = Vec::new();
        for item in items.iter().filter(|item| item.parent == Some(parent)) {
            match &item.kind {
                BookmarkKind::Link { url, .. } => urls.push(url.clone()),
                BookmarkKind::Folder => later.push(item.id),
            }
        }
        pending.extend(later.into_iter().rev());
        if urls.len() >= MAX_OPEN_ALL {
            break;
        }
    }
    urls.truncate(MAX_OPEN_ALL);
    urls
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bookmarks::BookmarkStore;

    fn fixture() -> BookmarkStore {
        let mut store = BookmarkStore::new();
        let folder = store.add_folder(None, "Work").unwrap();
        store.add_bookmark(Some(folder), "Docs", "https://example.com/docs").unwrap();
        store
    }

    #[test]
    fn open_all_collects_a_folder_s_links_including_sub_folders() {
        let mut store = BookmarkStore::new();
        let reading = store.add_folder(None, "Reading").unwrap();
        let rust = store.add_folder(Some(reading), "Rust").unwrap();
        store.add_bookmark(Some(reading), "A", "https://a.example/").unwrap();
        store.add_bookmark(Some(rust), "B", "https://b.example/").unwrap();
        store.add_bookmark(None, "C", "https://c.example/").unwrap();
        assert_eq!(links_under(store.items(), reading), ["https://a.example/", "https://b.example/"]);
        assert_eq!(map_command(&store, command("openall", Some(reading), None, None, None)), Some(Action::OpenAll(vec!["https://a.example/".into(), "https://b.example/".into()])));
        assert_eq!(map_command(&store, command("openall", Some(3), None, None, None)), None, "a link is not a folder");
        let empty = store.add_folder(None, "Empty").unwrap();
        assert_eq!(map_command(&store, command("openall", Some(empty), None, None, None)), None, "nothing to open");
        let mut big = BookmarkStore::new();
        let folder = big.add_folder(None, "Big").unwrap();
        for n in 0..150 {
            big.add_bookmark(Some(folder), "x", &format!("https://x{n}.example/")).unwrap();
        }
        assert_eq!(links_under(big.items(), folder).len(), MAX_OPEN_ALL);
    }

    #[test]
    fn snapshot_keeps_root_null_folder_counts_and_the_other_root() {
        let snapshot = snapshot(&fixture(), "", true);
        assert!(snapshot.contains(r#""parent":null"#));
        assert!(snapshot.contains(r#""n":1"#), "{snapshot}");
        assert!(snapshot.contains(r#""singleClick":true"#));
        assert!(snapshot.contains(&format!(r#""other":{OTHER_WIRE}"#)), "no Other bookmarks yet: the wire id");
        assert!(!snapshot.contains("https://example.com/docs"), "a closed folder's links are not sent");
        let open: HashSet<u64> = [1].into_iter().collect();
        let view = View { single_click: false, expanded: Some(&open), ..View::default() };
        let shown = snapshot_view(&fixture(), &view, |_| None);
        assert!(shown.contains(r#""url":"https://example.com/docs""#), "{shown}");
        assert!(shown.contains(r#""singleClick":false"#));
        assert!(shown.contains(r#""expanded":[1]"#));
    }

    /// A tree far past what one message may carry (the page drops anything over 256 KiB) still
    /// sends a snapshot that fits: every folder, then as many links as the budget allows.
    #[test]
    fn a_huge_tree_still_fits_one_message() {
        let mut store = BookmarkStore::new();
        let folder = store.add_folder(None, "Imported").unwrap();
        let long = "x".repeat(300);
        for n in 0..4000 {
            store.add_bookmark(if n % 2 == 0 { None } else { Some(folder) }, &format!("Site {n} {long}"), &format!("https://site{n}.example/{long}")).unwrap();
        }
        let open: HashSet<u64> = [folder].into_iter().collect();
        let view = View { expanded: Some(&open), ..View::default() };
        let json = snapshot_view(&store, &view, |_| Some(vec![0; 10]));
        assert!(json.len() < 256 * 1024 - 8 * 1024, "{} bytes", json.len());
        assert!(json.contains(r#""title":"Imported""#), "folders always come");
        let found = snapshot_view(&store, &View { query: "site", ..View::default() }, |_| None);
        assert!(found.len() < 256 * 1024 - 8 * 1024);
        assert!(found.contains(r#""total":4000"#));
    }

    #[test]
    fn search_matches_every_word_in_name_address_or_folder() {
        let mut store = fixture();
        store.add_bookmark(None, "Rust book", "https://doc.rust-lang.org/book/").unwrap();
        let found = |q: &str| search(&store, q).iter().map(|i| i.title.clone()).collect::<Vec<_>>();
        assert_eq!(found("work docs"), ["Docs"], "folder path and title");
        assert_eq!(found("RUST lang"), ["Rust book"]);
        assert!(found("nothing here").is_empty());
        assert_eq!(found("work"), ["Docs", "Work"], "links before folders");
    }

    #[test]
    fn command_mapper_requires_live_targets_and_sibling_destinations() {
        let store = fixture();
        assert_eq!(map_command(&store, command("newtab", Some(2), None, None, None)), Some(Action::Open { url: "https://example.com/docs".into(), new_tab: true }));
        assert_eq!(map_command(&store, command("copy", Some(2), None, None, None)), Some(Action::Copy("https://example.com/docs".into())));
        assert_eq!(map_command(&store, command("delete", Some(90), None, None, None)), None);
        assert_eq!(map_command(&store, command("move", Some(2), Some(0), Some(90), None)), None);
        assert_eq!(map_command(&store, command("move", Some(2), Some(OTHER_WIRE), Some(0), None)), Some(Action::Move { id: 2, parent: OTHER_WIRE, before: 0 }));
        assert_eq!(map_command(&store, command("addcurrent", None, Some(1), None, None)), Some(Action::AddCurrent { parent: 1 }));
        assert_eq!(map_command(&store, command("sort", None, Some(2), None, None)), None, "a link is not a folder");
        assert_eq!(map_command(&store, command("undo", None, None, None, None)), Some(Action::Undo));
        let expand = PanelFields { a: "expand".into(), id: Some(1), on: Some(true), ..PanelFields::default() };
        assert_eq!(map_command(&store, expand), Some(Action::Expand { id: 1, open: true }));
    }

    #[test]
    fn search_expand_open_and_copy_are_read_only_so_the_panel_does_not_save() {
        let store = fixture();
        let search = map_command(&store, command("search", None, None, None, Some("doc"))).unwrap();
        assert_eq!(search, Action::Search("doc".into()));
        assert!(!search.mutates(), "typing in the search box must not rewrite bookmarks-v2.txt");
        let open = map_command(&store, command("newtab", Some(2), None, None, None)).unwrap();
        assert!(!open.mutates());
        assert!(!Action::OpenAll(vec!["https://a.example/".into()]).mutates());
        assert!(!Action::Expand { id: 1, open: true }.mutates());
        assert!(map_command(&store, command("delete", Some(2), None, None, None)).unwrap().mutates());
        assert!(Action::Rename { id: 2, title: "x".into() }.mutates());
    }

    /// A refused change says why, and leaves the tree as it was.
    #[test]
    fn refusals_explain_themselves_and_change_nothing() {
        let mut store = fixture();
        let before = store.clone();
        let duplicate = mutate(&mut store, Action::AddLink { parent: 0, title: "Again".into(), url: "https://example.com/docs".into() }).unwrap_err();
        assert_eq!(duplicate, "That page is already bookmarked as \u{201c}Docs\u{201d} in Work.");
        let bad = mutate(&mut store, Action::AddLink { parent: OTHER_WIRE, title: "x".into(), url: "ftp://x".into() }).unwrap_err();
        assert!(bad.contains("http"), "{bad}");
        assert_eq!(store, before, "not even the Other bookmarks folder was made");
        assert_eq!(mutate(&mut store, Action::Move { id: 1, parent: 1, before: 0 }).unwrap_err(), "A folder cannot go inside itself.");
        // Into Other bookmarks: made on first use, and remembered as the last folder.
        mutate(&mut store, Action::AddLink { parent: OTHER_WIRE, title: "New".into(), url: "https://new.example/".into() }).unwrap();
        let other = store.other().unwrap();
        assert_eq!(store.last_folder(), Some(other));
        assert!(mutate(&mut store, Action::Delete(other)).is_err());
    }

    #[test]
    fn a_delete_hands_back_what_it_removed() {
        let mut store = fixture();
        let removed = mutate(&mut store, Action::Delete(1)).unwrap().unwrap();
        assert_eq!(removed.len(), 2);
        assert!(store.items().is_empty());
        store.restore(removed).unwrap();
        assert_eq!(store, fixture());
    }

    fn command(a: &str, id: Option<u64>, parent: Option<u64>, before: Option<u64>, text: Option<&str>) -> PanelFields {
        PanelFields { a: a.into(), id, parent, before, text: text.map(str::to_owned), ..PanelFields::default() }
    }
}
