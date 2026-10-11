//! Search model for the themed command prompt. The index is assembled only when opened.

use crate::productivity::{self, CommandRegistry, SearchItem};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResultItem {
    pub label: String,
    pub detail: String,
    pub action: String,
}

pub(crate) fn search(
    query: &str,
    tabs: &[(String, String)],
    bookmarks: &[(String, String, String)],
    history: &[(u64, String, String)],
    commands: &CommandRegistry,
) -> Vec<ResultItem> {
    let mut items = Vec::new();
    for (index, (title, url)) in tabs.iter().enumerate() {
        items.push(SearchItem { label: title.clone(), detail: url.clone(), action: format!("tab:{index}") });
    }
    for (title, url, folder) in bookmarks {
        items.push(SearchItem { label: title.clone(), detail: format!("{url} {folder}"), action: format!("url:{url}") });
    }
    for (_, url, title) in history.iter().rev() {
        items.push(SearchItem { label: title.clone(), detail: url.clone(), action: format!("url:{url}") });
    }
    items.extend(commands.commands().iter().cloned());
    productivity::rank_items(&items, query, 40)
        .into_iter()
        .map(|(item, _)| ResultItem { label: item.label.clone(), detail: item.detail.clone(), action: item.action.clone() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_search_indexes_tabs_bookmarks_history_and_registered_commands_lazily() {
        let mut registry = CommandRegistry::default();
        registry.register("New tab", "Open a blank tab", "new-tab");
        let found = search(
            "daily",
            &[("Daily notes".into(), "https://notes.example/".into())],
            &[("Daily plan".into(), "https://plan.example/".into(), "Work".into())],
            &[(1, "https://archive.example/".into(), "Daily archive".into())],
            &registry,
        );
        assert_eq!(found.len(), 3);
        assert!(found.iter().any(|item| item.action == "tab:0"));
        assert!(found.iter().any(|item| item.action == "url:https://plan.example/"));
        assert!(found.iter().any(|item| item.action == "url:https://archive.example/"));
        assert!(search("new tab", &[], &[], &[], &registry).iter().any(|item| item.action == "new-tab"));
    }
}
