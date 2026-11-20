//! Per-profile bookmark tree stored separately from the legacy importer file.
use std::{
    collections::HashMap,
    fs, io,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_BYTES: usize = 1024 * 1024;
const MAX_ITEMS: usize = 5000;
const MAX_FIELD: usize = 4096;
/// The row that keeps the next bookmark id: `next<TAB>id`.
const NEXT_ID_ROW: &str = "next";
/// The row that names the "Other bookmarks" folder: `other<TAB>id` (`0`: none yet). Its presence
/// also records that the one-time tidy of imported root folders ran. An older build skips it.
const OTHER_ROW: &str = "other";
/// The row that keeps the folder the last bookmark was saved to: `last<TAB>id` (`0`: the bar).
const LAST_ROW: &str = "last";
/// The name of the permanent folder beside the bookmarks bar (Chrome's and Edge's "Other bookmarks").
pub const OTHER_TITLE: &str = "Other bookmarks";
/// How the page names "Other bookmarks" before the folder exists: picking it creates the folder.
pub const OTHER_WIRE: u64 = 0xffff_ffff;
/// Top-level folders other browsers export for their bookmarks bar: imported, their contents go to
/// the bar itself (the top level) instead of one folder button on it.
/// Only names no person is likely to give a folder of their own: the tidy of existing trees uses
/// these alone.
const BAR_ALIASES: [&str; 4] = ["bookmarks bar", "bookmarks toolbar", "favorites bar", "favourites bar"];
/// Top-level folders other browsers export beside the bar: they become "Other bookmarks".
const OTHER_ALIASES: [&str; 5] = ["other bookmarks", "other favorites", "other favourites", "bookmarks menu", "unsorted bookmarks"];
/// A phone's bookmarks (Chrome "Mobile bookmarks"): kept, inside "Other bookmarks".
const MOBILE_ALIASES: [&str; 1] = ["mobile bookmarks"];
/// Firefox backups name their roots by key (`toolbar`, `menu`): `src/import/firefox.rs` maps those by
/// their fixed guids, so a folder a person named "Menu" is never taken for one.

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BookmarkKind {
    Folder,
    Link {
        url: String,
        favicon_host: String,
        created: u64,
    },
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bookmark {
    pub id: u64,
    pub parent: Option<u64>,
    pub title: String,
    pub kind: BookmarkKind,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BookmarkMatch {
    pub bookmark: Bookmark,
    pub folder_path: Vec<String>,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BookmarkStore {
    items: Vec<Bookmark>,
    next_id: u64,
    /// The "Other bookmarks" folder: a top-level folder the bar shows at its far end and the panel
    /// as its own root. It cannot be renamed, moved or deleted.
    other: Option<u64>,
    /// The folder the last bookmark was saved to; a new one goes there too (Chrome, Edge, Firefox).
    last_folder: Option<u64>,
    /// The one-time tidy of imported root folders ran (written as the `other` row).
    tidied: bool,
}
/// What [`BookmarkStore::take`] removed, to put back with [`BookmarkStore::restore`] (undo).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Removed {
    /// Each removed item with its index in the list before the removal, in index order.
    items: Vec<(usize, Bookmark)>,
}
impl Removed {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.items.iter().map(|(_, item)| item.id)
    }
    /// The item the removal was asked for (the first removed, the root of the sub-tree).
    pub fn root(&self) -> Option<&Bookmark> {
        let ids: Vec<u64> = self.ids().collect();
        self.items.iter().map(|(_, item)| item).find(|item| item.parent.is_none_or(|p| !ids.contains(&p)))
    }
}
impl BookmarkStore {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            next_id: 1,
            other: None,
            last_folder: None,
            tidied: false,
        }
    }
    pub fn items(&self) -> &[Bookmark] {
        &self.items
    }
    /// The "Other bookmarks" folder, when it exists.
    pub fn other(&self) -> Option<u64> {
        self.other
    }
    /// The id the page uses for "Other bookmarks": its own, or [`OTHER_WIRE`] before it exists.
    pub fn other_wire(&self) -> u64 {
        self.other.unwrap_or(OTHER_WIRE)
    }
    /// Whether `id` is a permanent folder (it cannot be renamed, moved or deleted).
    pub fn is_permanent(&self, id: u64) -> bool {
        self.other == Some(id)
    }
    /// The "Other bookmarks" folder, made (at the end of the top level) when it does not exist yet.
    pub fn ensure_other(&mut self) -> io::Result<u64> {
        if let Some(id) = self.other {
            return Ok(id);
        }
        let id = self.add_folder(None, OTHER_TITLE)?;
        self.other = Some(id);
        self.tidied = true;
        Ok(id)
    }
    /// A folder id from the page as a parent: `0` is the bar (the top level), [`OTHER_WIRE`] is
    /// "Other bookmarks" (made when missing); anything else must be a folder.
    pub fn resolve_parent(&mut self, wire: u64) -> io::Result<Option<u64>> {
        match wire {
            0 => Ok(None),
            OTHER_WIRE => self.ensure_other().map(Some),
            id => {
                self.check_parent(Some(id))?;
                Ok(Some(id))
            }
        }
    }
    /// The folder a new bookmark goes to: the one the last was saved to while it still exists,
    /// else the bar (the top level).
    pub fn last_folder(&self) -> Option<u64> {
        self.last_folder.filter(|id| self.items.iter().any(|item| item.id == *id && item.kind == BookmarkKind::Folder))
    }
    pub fn set_last_folder(&mut self, folder: Option<u64>) {
        self.last_folder = folder.filter(|id| self.items.iter().any(|item| item.id == *id && item.kind == BookmarkKind::Folder));
    }
    /// The link saved for `url` (compared as stored, after the same normalisation).
    pub fn find_url(&self, url: &str) -> Option<&Bookmark> {
        let url = valid_url(url)?;
        self.items.iter().find(|item| matches!(&item.kind, BookmarkKind::Link { url: stored, .. } if *stored == url))
    }
    /// The folder names from the top level down to `item`'s folder.
    pub fn folder_path(&self, item: &Bookmark) -> Vec<String> {
        let mut path = Vec::new();
        let mut parent = item.parent;
        while let Some(id) = parent {
            if path.len() >= self.items.len() {
                break;
            }
            let Some(folder) = self.items.iter().find(|i| i.id == id) else { break };
            path.push(folder.title.clone());
            parent = folder.parent;
        }
        path.reverse();
        path
    }
    pub fn load(profile_dir: &Path) -> io::Result<Self> {
        let v2 = profile_dir.join("bookmarks-v2.txt");
        match fs::read(&v2) {
            Ok(bytes) => {
                if bytes.len() > MAX_BYTES {
                    return Err(invalid_data("bookmarks exceed size limit"));
                }
                let text = String::from_utf8(bytes)
                    .map_err(|_| invalid_data("bookmarks are not UTF-8"))?;
                // A file that does not parse is an error, never an empty tree: every writer saves what
                // it loaded, so `Ok(empty)` here would replace the user's bookmarks on the next edit.
                let mut store = Self::parse(&text)?;
                // Once per profile: imported "Bookmarks bar" / "Other bookmarks" folders become the
                // bar itself and the "Other bookmarks" folder. Saved at once, so it never runs again.
                if !store.tidied && !store.items.is_empty() {
                    let before = (store.items.clone(), store.other);
                    store.tidy_imported_roots();
                    if (store.items.clone(), store.other) != before {
                        let _ = store.save(profile_dir);
                    } else {
                        // Nothing to tidy: the file stays as it is, and so does its time stamp.
                        store.tidied = false;
                    }
                }
                Ok(store)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let mut store = Self::new();
                let legacy =
                    crate::productivity::read_legacy_bookmarks(&profile_dir.join("bookmarks.txt"));
                let mut folders: HashMap<String, u64> = HashMap::new();
                for (title, url, folder) in legacy {
                    let mut parent = None;
                    let mut path = String::new();
                    let folder = import_folder(&folder);
                    for part in folder
                        .split('/')
                        .map(str::trim)
                        .filter(|p| !p.is_empty())
                        .take(16)
                    {
                        path.push('/');
                        path.push_str(part);
                        parent = Some(if let Some(id) = folders.get(&path) {
                            *id
                        } else {
                            let id = store.add_folder(parent, part)?;
                            folders.insert(path.clone(), id);
                            id
                        });
                    }
                    if !store.is_bookmarked(&url) {
                        store.add_bookmark(parent, &title, &url)?;
                    }
                }
                store.adopt_other();
                if !store.items.is_empty() {
                    store.save(profile_dir)?;
                }
                Ok(store)
            }
            Err(e) => Err(e),
        }
    }
    pub fn save(&self, profile_dir: &Path) -> io::Result<()> {
        atomic_write(
            &profile_dir.join("bookmarks-v2.txt"),
            self.serialize().as_bytes(),
        )?;
        // A deleted bookmark's speed-dial image goes with it, wherever it was deleted from.
        crate::start_page::forget_thumbs_of_deleted(profile_dir, self);
        Ok(())
    }
    pub fn add_folder(&mut self, parent: Option<u64>, title: &str) -> io::Result<u64> {
        self.check_parent(parent)?;
        let title = clean_title(title)?;
        if parent.is_none() {
            // A top-level folder made from here on is the user's own, whatever its name ("load"
            // already ran the one-time tidy of imported root folders), so it never runs again.
            self.tidied = true;
        }
        let id = self.alloc()?;
        self.items.push(Bookmark {
            id,
            parent,
            title,
            kind: BookmarkKind::Folder,
        });
        Ok(id)
    }
    pub fn add_bookmark(&mut self, parent: Option<u64>, title: &str, url: &str) -> io::Result<u64> {
        self.check_parent(parent)?;
        let title = clean_title(title)?;
        let url = valid_url(url).ok_or_else(|| invalid_input("invalid bookmark URL"))?;
        if self
            .items
            .iter()
            .any(|item| matches!(&item.kind,BookmarkKind::Link{url:existing,..} if existing==&url))
        {
            return Err(invalid_input("bookmark already exists"));
        }
        let host = host_key(&url);
        let created = now();
        let id = self.alloc()?;
        self.items.push(Bookmark {
            id,
            parent,
            title,
            kind: BookmarkKind::Link {
                url,
                favicon_host: host,
                created,
            },
        });
        Ok(id)
    }
    fn alloc(&mut self) -> io::Result<u64> {
        if self.items.len() >= MAX_ITEMS {
            return Err(invalid_input("too many bookmarks"));
        }
        let id = self.next_id.max(1);
        self.next_id = id
            .checked_add(1)
            .ok_or_else(|| invalid_input("bookmark id exhausted"))?;
        Ok(id)
    }
    fn check_parent(&self, parent: Option<u64>) -> io::Result<()> {
        if parent.is_some_and(|id| {
            !self
                .items
                .iter()
                .any(|i| i.id == id && matches!(i.kind, BookmarkKind::Folder))
        }) {
            Err(invalid_input("parent folder not found"))
        } else {
            Ok(())
        }
    }
    pub fn rename(&mut self, id: u64, title: &str) -> io::Result<bool> {
        if self.is_permanent(id) {
            return Err(invalid_input("this folder cannot be renamed"));
        }
        let title = clean_title(title)?;
        let Some(item) = self.items.iter_mut().find(|item| item.id == id) else {
            return Ok(false);
        };
        item.title = title;
        Ok(true)
    }
    /// Changes a link's title and address in place. The id, parent and created time are kept.
    /// `Ok(false)` means `id` is not a link. An invalid title or address, or an address another
    /// bookmark already uses, is an error and leaves the link unchanged.
    pub fn update_bookmark(&mut self, id: u64, title: &str, url: &str) -> io::Result<bool> {
        let title = clean_title(title)?;
        let url = valid_url(url).ok_or_else(|| invalid_input("invalid bookmark URL"))?;
        if self.items.iter().any(|item| {
            item.id != id
                && matches!(&item.kind, BookmarkKind::Link { url: existing, .. } if existing == &url)
        }) {
            return Err(invalid_input("bookmark already exists"));
        }
        let Some(item) = self.items.iter_mut().find(|item| item.id == id) else {
            return Ok(false);
        };
        let BookmarkKind::Link {
            url: stored,
            favicon_host,
            ..
        } = &mut item.kind
        else {
            return Ok(false);
        };
        *favicon_host = host_key(&url);
        *stored = url;
        item.title = title;
        Ok(true)
    }
    pub fn delete(&mut self, id: u64) -> usize {
        self.take(id).len()
    }
    /// Removes `id` and everything inside it, returning what went (with where it was) so
    /// [`Self::restore`] can put it back. A permanent folder is never removed.
    pub fn take(&mut self, id: u64) -> Removed {
        if self.is_permanent(id) || !self.items.iter().any(|i| i.id == id) {
            return Removed::default();
        }
        let mut ids = vec![id];
        let mut index = 0;
        while index < ids.len() {
            let parent = ids[index];
            for child in self.items.iter().filter(|i| i.parent == Some(parent)) {
                // A cycle must not grow the list forever.
                if !ids.contains(&child.id) {
                    ids.push(child.id);
                }
            }
            index += 1;
        }
        let mut removed = Removed::default();
        let mut index = 0;
        self.items.retain(|i| {
            let gone = ids.contains(&i.id);
            if gone {
                removed.items.push((index, i.clone()));
            }
            index += 1;
            !gone
        });
        removed
    }
    /// Puts back what [`Self::take`] removed, where it was. Items whose id is in use again, or
    /// whose folder is gone, are skipped; `Ok(n)` is how many came back.
    pub fn restore(&mut self, removed: Removed) -> io::Result<usize> {
        if self.items.len() + removed.items.len() > MAX_ITEMS {
            return Err(invalid_input("too many bookmarks"));
        }
        let mut coming: Vec<(usize, Bookmark)> = removed
            .items
            .into_iter()
            .filter(|(_, item)| !self.items.iter().any(|i| i.id == item.id))
            .filter(|(_, item)| match &item.kind {
                BookmarkKind::Link { url, .. } => self.find_url(url).is_none(),
                BookmarkKind::Folder => true,
            })
            .collect();
        // A folder that comes back may come after its contents in the list: parents are checked
        // against what is there and what is coming back, until nothing more drops out.
        loop {
            let folders: Vec<u64> = self
                .items
                .iter()
                .chain(coming.iter().map(|(_, item)| item))
                .filter(|i| i.kind == BookmarkKind::Folder)
                .map(|i| i.id)
                .collect();
            let before = coming.len();
            coming.retain(|(_, item)| item.parent.is_none_or(|p| folders.contains(&p)));
            if coming.len() == before {
                break;
            }
        }
        let back = coming.len();
        for (index, item) in coming {
            self.next_id = self.next_id.max(item.id.saturating_add(1));
            self.items.insert(index.min(self.items.len()), item);
        }
        Ok(back)
    }
    /// Sorts folder `parent`'s own items: folders first, then by name (case-insensitive, numbers
    /// in number order), as Chrome's "Sort by name" does. Sub-folders keep their order.
    pub fn sort_folder(&mut self, parent: Option<u64>) {
        let slots: Vec<usize> = self.items.iter().enumerate().filter(|(_, i)| i.parent == parent).map(|(n, _)| n).collect();
        let mut children: Vec<Bookmark> = slots.iter().map(|n| self.items[*n].clone()).collect();
        let other = self.other;
        children.sort_by(|a, b| {
            let rank = |i: &Bookmark| (Some(i.id) == other, !matches!(i.kind, BookmarkKind::Folder));
            rank(a).cmp(&rank(b)).then_with(|| natural_key(&a.title).cmp(&natural_key(&b.title)))
        });
        for (slot, child) in slots.into_iter().zip(children) {
            self.items[slot] = child;
        }
    }
    pub fn move_item(
        &mut self,
        id: u64,
        new_parent: Option<u64>,
        index: usize,
    ) -> io::Result<bool> {
        self.check_parent(new_parent)?;
        if self.is_permanent(id) {
            return Err(invalid_input("this folder cannot be moved"));
        }
        let Some(pos) = self.items.iter().position(|i| i.id == id) else {
            return Ok(false);
        };
        if matches!(self.items[pos].kind, BookmarkKind::Folder) {
            let mut descendants = vec![id];
            let mut n = 0;
            while n < descendants.len() {
                let p = descendants[n];
                descendants.extend(
                    self.items
                        .iter()
                        .filter(|i| i.parent == Some(p))
                        .map(|i| i.id),
                );
                n += 1;
            }
            if new_parent.is_some_and(|p| descendants.contains(&p)) {
                return Err(invalid_input("cannot move a folder into its descendant"));
            }
        }
        let mut item = self.items.remove(pos);
        item.parent = new_parent;
        let siblings = self.items.iter().filter(|i| i.parent == new_parent).count();
        let target = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.parent == new_parent)
            .nth(index.min(siblings))
            .map_or(self.items.len(), |(p, _)| p);
        self.items.insert(target, item);
        Ok(true)
    }
    pub fn reorder(&mut self, id: u64, index: usize) -> io::Result<bool> {
        let parent = self
            .items
            .iter()
            .find(|i| i.id == id)
            .ok_or_else(|| invalid_input("bookmark not found"))?
            .parent;
        self.move_item(id, parent, index)
    }
    /// The one-time tidy of what other browsers' exports left at the top level: a "Bookmarks bar"
    /// (or toolbar, or favorites bar) folder gives its contents to the bar itself, in its place; the
    /// first "Other bookmarks" (or "Bookmarks menu", "Other favorites") folder becomes the permanent
    /// "Other bookmarks" and later ones are merged into it. Nothing is deleted but emptied root
    /// folders. Marks the store tidied.
    pub fn tidy_imported_roots(&mut self) {
        self.tidied = true;
        let top: Vec<(u64, String)> = self
            .items
            .iter()
            .filter(|i| i.parent.is_none() && i.kind == BookmarkKind::Folder && Some(i.id) != self.other)
            .map(|i| (i.id, i.title.trim().to_lowercase()))
            .collect();
        for (id, title) in &top {
            if BAR_ALIASES.contains(&title.as_str()) {
                self.hoist(*id, None);
            } else if OTHER_ALIASES.contains(&title.as_str()) {
                match self.other {
                    None => {
                        self.other = Some(*id);
                        if let Some(item) = self.items.iter_mut().find(|i| i.id == *id) {
                            item.title = OTHER_TITLE.to_owned();
                        }
                    }
                    Some(other) => self.hoist(*id, Some(other)),
                }
            }
        }
        self.adopt_other();
        if let Some(other) = self.other {
            if let Some(item) = self.items.iter_mut().find(|i| i.id == other) {
                item.title = OTHER_TITLE.to_owned();
            }
        }
    }
    /// Moves folder `id`'s items into `into` (in the folder's place when `into` is its own parent,
    /// else at the end), then removes the emptied folder.
    fn hoist(&mut self, id: u64, into: Option<u64>) {
        let Some(at) = self.items.iter().position(|i| i.id == id) else { return };
        let parent = self.items[at].parent;
        let mut moved: Vec<Bookmark> = Vec::new();
        self.items.retain(|i| {
            if i.parent == Some(id) {
                moved.push(i.clone());
                false
            } else {
                true
            }
        });
        for item in &mut moved {
            item.parent = into;
        }
        let at = self.items.iter().position(|i| i.id == id).unwrap_or(self.items.len());
        let insert_at = if into == parent { at } else { self.items.len() };
        self.items.splice(insert_at..insert_at, moved);
        self.items.retain(|i| i.id != id);
        if self.last_folder == Some(id) {
            self.last_folder = into;
        }
    }
    /// Without a marked "Other bookmarks", a top-level folder of that name is it.
    fn adopt_other(&mut self) {
        if self.other.is_some() {
            return;
        }
        self.other = self
            .items
            .iter()
            .find(|i| i.parent.is_none() && i.kind == BookmarkKind::Folder && i.title.trim().eq_ignore_ascii_case(OTHER_TITLE))
            .map(|i| i.id);
    }
    /// Bookmarks a tab group, as Chrome's "Bookmark all tabs" does: a folder named `name` in
    /// `parent` (the one already there with that name, so bookmarking the group again brings it up
    /// to date) holding each of `pages` (`(title, address)`). A page bookmarked already stays where
    /// it is; one that is not a web address is skipped. Nothing changes on an error.
    pub fn bookmark_tabs(&mut self, parent: Option<u64>, name: &str, pages: &[(String, String)]) -> io::Result<TabsSaved> {
        self.check_parent(parent)?;
        let name = clean_title(name).or_else(|_| clean_title("Tab group"))?;
        let before = self.clone();
        let result = (|| {
            let existing = self
                .items
                .iter()
                .find(|i| i.parent == parent && i.kind == BookmarkKind::Folder && i.title.eq_ignore_ascii_case(&name) && !self.is_permanent(i.id))
                .map(|i| i.id);
            let folder = match existing {
                Some(id) => id,
                None => self.add_folder(parent, &name)?,
            };
            let mut saved = TabsSaved { folder, ..TabsSaved::default() };
            for (title, url) in pages {
                let Some(url) = valid_url(url) else { continue };
                if self.find_url(&url).is_some() {
                    saved.already += 1;
                    continue;
                }
                let title = if title.trim().is_empty() { url.clone() } else { title.clone() };
                self.add_bookmark(Some(folder), &title, &url)?;
                saved.added += 1;
            }
            Ok(saved)
        })();
        if result.is_err() {
            *self = before;
        }
        result
    }
    pub fn children(&self, parent: Option<u64>) -> Vec<&Bookmark> {
        self.items.iter().filter(|i| i.parent == parent).collect()
    }
    pub fn is_bookmarked(&self, url: &str) -> bool {
        valid_url(url).is_some_and(|url| {
            self.items
                .iter()
                .any(|i| matches!(&i.kind,BookmarkKind::Link{url:existing,..}if existing==&url))
        })
    }
    pub fn toggle(&mut self, parent: Option<u64>, title: &str, url: &str) -> io::Result<bool> {
        let normalized = valid_url(url).ok_or_else(|| invalid_input("invalid bookmark URL"))?;
        if let Some(id) = self
            .items
            .iter()
            .find(|i| matches!(&i.kind,BookmarkKind::Link{url,..}if url==&normalized))
            .map(|i| i.id)
        {
            self.delete(id);
            Ok(false)
        } else {
            self.add_bookmark(parent, title, &normalized)?;
            Ok(true)
        }
    }
    pub fn search(&self, query: &str) -> Vec<BookmarkMatch> {
        let q = query.trim().to_lowercase();
        self.items
            .iter()
            .filter(|item| {
                if q.is_empty() {
                    return false;
                }
                item.title.to_lowercase().contains(&q)
                    || match &item.kind {
                        BookmarkKind::Link { url, .. } => url.to_lowercase().contains(&q),
                        _ => false,
                    }
            })
            .map(|bookmark| {
                let mut path = Vec::new();
                let mut parent = bookmark.parent;
                while let Some(id) = parent {
                    // A cycle ends the walk instead of hanging it.
                    if path.len() >= self.items.len() {
                        break;
                    }
                    if let Some(folder) = self.items.iter().find(|i| i.id == id) {
                        path.push(folder.title.clone());
                        parent = folder.parent
                    } else {
                        break;
                    }
                }
                path.reverse();
                BookmarkMatch {
                    bookmark: bookmark.clone(),
                    folder_path: path,
                }
            })
            .collect()
    }
    /// Every link as `(title, address, folder path)`, in file order. The folder path is the names
    /// from the top level down, joined with `/` (the shape the command palette shows).
    pub fn flat_links(&self) -> Vec<(String, String, String)> {
        let folders: HashMap<u64, (Option<u64>, &str)> = self
            .items
            .iter()
            .filter(|i| matches!(i.kind, BookmarkKind::Folder))
            .map(|i| (i.id, (i.parent, i.title.as_str())))
            .collect();
        let mut paths: HashMap<Option<u64>, String> = HashMap::new();
        self.items
            .iter()
            .filter_map(|item| {
                let BookmarkKind::Link { url, .. } = &item.kind else {
                    return None;
                };
                let folder = paths
                    .entry(item.parent)
                    .or_insert_with(|| {
                        let mut names = Vec::new();
                        let mut at = item.parent;
                        while let Some((parent, title)) = at.and_then(|id| folders.get(&id)) {
                            // A cycle (never written by this module) ends the walk.
                            if names.len() > folders.len() {
                                break;
                            }
                            names.push(*title);
                            at = *parent;
                        }
                        names.reverse();
                        names.join("/")
                    })
                    .clone();
                Some((item.title.clone(), url.clone(), folder))
            })
            .collect()
    }
    /// Adds many links at once (an import) as `(folder path, title, address)`. The folder path's
    /// `/`-separated names are matched (ASCII case-insensitively) against the folders already
    /// there and created when missing. An address already bookmarked counts as a duplicate; one
    /// that is not a web address, or a link with neither title nor address usable, is skipped.
    /// Links past the item or file-size limit are counted in `limited` and not added, so the saved
    /// file always loads again. Linear in the number of bookmarks.
    pub fn import_links<'a>(
        &mut self,
        links: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>,
    ) -> ImportCounts {
        let mut counts = ImportCounts::default();
        let mut urls: std::collections::HashSet<String> = self
            .items
            .iter()
            .filter_map(|i| match &i.kind {
                BookmarkKind::Link { url, .. } => Some(url.clone()),
                BookmarkKind::Folder => None,
            })
            .collect();
        let mut folders: HashMap<(Option<u64>, String), u64> = self
            .items
            .iter()
            .filter(|i| matches!(i.kind, BookmarkKind::Folder))
            .map(|i| ((i.parent, i.title.to_ascii_lowercase()), i.id))
            .collect();
        let mut bytes = self.serialize().len();
        let mut full = false;
        for (folder, title, url) in links {
            if full {
                counts.limited += 1;
                continue;
            }
            let folder = import_folder(folder);
            let Some(url) = valid_url(url.trim()) else {
                counts.skipped += 1;
                continue;
            };
            if urls.contains(&url) {
                counts.duplicates += 1;
                continue;
            }
            let Ok(title) = clean_title(title).or_else(|_| clean_title(&url)) else {
                counts.skipped += 1;
                continue;
            };
            let mut parent = None;
            for part in folder
                .split('/')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .take(16)
            {
                let Ok(name) = clean_title(part) else { break };
                let key = (parent, name.to_ascii_lowercase());
                if let Some(id) = folders.get(&key) {
                    parent = Some(*id);
                    continue;
                }
                let item = Bookmark {
                    id: 0,
                    parent,
                    title: name,
                    kind: BookmarkKind::Folder,
                };
                let Some(id) = self.push_within(item, &mut bytes) else {
                    full = true;
                    break;
                };
                folders.insert(key, id);
                parent = Some(id);
            }
            if full {
                counts.limited += 1;
                continue;
            }
            let item = Bookmark {
                id: 0,
                parent,
                title,
                kind: BookmarkKind::Link {
                    favicon_host: host_key(&url),
                    url: url.clone(),
                    created: now(),
                },
            };
            if self.push_within(item, &mut bytes).is_none() {
                full = true;
                counts.limited += 1;
                continue;
            }
            urls.insert(url);
            counts.added += 1;
        }
        self.adopt_other();
        counts
    }
    /// Appends `item` with a fresh id when the tree stays inside the item and file-size limits;
    /// `bytes` is the serialized size so far and grows by the new row.
    fn push_within(&mut self, mut item: Bookmark, bytes: &mut usize) -> Option<u64> {
        let id = self.next_id.max(1);
        item.id = id;
        let row = row_text(&item).len();
        if self.items.len() >= MAX_ITEMS || *bytes + row > MAX_BYTES {
            return None;
        }
        self.next_id = id.checked_add(1)?;
        *bytes += row;
        self.items.push(item);
        Some(id)
    }
    pub fn serialize(&self) -> String {
        let mut out = String::from("bowser-bookmarks-v2\n");
        let mut highest = 0;
        for i in self.items.iter().take(MAX_ITEMS) {
            highest = highest.max(i.id);
            out.push_str(&row_text(i));
        }
        // Ids are keys elsewhere (nicknames and descriptions, speed-dial thumbnails), so the next id
        // is kept when deletions put it past the highest one left: otherwise the next load would
        // hand a deleted id to a new bookmark. An older build skips this row as one it cannot parse.
        let next = format!("{NEXT_ID_ROW}\t{}\n", self.next_id);
        if self.next_id > highest.saturating_add(1) && out.len() + next.len() <= MAX_BYTES {
            out.push_str(&next);
        }
        let other = self.other.filter(|id| self.items.iter().any(|i| i.id == *id));
        if self.tidied || other.is_some() {
            out.push_str(&format!("{OTHER_ROW}\t{}\n", other.unwrap_or(0)));
        }
        if let Some(last) = self.last_folder() {
            out.push_str(&format!("{LAST_ROW}\t{last}\n"));
        }
        out
    }
    pub fn parse(text: &str) -> io::Result<Self> {
        if text.len() > MAX_BYTES {
            return Err(invalid_data("bookmarks exceed size limit"));
        }
        let mut lines = text.lines();
        if lines.next() != Some("bowser-bookmarks-v2") {
            return Err(invalid_data("bad bookmarks header"));
        }
        let mut s = Self::new();
        // Ids kept so far, so a repeated id is found without scanning the list (the start page parses
        // this file for every new tab it rebuilds for).
        let mut seen = std::collections::HashSet::new();
        // A line that does not parse (a blank line, a short or hand-edited row) is skipped, never the
        // reason the whole tree loads empty: every writer saves what it loaded.
        type Row = (u64, Option<u64>, String, String, String, String, u64);
        let row = |line: &str| -> Option<Row> {
            let mut f = line.splitn(7, '\t');
            let id = f.next()?.parse::<u64>().ok()?;
            let parent = match f.next()? {
                "-" => None,
                x => Some(x.parse().ok()?),
            };
            let kind = f.next()?.to_owned();
            let title = unescape(f.next()?);
            let url = unescape(f.next()?);
            let host = unescape(f.next()?);
            let created = f.next()?.parse().ok()?;
            Some((id, parent, kind, title, url, host, created))
        };
        let mut rows = 0;
        for line in lines {
            if let Some(next) = line.strip_prefix(NEXT_ID_ROW).and_then(|rest| rest.strip_prefix('\t')) {
                if let Ok(next) = next.parse::<u64>() {
                    s.next_id = s.next_id.max(next);
                }
                continue;
            }
            if let Some(other) = line.strip_prefix(OTHER_ROW).and_then(|rest| rest.strip_prefix('\t')) {
                s.tidied = true;
                s.other = other.parse::<u64>().ok().filter(|id| *id != 0);
                continue;
            }
            if let Some(last) = line.strip_prefix(LAST_ROW).and_then(|rest| rest.strip_prefix('\t')) {
                s.last_folder = last.parse::<u64>().ok().filter(|id| *id != 0);
                continue;
            }
            if rows == MAX_ITEMS {
                continue;
            }
            rows += 1;
            let Some((id, parent, kind, title, url, host, created)) = row(line) else {
                continue;
            };
            if id == 0 || seen.contains(&id) || title.len() > MAX_FIELD {
                continue;
            }
            let kind = match kind.as_str() {
                "F" => BookmarkKind::Folder,
                "B" => {
                    let Some(url) = valid_url(&url) else { continue };
                    BookmarkKind::Link {
                        url,
                        favicon_host: host.chars().take(253).collect(),
                        created,
                    }
                }
                _ => continue,
            };
            s.next_id = s.next_id.max(id.saturating_add(1));
            seen.insert(id);
            s.items.push(Bookmark {
                id,
                parent,
                title,
                kind,
            });
        }
        let valid_ids = s
            .items
            .iter()
            .filter(|i| matches!(i.kind, BookmarkKind::Folder))
            .map(|i| i.id)
            .collect::<std::collections::HashSet<_>>();
        s.items
            .retain(|i| i.parent.is_none_or(|p| valid_ids.contains(&p)));
        // A folder that is its own ancestor (a damaged or hand-edited file) moves to the top level with
        // the rest of its cycle, so every walk up the tree ends and nothing is lost.
        let parents: HashMap<u64, Option<u64>> = s.items.iter().map(|i| (i.id, i.parent)).collect();
        let in_cycle = |id: u64| {
            let mut at = parents.get(&id).copied().flatten();
            for _ in 0..=parents.len() {
                match at {
                    None => return false,
                    Some(p) if p == id => return true,
                    Some(p) => at = parents.get(&p).copied().flatten(),
                }
            }
            false
        };
        let cyclic: std::collections::HashSet<u64> =
            s.items.iter().filter(|i| in_cycle(i.id)).map(|i| i.id).collect();
        for item in s.items.iter_mut().filter(|i| cyclic.contains(&i.id)) {
            item.parent = None;
        }
        // "Other bookmarks" is a folder at the top level, or there is none.
        let items = &s.items;
        s.other = s.other.filter(|id| items.iter().any(|i| i.id == *id && i.parent.is_none() && i.kind == BookmarkKind::Folder));
        s.adopt_other();
        Ok(s)
    }
}
/// An imported folder path with other browsers' root folders mapped onto Bowser's: the bookmarks
/// bar's own folder goes (its contents are the bar), the rest go to "Other bookmarks", a phone's to
/// "Other bookmarks/Mobile bookmarks".
fn import_folder(path: &str) -> String {
    let mut parts = path.split('/').map(str::trim).filter(|p| !p.is_empty());
    let Some(first) = parts.next() else { return String::new() };
    let rest: Vec<&str> = parts.collect();
    let key = first.to_lowercase();
    let head: Vec<&str> = if BAR_ALIASES.contains(&key.as_str()) {
        Vec::new()
    } else if OTHER_ALIASES.contains(&key.as_str()) {
        vec![OTHER_TITLE]
    } else if MOBILE_ALIASES.contains(&key.as_str()) {
        vec![OTHER_TITLE, "Mobile bookmarks"]
    } else {
        vec![first]
    };
    head.into_iter().chain(rest).collect::<Vec<_>>().join("/")
}
/// What [`BookmarkStore::bookmark_tabs`] did.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TabsSaved {
    /// The folder that holds the group's pages.
    pub folder: u64,
    /// Pages bookmarked into it now.
    pub added: usize,
    /// Pages that were bookmarked already (in this folder or elsewhere) and were left where they are.
    pub already: usize,
}
/// What [`BookmarkStore::import_links`] did with the links it was given.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ImportCounts {
    pub added: usize,
    pub duplicates: usize,
    pub skipped: usize,
    /// Not added because the tree reached its item or file-size limit.
    pub limited: usize,
}
/// A sort key that orders names case-insensitively with runs of digits by their value
/// ("Item 2" before "Item 10").
fn natural_key(text: &str) -> Vec<(u8, u128, String)> {
    let mut key = Vec::new();
    let mut chars = text.trim().chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(digit) = c.to_digit(10) {
            let mut value = u128::from(digit);
            while let Some(next) = chars.peek().and_then(|c| c.to_digit(10)) {
                value = value.saturating_mul(10).saturating_add(u128::from(next));
                chars.next();
            }
            key.push((0, value, String::new()));
        } else {
            key.push((1, 0, c.to_lowercase().collect()));
        }
    }
    key
}
/// One line of `bookmarks-v2.txt`.
fn row_text(i: &Bookmark) -> String {
    let (kind, url, host, created): (&str, &str, &str, u64) = match &i.kind {
        BookmarkKind::Folder => ("F", "", "", 0),
        BookmarkKind::Link {
            url,
            favicon_host,
            created,
        } => ("B", url, favicon_host, *created),
    };
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        i.id,
        i.parent.map_or("-".into(), |p| p.to_string()),
        kind,
        escape(&i.title),
        escape(url),
        escape(host),
        created
    )
}
fn clean_title(s: &str) -> io::Result<String> {
    let s = s.trim();
    if s.is_empty() || s.len() > MAX_FIELD || s.chars().any(char::is_control) {
        Err(invalid_input("invalid bookmark title"))
    } else {
        Ok(s.to_owned())
    }
}
fn valid_url(s: &str) -> Option<String> {
    if s.len() > 2048 {
        return None;
    }
    let u = url::Url::parse(s).ok()?;
    if !matches!(u.scheme(), "http" | "https") || !u.username().is_empty() || u.password().is_some()
    {
        None
    } else {
        Some(u.to_string())
    }
}
fn host_key(s: &str) -> String {
    url::Url::parse(s)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default()
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}
fn unescape(s: &str) -> String {
    let mut o = String::new();
    let mut c = s.chars();
    while let Some(x) = c.next() {
        if x != '\\' {
            o.push(x)
        } else {
            match c.next() {
                Some('t') => o.push('\t'),
                Some('n') => o.push('\n'),
                Some('r') => o.push('\r'),
                Some('\\') => o.push('\\'),
                Some(x) => {
                    o.push('\\');
                    o.push(x)
                }
                None => o.push('\\'),
            }
        }
    }
    o
}
fn invalid_input(s: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, s)
}
fn invalid_data(s: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, s)
}
fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > MAX_BYTES {
        return Err(invalid_input("bookmarks exceed size limit"));
    }
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?
    }
    let temp = path.with_extension("tmp");
    fs::write(&temp, bytes)?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        let a: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
        let b: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            MoveFileExW(
                windows::core::PCWSTR(a.as_ptr()),
                windows::core::PCWSTR(b.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )?;
        }
    }
    #[cfg(not(windows))]
    fs::rename(temp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_search_delete_and_cycle() {
        let mut s = BookmarkStore::new();
        let a = s.add_folder(None, "Read").unwrap();
        let b = s.add_folder(Some(a), "Rust").unwrap();
        let link = s
            .add_bookmark(Some(b), "Rust Book", "https://rust-lang.org/")
            .unwrap();
        assert_eq!(
            s.search("RUST")
                .into_iter()
                .find(|m| m.bookmark.id == link)
                .unwrap()
                .folder_path,
            vec!["Read", "Rust"]
        );
        assert!(s.move_item(a, Some(b), 0).is_err());
        assert_eq!(s.delete(a), 3);
        assert!(!s.is_bookmarked("https://rust-lang.org/"));
    }
    #[test]
    fn legacy_import_and_corrupt_v2_tolerance() {
        let dir = std::env::temp_dir().join(format!("bowser-bookmarks-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("bookmarks.txt"),
            "Rust\thttps://rust-lang.org/\tReading/Code\n",
        )
        .unwrap();
        let s = BookmarkStore::load(&dir).unwrap();
        assert_eq!(
            s.search("rust")
                .into_iter()
                .find(|m| matches!(m.bookmark.kind, BookmarkKind::Link { .. }))
                .unwrap()
                .folder_path,
            vec!["Reading", "Code"]
        );
        fs::write(dir.join("bookmarks-v2.txt"), b"bad").unwrap();
        assert!(BookmarkStore::load(&dir).is_err(), "a corrupt file is an error, not an empty tree");
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn a_malformed_line_is_skipped_and_every_good_row_is_kept() {
        let mut s = BookmarkStore::new();
        let folder = s.add_folder(None, "Read").unwrap();
        s.add_bookmark(Some(folder), "Example", "https://example.com/").unwrap();
        s.add_bookmark(None, "Other", "https://other.example/").unwrap();
        let good = s.serialize();
        let (header, rows) = good.split_once('\n').unwrap();
        let (first, rest) = rows.split_once('\n').unwrap();
        for bad in ["", "garbage line without tabs", "x\t-\tF\tBad id\t\t\t0", "9\t-\tF\tshort", "9\tx\tF\tBad parent\t\t\t0", "9\t-\tB\tBad time\thttps://t.example/\tt.example\tlater"] {
            let text = format!("{header}\n{first}\n{bad}\n{rest}\n");
            let loaded = BookmarkStore::parse(&text).unwrap_or_else(|e| panic!("{bad:?}: {e}"));
            assert_eq!(loaded, s, "{bad:?}");
        }
        assert!(BookmarkStore::parse(&format!("{good}\n")).unwrap() == s, "a trailing blank line");
    }
    #[test]
    fn a_folder_cycle_is_broken_on_load_so_search_and_delete_end() {
        // 1 and 2 are each other's parent; 3 is its own; a link sits in 1 and another in 3.
        let text = "bowser-bookmarks-v2\n\
            1\t2\tF\tA\t\t\t0\n\
            2\t1\tF\tB\t\t\t0\n\
            3\t3\tF\tSelf\t\t\t0\n\
            4\t1\tB\tLink\thttps://a.example/\ta.example\t1\n\
            5\t3\tB\tSelf link\thttps://b.example/\tb.example\t1\n";
        let mut s = BookmarkStore::parse(text).unwrap();
        assert_eq!(s.items().len(), 5, "nothing is dropped");
        assert!(s.items().iter().filter(|i| [1, 2, 3].contains(&i.id)).all(|i| i.parent.is_none()));
        assert_eq!(s.search("a.example")[0].folder_path, vec!["A"]);
        assert_eq!(s.search("b.example")[0].folder_path, vec!["Self"]);
        assert_eq!(s.delete(3), 2);
        assert_eq!(s.items().len(), 3);
    }
    #[test]
    fn search_and_delete_end_even_on_a_cycle_made_in_memory() {
        let mut s = BookmarkStore::new();
        let a = s.add_folder(None, "A").unwrap();
        let b = s.add_folder(Some(a), "B").unwrap();
        s.add_bookmark(Some(b), "Link", "https://a.example/").unwrap();
        s.items.iter_mut().find(|i| i.id == a).unwrap().parent = Some(b);
        assert_eq!(s.search("a.example").len(), 1);
        assert_eq!(s.delete(a), 3);
        assert!(s.items().is_empty());
    }
    #[test]
    fn round_trip() {
        let mut s = BookmarkStore::new();
        s.add_bookmark(None, "Example", "https://example.com/")
            .unwrap();
        assert_eq!(BookmarkStore::parse(&s.serialize()).unwrap(), s)
    }
    #[test]
    fn update_keeps_id_parent_and_created_time() {
        let mut s = BookmarkStore::new();
        let folder = s.add_folder(None, "Read").unwrap();
        let link = s
            .add_bookmark(Some(folder), "Old", "https://old.example/")
            .unwrap();
        let before = s.items().iter().find(|i| i.id == link).unwrap().clone();
        let BookmarkKind::Link { created, .. } = before.kind else {
            panic!("expected a link");
        };
        // Force a distinct created time so the test notices if it is overwritten.
        if let Some(item) = s.items.iter_mut().find(|i| i.id == link)
            && let BookmarkKind::Link { created, .. } = &mut item.kind
        {
            *created = 1234;
        }
        assert!(s.update_bookmark(link, "  New  ", "https://New.Example/path").unwrap());
        let after = s.items().iter().find(|i| i.id == link).unwrap();
        assert_eq!(after.title, "New");
        assert_eq!(after.parent, Some(folder));
        match &after.kind {
            BookmarkKind::Link { url, favicon_host, created: kept } => {
                assert_eq!(url, "https://new.example/path");
                assert_eq!(favicon_host, "new.example");
                assert_eq!(*kept, 1234);
            }
            other => panic!("expected a link, got {other:?}"),
        }
        assert_ne!(created, 1234);
        assert!(s.is_bookmarked("https://new.example/path"));
        assert!(!s.is_bookmarked("https://old.example/"));
    }
    #[test]
    fn update_rejects_bad_input_and_duplicates_without_changing_the_link() {
        let mut s = BookmarkStore::new();
        let a = s.add_bookmark(None, "A", "https://a.example/").unwrap();
        let b = s.add_bookmark(None, "B", "https://b.example/").unwrap();
        let folder = s.add_folder(None, "Folder").unwrap();
        assert!(s.update_bookmark(a, "A", "https://b.example/").is_err());
        assert!(s.update_bookmark(a, "A", "javascript:alert(1)").is_err());
        assert!(s.update_bookmark(a, "   ", "https://c.example/").is_err());
        // Updating a link to its own address is not a duplicate.
        assert!(s.update_bookmark(a, "A2", "https://a.example/").unwrap());
        assert!(!s.update_bookmark(folder, "X", "https://x.example/").unwrap());
        assert!(!s.update_bookmark(999, "X", "https://x.example/").unwrap());
        let item = |id| s.items().iter().find(|i| i.id == id).unwrap().clone();
        assert_eq!(item(a).title, "A2");
        assert_eq!(item(b).title, "B");
        assert!(matches!(item(folder).kind, BookmarkKind::Folder));
    }

    /// R-5: every bookmark action reloads the file. Deleting the newest bookmark and then adding
    /// another must not hand out the deleted id again, or the new bookmark inherits the old one's
    /// nickname, description and speed-dial thumbnail (all keyed by id).
    #[test]
    fn a_deleted_bookmarks_id_is_never_handed_out_again() {
        let mut store = BookmarkStore::parse("bowser-bookmarks-v2\n").unwrap();
        store.add_bookmark(None, "Bank", "https://bank.example/").unwrap();
        let mail = store.add_bookmark(None, "Work mail", "https://mail.work.example/").unwrap();
        store.delete(mail);
        let mut reloaded = BookmarkStore::parse(&store.serialize()).unwrap();
        let other = reloaded.add_bookmark(None, "Other", "https://other.example/").unwrap();
        assert!(other > mail, "deleted id {mail}, new id {other}");
        // Still true after the next reload, and a file without deletions is written as before.
        let again = BookmarkStore::parse(&reloaded.serialize()).unwrap();
        assert_eq!(again, reloaded);
        let mut fresh = BookmarkStore::new();
        fresh.add_bookmark(None, "A", "https://a.example/").unwrap();
        assert_eq!(fresh.serialize().lines().count(), 2, "{}", fresh.serialize());
    }

    /// Chrome and Firefox exports carry their own root folders. Imported, the bar's contents are the
    /// bar (not one "Bookmarks bar" folder button on it) and the rest is "Other bookmarks".
    #[test]
    fn imported_root_folders_map_onto_the_bar_and_other_bookmarks() {
        let mut s = BookmarkStore::new();
        let counts = s.import_links([
            ("Bookmarks bar", "News", "https://news.example/"),
            ("Bookmarks bar / Work", "Mail", "https://mail.example/"),
            ("Other bookmarks", "Recipes", "https://food.example/"),
            ("Mobile bookmarks", "Phone", "https://phone.example/"),
            ("Bookmarks toolbar", "Firefox bar", "https://ff.example/"),
            ("Bookmarks menu / Reading", "Paper", "https://paper.example/"),
            ("Menu", "Pizza", "https://pizza.example/"),
        ]);
        assert_eq!(counts.added, 7);
        let path = |url: &str| s.folder_path(s.find_url(url).unwrap()).join("/");
        assert_eq!(path("https://news.example/"), "");
        assert_eq!(path("https://mail.example/"), "Work");
        assert_eq!(path("https://ff.example/"), "");
        assert_eq!(path("https://food.example/"), OTHER_TITLE);
        assert_eq!(path("https://phone.example/"), "Other bookmarks/Mobile bookmarks");
        assert_eq!(path("https://paper.example/"), "Other bookmarks/Reading");
        assert_eq!(path("https://pizza.example/"), "Menu", "a folder a person named Menu stays theirs");
        let other = s.other().expect("the imported folder is Other bookmarks");
        assert_eq!(s.items().iter().filter(|i| i.parent.is_none() && i.kind == BookmarkKind::Folder).count(), 3, "Work, Menu and Other bookmarks");
        assert!(s.rename(other, "Mine").is_err() && s.move_item(other, None, 0).is_err());
        assert_eq!(s.delete(other), 0, "a permanent folder is never deleted");
        let again = BookmarkStore::parse(&s.serialize()).unwrap();
        assert_eq!(again.other(), Some(other));
    }

    /// Bookmarks imported before this version sit in a "Bookmarks bar" folder: the first load tidies
    /// them once, in place, and the tidy is written so it never runs again.
    #[test]
    fn a_tree_imported_earlier_is_tidied_once_on_load() {
        let mut s = BookmarkStore::new();
        let first = s.add_bookmark(None, "First", "https://first.example/").unwrap();
        let bar = s.add_folder(None, "Bookmarks bar").unwrap();
        let a = s.add_bookmark(Some(bar), "A", "https://a.example/").unwrap();
        let sub = s.add_folder(Some(bar), "Sub").unwrap();
        let other = s.add_folder(None, "Other Bookmarks").unwrap();
        let o = s.add_bookmark(Some(other), "O", "https://o.example/").unwrap();
        let last = s.add_bookmark(None, "Last", "https://last.example/").unwrap();
        let dir = std::env::temp_dir().join(format!("bowser-bookmarks-tidy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // Written as an older build wrote it: no `other` row.
        let old: String = s.serialize().lines().filter(|l| !l.starts_with("other\t")).map(|l| format!("{l}\n")).collect();
        fs::write(dir.join("bookmarks-v2.txt"), old).unwrap();
        let loaded = BookmarkStore::load(&dir).unwrap();
        let top: Vec<u64> = loaded.children(None).iter().map(|i| i.id).collect();
        assert_eq!(top, vec![first, a, sub, other, last], "the bar folder's items take its place");
        assert_eq!(loaded.other(), Some(other));
        assert_eq!(loaded.items().iter().find(|i| i.id == o).unwrap().parent, Some(other));
        assert_eq!(loaded.items().iter().find(|i| i.id == other).unwrap().title, OTHER_TITLE);
        assert!(fs::read_to_string(dir.join("bookmarks-v2.txt")).unwrap().contains(&format!("\nother\t{other}\n")), "the tidy is saved");
        // A folder the user makes later with that name is left alone.
        let mut later = loaded.clone();
        let mine = later.add_folder(None, "Bookmarks bar").unwrap();
        later.save(&dir).unwrap();
        assert!(BookmarkStore::load(&dir).unwrap().items().iter().any(|i| i.id == mine));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn other_bookmarks_is_made_when_first_picked_and_the_last_folder_is_remembered() {
        let mut s = BookmarkStore::new();
        assert_eq!(s.other_wire(), OTHER_WIRE);
        assert_eq!(s.resolve_parent(0).unwrap(), None);
        let other = s.resolve_parent(OTHER_WIRE).unwrap().unwrap();
        assert_eq!(s.other(), Some(other));
        assert_eq!(s.resolve_parent(OTHER_WIRE).unwrap(), Some(other), "made once");
        assert!(s.resolve_parent(777).is_err());
        s.set_last_folder(Some(other));
        let back = BookmarkStore::parse(&s.serialize()).unwrap();
        assert_eq!(back.last_folder(), Some(other));
        s.set_last_folder(Some(999));
        assert_eq!(s.last_folder(), None, "a folder that is gone means the bar");
    }

    #[test]
    fn a_deleted_folder_comes_back_whole_and_in_place() {
        let mut s = BookmarkStore::new();
        s.add_bookmark(None, "Before", "https://before.example/").unwrap();
        let folder = s.add_folder(None, "Work").unwrap();
        let inner = s.add_folder(Some(folder), "Inner").unwrap();
        s.add_bookmark(Some(inner), "Deep", "https://deep.example/").unwrap();
        s.add_bookmark(Some(folder), "Mail", "https://mail.example/").unwrap();
        s.add_bookmark(None, "After", "https://after.example/").unwrap();
        // Put the folder after its contents in the list, as moves can.
        s.move_item(inner, Some(folder), 0).unwrap();
        let before = s.clone();
        let removed = s.take(folder);
        assert_eq!(removed.len(), 4);
        assert_eq!(removed.root().unwrap().id, folder);
        assert_eq!(s.items().len(), 2);
        assert_eq!(s.restore(removed).unwrap(), 4);
        assert_eq!(s, before);
    }

    #[test]
    fn sort_by_name_puts_folders_first_and_numbers_in_order() {
        let mut s = BookmarkStore::new();
        s.add_bookmark(None, "item 10", "https://ten.example/").unwrap();
        s.add_bookmark(None, "Item 2", "https://two.example/").unwrap();
        s.add_folder(None, "zeta").unwrap();
        s.add_bookmark(None, "apple", "https://apple.example/").unwrap();
        s.add_folder(None, "Alpha").unwrap();
        let other = s.ensure_other().unwrap();
        s.move_item(other, None, 0).ok();
        s.sort_folder(None);
        let names: Vec<&str> = s.children(None).iter().map(|i| i.title.as_str()).collect();
        assert_eq!(names, ["Alpha", "zeta", "apple", "Item 2", "item 10", OTHER_TITLE]);
    }

    #[test]
    fn a_tab_group_becomes_a_folder_and_again_updates_it() {
        let mut s = BookmarkStore::new();
        s.add_bookmark(None, "Kept", "https://kept.example/").unwrap();
        let pages = |list: &[(&str, &str)]| list.iter().map(|(t, u)| (t.to_string(), u.to_string())).collect::<Vec<_>>();
        let first = s.bookmark_tabs(None, "GitHub", &pages(&[("Repo", "https://github.com/a"), ("", "https://github.com/b"), ("Kept", "https://kept.example/"), ("Local", "file:///c:/x")])).unwrap();
        assert_eq!((first.added, first.already), (2, 1), "the bookmarked page stays put, a file is skipped");
        let names: Vec<&str> = s.children(Some(first.folder)).iter().map(|i| i.title.as_str()).collect();
        assert_eq!(names, ["Repo", "https://github.com/b"]);
        let again = s.bookmark_tabs(None, "github", &pages(&[("Repo", "https://github.com/a"), ("Issues", "https://github.com/c")])).unwrap();
        assert_eq!(again.folder, first.folder, "the same folder, brought up to date");
        assert_eq!((again.added, again.already), (1, 1));
        assert!(s.bookmark_tabs(Some(999), "X", &[]).is_err());
    }
}
