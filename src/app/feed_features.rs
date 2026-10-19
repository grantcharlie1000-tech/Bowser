//! What the toolbar and feed options do to the running app (thread T13): saving a bar's list, finding
//! the feeds a page offers, and opening a feed item in a tab. The rules are pure and tested in
//! `crate::toolbar_prefs` and `crate::feeds`; this file applies them to `App`.

use super::*;
use crate::feeds::discover::{self, FeedLink};

/// The page's feed links: only `<link rel~=alternate>` tags whose type names a feed, at most twelve, as
/// `[rel, type, href, title]`. It reads and changes nothing else on the page.
const FEED_SCRIPT: &str = "(()=>{const o=[];for(const l of document.querySelectorAll('link[rel~=\"alternate\" i][href]')){const t=(l.getAttribute('type')||'').toLowerCase();if(t.indexOf('rss')<0&&t.indexOf('atom')<0&&t.indexOf('rdf')<0&&t.indexOf('feed')<0)continue;o.push([l.getAttribute('rel')||'',t,l.href||'',(l.getAttribute('title')||'').slice(0,120)]);if(o.length>=12)break}return o})()";

/// Longest script answer read back, in bytes.
pub(crate) const MAX_FEED_ANSWER_BYTES: usize = 16 * 1024;

/// The feeds a page offered, with the address of the page they were read from (so an answer is never
/// shown for another page).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PageFeeds {
    pub url: String,
    pub links: Vec<FeedLink>,
}

/// Reads the script's answer: an array of `[rel, type, href, title]` strings, judged by the same gate as
/// a scanned page.
pub(crate) fn links_from_answer(base: &url::Url, answer: &str) -> Vec<FeedLink> {
    let Some(value) = crate::translate::json::parse(answer) else { return Vec::new() };
    let Some(rows) = value.as_array() else { return Vec::new() };
    let mut out = Vec::new();
    for row in rows.iter().take(16) {
        let Some([rel, mime, href, title]) = row.as_array().map(|cells| cells.iter().map(|cell| cell.as_str().unwrap_or("")).collect::<Vec<_>>()).and_then(|cells| <[&str; 4]>::try_from(cells).ok()) else {
            continue;
        };
        if let Some(link) = discover::accept_link(base, rel, mime, href, title) {
            discover::push_unique(&mut out, link);
        }
    }
    out
}

impl App {
    /// Settings > Toolbars and Feeds: one `key=value` change of the tab bar, panel bar or feed options,
    /// kept, saved and put to work at once. False (and nothing changed) when the key or value is not one the
    /// page may send; `Err` too when it is in force but `settings.txt` could not be written.
    pub fn set_bar_option(app: &AppRef, text: &str) -> std::result::Result<(), String> {
        if !Self::apply_bar_option(app, text) {
            return Err("That value is not allowed. Nothing was saved.".into());
        }
        Self::try_save_settings(app)
    }

    /// The in-memory half of [`App::set_bar_option`]: checks and applies the change and redraws the bars.
    pub(crate) fn apply_bar_option(app: &AppRef, text: &str) -> bool {
        {
            let mut state = app.borrow_mut();
            if !crate::toolbar_prefs::apply_assignment(&mut state.settings, text) {
                return false;
            }
        }
        Self::bars_changed(app);
        true
    }

    /// The bars' lists changed (a save, a reset): the toolbar, the strip and the rail draw again.
    pub(crate) fn bars_changed(app: &AppRef) {
        let (hwnd, active) = {
            let state = app.borrow();
            (state.hwnd, state.tab_model.active())
        };
        {
            // Pages that were not looked at while detection was off are looked at again on their next load; what
            // is known stays until then, but is not shown while the setting is off.
            let mut state = app.borrow_mut();
            if !state.settings.feeds_detect {
                state.feed_pages.clear();
            }
        }
        Self::notify_rail_changed(app);
        Self::post_tab_changed(hwnd, active);
    }

    /// After a page finished loading: reads the feeds it offers (when the setting is on). One small script
    /// per page, run after the page is usable; nothing is fetched from the network.
    pub(crate) fn feed_probe(app: &AppRef, tab: TabId) {
        let url = {
            let state = app.borrow();
            if !state.settings.feeds_detect {
                return;
            }
            state.tabs.iter().find(|candidate| candidate.id == tab).filter(|tab| tab.controller_created && tab.page_usable && !tab.discarded).and_then(|tab| tab.url.clone())
        };
        let Some(url) = url.filter(|url| matches!(url.scheme(), "http" | "https")) else { return };
        let weak = Rc::downgrade(app);
        let page = url.clone();
        Self::feed_script(
            app,
            tab,
            FEED_SCRIPT.to_owned(),
            Box::new(move |answer| {
                let Some(app) = weak.upgrade() else { return };
                let links = answer.map(|text| links_from_answer(&page, &text)).unwrap_or_default();
                let hwnd = {
                    let mut state = app.borrow_mut();
                    let live: Vec<TabId> = state.tabs.iter().map(|tab| tab.id).collect();
                    state.feed_pages.retain(|id, _| live.contains(id));
                    // Only a page that is still the tab's own is recorded.
                    if state.tabs.iter().find(|candidate| candidate.id == tab).and_then(|tab| tab.url.as_ref()) != Some(&page) {
                        return;
                    }
                    let old = state.feed_pages.get(&tab).cloned();
                    let new = (!links.is_empty()).then(|| PageFeeds { url: page.to_string(), links });
                    if old == new {
                        return;
                    }
                    match new {
                        Some(new) => {
                            state.feed_pages.insert(tab, new);
                        }
                        None => {
                            state.feed_pages.remove(&tab);
                        }
                    }
                    state.hwnd
                };
                Self::post_tab_changed_kind(hwnd, tab, false, false);
            }),
        );
    }

