//! Omnibox suggestion rows for the HTML toolbar: pure row building plus a cached bookmark list.
//!
//! The dropdown itself is drawn by the hover layer (`html_hover.rs`, `Kind::Suggest`); this module
//! only decides what the rows are and what picking each one does. Ranking lives in
//! `crate::omnibox::suggest`. Everything runs on the UI thread, once per edit.

use std::{cell::RefCell, collections::HashSet, path::PathBuf, rc::Rc, time::SystemTime};

use url::Url;

use crate::{
    bookmarks::{BookmarkKind, BookmarkStore},
    chrome_host::protocol::SuggestItem,
    omnibox::{self, Completion, History, SuggestKind, SuggestSource, Suggestion},
    tabs::TabId,
};

/// What picking a row does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    /// Go to this address (the same path as typing it and pressing Enter).
    Navigate(String),
    /// Search for this text (the same path as typing it and pressing Enter).
    Search(String),
    /// Switch to an open tab.
    Tab(TabId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub item: SuggestItem,
    pub target: Target,
    /// Address used to find a favicon among the open tabs; empty for a search row.
    pub fav_url: String,
}

/// An open tab offered to the dropdown: id, full address and title.
pub(crate) type TabSource = (TabId, String, String);

/// Rows for `typed`: the default row first, then up to seven matches. `tabs` must not hold the
/// active tab. Pure.
pub(crate) fn rows(
    history: &History,
    completion: Option<&Completion>,
    typed: &str,
    typed_url: Option<&Url>,
    tabs: &[TabSource],
    bookmarks: &[(String, String)],
) -> Vec<Row> {
    let tab_sources: Vec<SuggestSource<'_>> =
        tabs.iter().map(|(_, url, title)| SuggestSource { url, title }).collect();
    let mark_sources: Vec<SuggestSource<'_>> =
        bookmarks.iter().map(|(url, title)| SuggestSource { url, title }).collect();
    let first = omnibox::default_suggestion(typed, completion, typed_url);
    omnibox::suggest(history, &mark_sources, &tab_sources, typed, first)
        .into_iter()
        .filter_map(|row| to_row(row, tabs))
        .collect()
}

fn to_row(row: Suggestion, tabs: &[TabSource]) -> Option<Row> {
    let target = match (row.kind, row.tab) {
        (SuggestKind::Tab, Some(index)) => Target::Tab(tabs.get(index)?.0),
        (SuggestKind::Search, _) => Target::Search(row.target.clone()),
        _ => Target::Navigate(row.target.clone()),
    };
    let fav_url = if row.kind == SuggestKind::Search { String::new() } else { row.target.clone() };
    Some(Row {
        item: SuggestItem {
            kind: row.kind.wire().to_owned(),
            text: row.text,
            title: row.title,
            url: row.url,
            png: None,
        },
        target,
        fav_url,
    })
}

/// The open tab whose address has the same host as `url`, for borrowing its favicon.
pub(crate) fn tab_with_host(tabs: &[TabSource], url: &str) -> Option<TabId> {
    let host = Url::parse(url).ok()?.host_str()?.to_ascii_lowercase();
    tabs.iter()
        .find(|(_, tab_url, _)| {
            Url::parse(tab_url).ok().and_then(|u| u.host_str().map(str::to_ascii_lowercase)).as_deref() == Some(host.as_str())
        })
        .map(|(id, _, _)| *id)
}

// ----- bookmarks -----

struct BookmarkCache {
    path: PathBuf,
    stamp: Option<(SystemTime, u64)>,
    links: Rc<Vec<(String, String)>>,
    /// Every bookmarked address, for `is_bookmarked`.
    urls: Rc<HashSet<String>>,
}

thread_local! {
    static BOOKMARKS: RefCell<Option<BookmarkCache>> = const { RefCell::new(None) };
}

