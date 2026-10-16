//! The profile's search engines and typed history, read once and kept for the address bar. The
//! engine list is re-read only when one of the files it depends on changes (one `stat` each), so a
//! keystroke never parses anything. Everything runs on the UI thread.

use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
    time::SystemTime,
};

use crate::{
    address_suggest::TypedHistory,
    search_engines::{EngineStore, FILE_NAME},
};

type Stamp = Option<(SystemTime, u64)>;

fn stamp_of(path: &Path) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

struct EngineCache {
    root: PathBuf,
    stamps: [Stamp; 3],
    store: Rc<EngineStore>,
}

thread_local! {
    /// Tabs that were sent to an https address by "always use HTTPS", with the http address they
    /// were sent from: when the secure page cannot load, the plain one is tried once.
    static UPGRADED: RefCell<std::collections::HashMap<crate::tabs::TabId, url::Url>> = RefCell::new(std::collections::HashMap::new());
    static ENGINES: RefCell<Option<EngineCache>> = const { RefCell::new(None) };
    static TYPED: RefCell<Option<(PathBuf, TypedHistory)>> = const { RefCell::new(None) };
}

/// The active profile's engines. With no profile, the built-in list.
pub fn engines() -> Rc<EngineStore> {
    let Ok(profile) = crate::profile::active() else {
        return Rc::new(EngineStore::default());
    };
    let root = profile.root().to_path_buf();
    let settings_file = profile.settings_file();
    let provider_file = profile.search_provider_file();
    let stamps = [stamp_of(&root.join(FILE_NAME)), stamp_of(&settings_file), stamp_of(&provider_file)];
    if let Some(hit) = ENGINES.with(|cell| cell.borrow().as_ref().filter(|cache| cache.root == root && cache.stamps == stamps).map(|cache| cache.store.clone())) {
        return hit;
    }
    // The older `search_engine` setting and `search-provider.txt` only matter while there is no list file.
    let legacy = crate::settings::Settings::load(&settings_file).search_engine;
    let template = std::fs::read_to_string(&provider_file).ok().map(|text| text.trim().to_owned());
    let store = Rc::new(EngineStore::load(&root, legacy, template.as_deref()));
    ENGINES.with(|cell| *cell.borrow_mut() = Some(EngineCache { root, stamps, store: store.clone() }));
    store
}

/// Writes the engine list and forgets the cache, so the next read sees it.
pub fn save_engines(store: &EngineStore) -> std::io::Result<()> {
    let profile = crate::profile::active().map_err(|error| std::io::Error::other(error.to_string()))?;
    store.save(profile.root())?;
    ENGINES.with(|cell| *cell.borrow_mut() = None);
    Ok(())
}

/// Remembers that `tab` was sent from `original` (http) to its https address.
pub fn note_upgrade(tab: crate::tabs::TabId, original: url::Url) {
    UPGRADED.with(|map| {
        let mut map = map.borrow_mut();
        if map.len() > 64 {
            map.clear();
        }
        map.insert(tab, original);
    });
}

/// The http address `tab` was upgraded from, once: asking again gives nothing.
pub fn take_upgrade(tab: crate::tabs::TabId) -> Option<url::Url> {
    UPGRADED.with(|map| map.borrow_mut().remove(&tab))
}

/// A tab navigated somewhere else: its upgrade, if any, no longer applies.
pub fn forget_upgrade(tab: crate::tabs::TabId) {
    UPGRADED.with(|map| map.borrow_mut().remove(&tab));
}

/// Whether typed text may be remembered for the active profile: not a guest, history on.
fn remembers() -> Option<PathBuf> {
    let profile = crate::profile::active().ok()?;
    (!profile.is_guest() && profile.history_enabled()).then(|| profile.root().to_path_buf())
}

fn with_typed<T>(f: impl FnOnce(&mut TypedHistory) -> T) -> Option<T> {
    let root = remembers()?;
    TYPED.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.as_ref().is_none_or(|(path, _)| *path != root) {
            *slot = Some((root.clone(), TypedHistory::load(&root)));
        }
        slot.as_mut().map(|(_, history)| f(history))
    })
}

/// Remembers text submitted from the address field (a search, or an address as typed). Nothing is
/// kept for a private tab, a guest or with history off: `private` is the caller's word for the tab.
pub fn record_typed(text: &str, search: bool, private: bool) {
    if private {
        return;
    }
    let saved = with_typed(|history| {
        history.record(text, search);
        remembers().map(|root| history.save(&root))
    });
    let _ = saved;
}

/// A copy of what was typed before, oldest first. Empty for a guest or with history off.
pub fn typed() -> TypedHistory {
    with_typed(|history| history.clone()).unwrap_or_default()
}

/// Forgets everything typed (the history is being cleared).
pub fn clear_typed() {
    let _ = with_typed(|history| {
        history.clear();
        if let Some(root) = remembers() {
            let _ = history.save(&root);
        }
    });
}
