use super::super::{json::Json, protocol::{self, PanelFields}};
use crate::bookmarks::{Bookmark, BookmarkKind, BookmarkStore};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Search(String),
    Open { url: String, new_tab: bool },
    Delete(u64),
    Rename { id: u64, title: String },
    Update { id: u64, title: String, url: String },
    AddFolder { parent: Option<u64>, title: String },
    AddLink { parent: Option<u64>, title: String, url: String },
    Move { id: u64, parent: Option<u64>, index: usize },
}

pub(crate) fn snapshot(store: &BookmarkStore, query: &str) -> String {
    let searching = !query.trim().is_empty();
    let matched = if searching { Some(store.search(query)) } else { None };
    protocol::snap_json("bookmarks", |out| {
        out.array("items", |items| {
            if let Some(matches) = &matched {
                for found in matches {
                    write_item(items, &found.bookmark, Some(&found.folder_path));
                }
            } else {
                for item in store.items() { write_item(items, item, None); }
            }
        });
        out.string("query", query);
        out.null("currentFolder");
    }).unwrap_or_else(|| Json::object(|_| {}))
}

fn write_item(items: &mut super::super::json::Array<'_>, item: &Bookmark, path: Option<&Vec<String>>) {
    items.object(|out| {
        out.uint("id", item.id);
        if let Some(parent) = item.parent { out.uint("parent", parent); } else { out.null("parent"); }
        out.string("title", &item.title);
        match &item.kind {
            BookmarkKind::Folder => out.string("kind", "folder"),
            BookmarkKind::Link { url, .. } => { out.string("kind", "link"); out.string("url", url); }
        }
        if let Some(path) = path { out.array("folderPath", |parts| for part in path { parts.string(part); }); }
    });
}

pub(crate) fn map_command(store: &BookmarkStore, fields: PanelFields) -> Option<Action> {
    let no_extra = |id: bool, parent: bool, before: bool, text: bool, url: bool| {
        fields.id.is_some() == id && fields.parent.is_some() == parent
            && fields.before.is_some() == before && fields.text.is_some() == text
            && fields.url.is_some() == url && fields.n.is_none() && fields.on.is_none()
    };
    match fields.a.as_str() {
        "search" if no_extra(false, false, false, true, false) => Some(Action::Search(fields.text?)),
        "open" | "newtab" if no_extra(true, false, false, false, false) => {
            let id = fields.id?;
            let item = store.items().iter().find(|item| item.id == id)?;
            let BookmarkKind::Link { url, .. } = &item.kind else { return None; };
            Some(Action::Open { url: url.clone(), new_tab: fields.a == "newtab" })
        }
        "delete" if no_extra(true, false, false, false, false) => {
            let id = fields.id?;
            store.items().iter().any(|item| item.id == id).then_some(Action::Delete(id))
        }
        "rename" if no_extra(true, false, false, true, false) => {
            let id = fields.id?;
            store.items().iter().any(|item| item.id == id).then_some(Action::Rename { id, title: fields.text? })
        }
        "update" if no_extra(true, false, false, true, true) => {
            let id = fields.id?;
            store.items().iter().any(|item| item.id == id && matches!(&item.kind, BookmarkKind::Link { .. }))
                .then_some(Action::Update { id, title: fields.text?, url: fields.url? })
        }
        "addfolder" | "addlink" if no_extra(false, true, false, true, fields.a == "addlink") => {
            let parent = decode_parent(fields.parent?);
            if parent.is_some_and(|id| !store.items().iter().any(|item| item.id == id && item.kind == BookmarkKind::Folder)) { return None; }
            if fields.a == "addfolder" { Some(Action::AddFolder { parent, title: fields.text? }) }
            else { Some(Action::AddLink { parent, title: fields.text?, url: fields.url? }) }
        }
        "move" if no_extra(true, true, true, false, false) => {
            let id = fields.id?;
            let parent = decode_parent(fields.parent?);
            let item = store.items().iter().find(|item| item.id == id)?;
            if parent.is_some_and(|new_parent| !store.items().iter().any(|item| item.id == new_parent && item.kind == BookmarkKind::Folder)) { return None; }
            if matches!(&item.kind, BookmarkKind::Folder) && parent.is_some_and(|parent| is_descendant(store.items(), parent, id)) { return None; }
            let siblings: Vec<u64> = store.items().iter().filter(|item| item.parent == parent && item.id != id).map(|item| item.id).collect();
            let before = fields.before?;
            let index = if before == 0 { siblings.len() } else { siblings.iter().position(|sibling| *sibling == before)? };
            Some(Action::Move { id, parent, index })
        }
        _ => None,
    }
}

fn decode_parent(parent: u64) -> Option<u64> { (parent != 0).then_some(parent) }

fn is_descendant(items: &[Bookmark], candidate: u64, ancestor: u64) -> bool {
    let mut cursor = Some(candidate);
    while let Some(id) = cursor {
        if id == ancestor { return true; }
        cursor = items.iter().find(|item| item.id == id).and_then(|item| item.parent);
    }
    false
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
    fn snapshot_keeps_root_null_and_link_urls() {
        let snapshot = snapshot(&fixture(), "");
        assert!(snapshot.contains(r#""parent":null"#));
        assert!(snapshot.contains(r#""url":"https://example.com/docs""#));
    }

    #[test]
    fn command_mapper_requires_live_targets_and_sibling_destinations() {
        let store = fixture();
        assert_eq!(map_command(&store, command("newtab", Some(2), None, None, None)), Some(Action::Open { url: "https://example.com/docs".into(), new_tab: true }));
        assert_eq!(map_command(&store, command("delete", Some(90), None, None, None)), None);
        assert_eq!(map_command(&store, command("move", Some(2), Some(0), Some(90), None)), None);
    }

    fn command(a: &str, id: Option<u64>, parent: Option<u64>, before: Option<u64>, text: Option<&str>) -> PanelFields {
        PanelFields { a: a.into(), id, parent, before, text: text.map(str::to_owned), ..PanelFields::default() }
    }
}