    /// The feeds the page in front offers: empty unless detection is on and the page is the one that was read.
    pub fn active_feed_links(app: &AppRef) -> Vec<FeedLink> {
        let state = app.borrow();
        if !state.settings.feeds_detect {
            return Vec::new();
        }
        let active = state.tab_model.active();
        let current = state.tabs.iter().find(|tab| tab.id == active).and_then(|tab| tab.url.as_ref());
        match (state.feed_pages.get(&active), current) {
            (Some(known), Some(url)) if known.url == url.as_str() => known.links.clone(),
            _ => Vec::new(),
        }
    }

    /// Opens `url` (a feed item or an episode) in a new tab that comes to the front.
    pub fn open_in_new_tab(app: &AppRef, url: &url::Url) {
        Self::new_tab(app);
        Self::submit_url(app, url.clone(), None);
    }

    /// Runs the feed script in `tab`. `done` always runs with the app unborrowed, even when the engine answers at once.
    fn feed_script(app: &AppRef, tab: TabId, script: String, done: Box<dyn FnOnce(Option<String>)>) {
        let done = Rc::new(std::cell::RefCell::new(Some(done)));
        let early = Rc::new(std::cell::RefCell::new(None::<Option<String>>));
        let in_call = Rc::new(std::cell::Cell::new(true));
        let answer = {
            let (done, early, in_call) = (done.clone(), early.clone(), in_call.clone());
            Box::new(move |text: Option<String>| {
                if in_call.get() {
                    *early.borrow_mut() = Some(text);
                } else if let Some(done) = done.borrow_mut().take() {
                    done(text);
                }
            }) as Box<dyn FnOnce(Option<String>)>
        };
        let called = Self::call_engine(app, |engine| engine.run_feed_script(tab, &script, answer));
        in_call.set(false);
        let text = match (called, early.borrow_mut().take()) {
            (None, _) => Some(None),
            (Some(()), early) => early,
        };
        if let Some(text) = text
            && let Some(done) = done.borrow_mut().take()
        {
            done(text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> url::Url {
        url::Url::parse("https://blog.example/post").unwrap()
    }

    #[test]
    fn the_scripts_answer_is_judged_by_the_same_gate_as_a_scanned_page() {
        let answer = r#"[["alternate","application/rss+xml","https://blog.example/feed.xml","Posts"],
            ["alternate","application/atom+xml","/atom.xml",""],
            ["stylesheet","application/rss+xml","/x.xml",""],
            ["alternate","application/rss+xml","javascript:alert(1)",""],
            ["alternate","application/rss+xml","https://blog.example/feed.xml","dup"],
            ["alternate"],
            [1,2,3,4]]"#;
        let links = links_from_answer(&base(), answer);
        assert_eq!(links.iter().map(|l| (l.url.as_str(), l.title.as_str())).collect::<Vec<_>>(), [("https://blog.example/feed.xml", "Posts"), ("https://blog.example/atom.xml", "")]);
    }

    #[test]
    fn a_hostile_or_malformed_answer_yields_nothing() {
        for bad in ["", "null", "{}", "\"x\"", "[[[[[[[[[1]]]]]]]]]", "[[\"alternate\",\"application/rss+xml\"", "not json", "[1e5]"] {
            assert!(links_from_answer(&base(), bad).is_empty(), "{bad}");
        }
        let many: String = format!("[{}]", (0..40).map(|n| format!("[\"alternate\",\"application/rss+xml\",\"/f{n}.xml\",\"\"]")).collect::<Vec<_>>().join(","));
        assert!(links_from_answer(&base(), &many).len() <= discover::MAX_LINKS);
    }

    #[test]
    fn the_script_only_reads_link_tags() {
        assert!(FEED_SCRIPT.contains("querySelectorAll('link[rel~=\"alternate\" i][href]')"));
        for forbidden in ["fetch(", "XMLHttpRequest", "innerHTML", "eval(", "document.write", "location", "cookie", "localStorage", "postMessage"] {
            assert!(!FEED_SCRIPT.contains(forbidden), "{forbidden}");
        }
    }
}