fn stamp_of(path: &std::path::Path) -> Option<(SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Every bookmarked page in `store` as (address, title). Pure.
fn links_of(store: &BookmarkStore) -> Vec<(String, String)> {
    store
        .items()
        .iter()
        .filter_map(|item| match &item.kind {
            BookmarkKind::Link { url, .. } => Some((url.clone(), item.title.clone())),
            BookmarkKind::Folder => None,
        })
        .collect()
}

/// The addresses of `links`, one lookup each. Pure.
fn url_set(links: &[(String, String)]) -> HashSet<String> {
    links.iter().map(|(url, _)| url.clone()).collect()
}

/// Every bookmarked page as (address, title). The list is kept until the bookmark file's time or
/// size changes, so a keystroke costs one `stat`, not a parse. Empty in a guest profile.
pub(crate) fn bookmarks() -> Rc<Vec<(String, String)>> {
    refreshed().map(|(links, _)| links).unwrap_or_default()
}

/// Re-reads the bookmark file if it changed since the cache was filled, and reports whether the set
/// of bookmarked addresses changed. Called where bookmarks may have been edited (`WM_RAIL_CHANGED`,
/// which every bookmark editor in the window posts), not per paint. Nothing is read while nothing
/// is cached: the first lookup reads the file then.
pub(crate) fn refresh_bookmarks() -> bool {
    let Some(before) = BOOKMARKS.with(|cell| cell.borrow().as_ref().map(|cache| cache.urls.clone())) else {
        return false;
    };
    match refreshed() {
        Some((_, after)) => !Rc::ptr_eq(&before, &after) && before != after,
        None => !before.is_empty(),
    }
}

/// The cached links and address set, re-read only when the file's time or size changed. `None` in
/// a guest profile or without a profile.
fn refreshed() -> Option<(Rc<Vec<(String, String)>>, Rc<HashSet<String>>)> {
    let profile = crate::profile::active().ok()?;
    if !crate::productivity::profile_allows_productivity(&profile) {
        return None;
    }
    let path = profile.root().join("bookmarks-v2.txt");
    let stamp = stamp_of(&path);
    Some(BOOKMARKS.with(|cell| {
        let mut cache = cell.borrow_mut();
        if let Some(hit) = cache.as_ref().filter(|c| c.path == path && c.stamp == stamp && stamp.is_some()) {
            return (hit.links.clone(), hit.urls.clone());
        }
        let links = BookmarkStore::load(profile.root()).map(|store| links_of(&store)).unwrap_or_default();
        let urls = Rc::new(url_set(&links));
        let links = Rc::new(links);
        *cache = Some(BookmarkCache { stamp: stamp_of(&path), path, links: links.clone(), urls: urls.clone() });
        (links, urls)
    }))
}

/// Whether `url` is bookmarked: one lookup in the in-memory set. The file is read only the first
/// time; after that the set follows `refresh_bookmarks` and `bookmarks`, so a paint or a sync never
/// touches the disk.
pub(crate) fn is_bookmarked(url: &str) -> bool {
    let cached = BOOKMARKS.with(|cell| cell.borrow().as_ref().map(|cache| cache.urls.clone()));
    match cached {
        Some(urls) => urls.contains(url),
        None => refreshed().is_some_and(|(_, urls)| urls.contains(url)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history(entries: &[(&str, &str)]) -> History {
        let mut history = History::default();
        for (url, title) in entries {
            history.record(url, title);
        }
        history
    }

    #[test]
    fn rows_start_with_the_default_row_and_map_tabs_to_switch_targets() {
        let history = history(&[("https://docs.example.com/guide", "Guide")]);
        let tabs = vec![(TabId(7), "https://docs.example.com/guide".to_owned(), "Guide".to_owned())];
        let marks = vec![("https://example.org/".to_owned(), "Example".to_owned())];
        let rows = rows(&history, None, "exa", None, &tabs, &marks);
        assert_eq!(rows[0].item.kind, "search");
        assert_eq!(rows[0].target, Target::Search("exa".into()));
        assert!(rows[0].fav_url.is_empty());
        assert!(rows.iter().any(|r| r.target == Target::Tab(TabId(7)) && r.item.kind == "tab"));
        assert!(rows.iter().any(|r| r.item.kind == "bm" && r.target == Target::Navigate("https://example.org/".into())));
        // The tab page is not repeated as a history row.
        assert_eq!(rows.iter().filter(|r| r.item.url.contains("docs.example.com")).count(), 1);
    }

    #[test]
    fn a_typed_address_is_the_default_row_and_hosts_find_their_tab() {
        let url = Url::parse("https://example.com/").unwrap();
        let rows = rows(&History::default(), None, "example.com", Some(&url), &[], &[]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].item.kind, "url");
        assert_eq!(rows[0].target, Target::Navigate("https://example.com/".into()));
        let tabs = vec![(TabId(3), "https://www.example.com/a".to_owned(), String::new())];
        assert_eq!(tab_with_host(&tabs, "https://www.example.com/b"), Some(TabId(3)));
        assert_eq!(tab_with_host(&tabs, "https://other.test/"), None);
    }

    #[test]
    fn bookmark_lookups_use_one_set_of_link_addresses() {
        let mut store = BookmarkStore::new();
        let folder = store.add_folder(None, "Reading").unwrap();
        store.add_bookmark(Some(folder), "Rust", "https://www.rust-lang.org/").unwrap();
        store.add_bookmark(None, "Example", "https://example.com/a").unwrap();
        let links = links_of(&store);
        assert_eq!(links.len(), 2, "folders are not links");
        let urls = url_set(&links);
        assert!(urls.contains("https://www.rust-lang.org/"));
        assert!(urls.contains("https://example.com/a"));
        assert!(!urls.contains("https://example.com/"), "a lookup is exact, as the old scan was");
    }
}
