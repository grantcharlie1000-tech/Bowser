//! Decision logic for putting the chrome views of one WebView2 environment in one BrowsingInstance,
//! so Chromium serves them all from one renderer process instead of one per view.
//!
//! Every chrome view used to be an independent top-level WebView, and Chromium gives each its own
//! BrowsingInstance and therefore its own renderer, even though they are all same-site. Instead,
//! the first view of an environment (the anchor) navigates normally. Each further view is created
//! as usual (its own controller, handlers and parent window) but not navigated; a loaded chrome
//! view then calls `window.open(<that view's exact start url>)` and `NewWindowRequested` answers it
//! with the pre-created WebView (`put_NewWindow`). The opener relationship is kept (no `noopener`),
//! which keeps the new page in the opener's BrowsingInstance and therefore in its process.
//!
//! Only one open is ever in flight, so a request is matched to exactly one waiting view: the
//! `NewWindowRequested` must come from the chosen opener and carry exactly the waiting view's start
//! url. Anything else keeps the old policy (popups are swallowed). Every failure (no loaded opener,
//! a timeout, a dead opener) ends in a plain `Navigate`, i.e. the previous behaviour. This module is
//! pure: it holds no COM objects (`T` is whatever the caller needs to finish the view) and takes the
//! clock as an argument.

use std::collections::VecDeque;

pub(crate) type HostId = u64;

/// How long an opener has to deliver `NewWindowRequested` after `window.open` was called.
pub(crate) const OPEN_TIMEOUT_MS: u64 = 4_000;
/// How long a view waits for an anchor that never finishes loading before it loads on its own.
pub(crate) const ANCHOR_WAIT_MS: u64 = 6_000;
/// Consecutive failed opens after which the mechanism is switched off for the process.
const MAX_CONSECUTIVE_TIMEOUTS: u8 = 3;

/// What the caller must do next.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step<T> {
    /// Load the view's own start url in its WebView (the previous behaviour).
    Navigate(T),
    /// Run `window.open(url)` in the loaded WebView of `opener`.
    Open { opener: HostId, url: &'static str },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Navigating on its own; the first view of an environment.
    Direct,
    /// Waiting for a loaded opener.
    Queued,
    /// Loaded into the opener's BrowsingInstance.
    Opened,
}

struct Member {
    id: HostId,
    env: usize,
    kind: Kind,
    loaded: bool,
    /// A host whose renderer is known to have failed, or that did not answer `window.open`.
    distrusted: bool,
    /// Preferred as an opener (the split host embeds web frames and is used last).
    preferred: bool,
}

struct Waiting<T> {
    id: HostId,
    env: usize,
    url: &'static str,
    since: u64,
    item: T,
}

struct InFlight<T> {
    waiting: Waiting<T>,
    opener: HostId,
    deadline: u64,
}

pub(crate) struct Coordinator<T> {
    members: Vec<Member>,
    waiting: VecDeque<Waiting<T>>,
    in_flight: Option<InFlight<T>>,
    timeouts: u8,
    disabled: bool,
}

impl<T> Default for Coordinator<T> {
    fn default() -> Self {
        Self { members: Vec::new(), waiting: VecDeque::new(), in_flight: None, timeouts: 0, disabled: false }
    }
}

impl<T> Coordinator<T> {
    /// A view finished configuring and wants its page. `url` is its exact start url.
    pub(crate) fn register(&mut self, id: HostId, env: usize, url: &'static str, preferred: bool, item: T, now: u64) -> Vec<Step<T>> {
        let anchored = self.members.iter().any(|m| m.env == env && !m.distrusted && (m.kind == Kind::Direct || m.loaded));
        let direct = self.disabled || !anchored;
        self.members.push(Member { id, env, kind: if direct { Kind::Direct } else { Kind::Queued }, loaded: false, distrusted: false, preferred });
        if direct {
            return vec![Step::Navigate(item)];
        }
        self.waiting.push_back(Waiting { id, env, url, since: now, item });
        self.pump(now)
    }

    /// The view's page finished loading: it may now open the views waiting behind it.
    pub(crate) fn loaded(&mut self, id: HostId, now: u64) -> Vec<Step<T>> {
        if let Some(member) = self.members.iter_mut().find(|m| m.id == id) {
            member.loaded = true;
        }
        self.pump(now)
    }

    /// The view's renderer failed: it can no longer open anything.
    pub(crate) fn failed(&mut self, id: HostId, now: u64) -> Vec<Step<T>> {
        if let Some(member) = self.members.iter_mut().find(|m| m.id == id) {
            member.distrusted = true;
            member.loaded = false;
        }
        self.reroute_if_opener(id);
        self.pump(now)
    }

    /// The view was closed or dropped. Nothing may depend on it any longer.
    pub(crate) fn gone(&mut self, id: HostId, now: u64) -> Vec<Step<T>> {
        self.members.retain(|m| m.id != id);
        self.waiting.retain(|w| w.id != id);
        if self.in_flight.as_ref().is_some_and(|f| f.waiting.id == id) {
            self.in_flight = None;
        }
        self.reroute_if_opener(id);
        self.pump(now)
    }

    /// `NewWindowRequested` from host `opener` for `uri`. Returns the waiting view's item only for
    /// the in-flight request, from its chosen opener, for exactly its start url.
    pub(crate) fn accept(&mut self, opener: HostId, uri: &str) -> Option<T> {
        let matches = self.in_flight.as_ref().is_some_and(|f| f.opener == opener && f.waiting.url == uri);
        if !matches {
            return None;
        }
        let flight = self.in_flight.take()?;
        if let Some(member) = self.members.iter_mut().find(|m| m.id == flight.waiting.id) {
            member.kind = Kind::Opened;
        }
        self.timeouts = 0;
        Some(flight.waiting.item)
    }

    /// `window.open` could not even be started in the opener: the same as a timeout.
    pub(crate) fn open_failed(&mut self, now: u64) -> Vec<Step<T>> {
        let Some(flight) = self.in_flight.take() else { return Vec::new(); };
        if let Some(member) = self.members.iter_mut().find(|m| m.id == flight.opener) {
            member.distrusted = true;
        }
        self.timeouts = self.timeouts.saturating_add(1);
        self.waiting.push_front(flight.waiting);
        if self.timeouts >= MAX_CONSECUTIVE_TIMEOUTS {
            self.disabled = true;
        }
        self.pump(now)
    }

    /// Time moved on: expire an unanswered open, and let views that wait for an anchor which never
    /// loads load on their own.
    pub(crate) fn tick(&mut self, now: u64) -> Vec<Step<T>> {
        if self.in_flight.as_ref().is_some_and(|f| now >= f.deadline) {
            return self.open_failed(now);
        }
        self.pump(now)
    }

    /// When `tick` next has something to do, if ever.
    pub(crate) fn next_deadline(&self) -> Option<u64> {
        let flight = self.in_flight.as_ref().map(|f| f.deadline);
        let waiting = self.waiting.iter().map(|w| w.since + ANCHOR_WAIT_MS).min();
        match (flight, waiting) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    #[cfg(test)]
    pub(crate) fn is_disabled(&self) -> bool { self.disabled }

    /// An in-flight open whose opener vanished goes back to the front of the queue.
    fn reroute_if_opener(&mut self, id: HostId) {
        if self.in_flight.as_ref().is_some_and(|f| f.opener == id) {
            if let Some(flight) = self.in_flight.take() {
                self.waiting.push_front(flight.waiting);
            }
        }
    }

    fn opener_for(&self, env: usize, requester: HostId) -> Option<HostId> {
        let usable = |m: &&Member| m.env == env && m.loaded && !m.distrusted && m.id != requester;
        self.members.iter().filter(usable).find(|m| m.preferred).or_else(|| self.members.iter().find(|m| usable(m))).map(|m| m.id)
    }

    fn pump(&mut self, now: u64) -> Vec<Step<T>> {
        let mut steps = Vec::new();
        let mut index = 0;
        while index < self.waiting.len() {
            if self.disabled {
                if let Some(w) = self.waiting.remove(index) {
                    self.set_direct(w.id);
                    steps.push(Step::Navigate(w.item));
                }
                continue;
            }
            let (id, env, since) = { let w = &self.waiting[index]; (w.id, w.env, w.since) };
            if self.in_flight.is_none() {
                if let Some(opener) = self.opener_for(env, id) {
                    let Some(waiting) = self.waiting.remove(index) else { break; };
                    steps.push(Step::Open { opener, url: waiting.url });
                    self.in_flight = Some(InFlight { waiting, opener, deadline: now + OPEN_TIMEOUT_MS });
                    continue;
                }
            }
            // Nobody in this environment is going to load, or the anchor has taken too long: load alone.
            let anchor_coming = self.members.iter().any(|m| m.env == env && !m.distrusted && !m.loaded && matches!(m.kind, Kind::Direct | Kind::Opened));
            let no_opener_at_all = self.opener_for(env, id).is_none();
            if no_opener_at_all && (!anchor_coming || now.saturating_sub(since) >= ANCHOR_WAIT_MS) {
                if let Some(w) = self.waiting.remove(index) {
                    self.set_direct(w.id);
                    steps.push(Step::Navigate(w.item));
                }
                continue;
            }
            index += 1;
        }
        steps
    }

    fn set_direct(&mut self, id: HostId) {
        if let Some(member) = self.members.iter_mut().find(|m| m.id == id) {
            member.kind = Kind::Direct;
        }
    }
}

/// The script that makes a loaded chrome view open `url`. `url` is always one of the fixed chrome
/// start urls; anything that could break out of the string literal is refused.
pub(crate) fn open_script(url: &str) -> Option<String> {
    let safe = !url.is_empty() && url.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~:/?=&".contains(&b));
    safe.then(|| format!("window.open(\"{url}\");"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "bowser-ui://app/index.html?view=a";
    const B: &str = "bowser-ui://app/index.html?view=b";
    const C: &str = "bowser-ui://app/index.html?view=c";
    const ENV: usize = 1;

    fn nav(steps: &[Step<&'static str>]) -> Vec<&'static str> {
        steps.iter().filter_map(|s| if let Step::Navigate(t) = s { Some(*t) } else { None }).collect()
    }

    #[test]
    fn first_view_of_an_environment_loads_on_its_own() {
        let mut c = Coordinator::default();
        assert_eq!(c.register(1, ENV, A, true, "a", 0), vec![Step::Navigate("a")]);
    }

    #[test]
    fn later_views_wait_for_a_loaded_opener_then_open_one_at_a_time() {
        let mut c = Coordinator::default();
        c.register(1, ENV, A, true, "a", 0);
        assert!(c.register(2, ENV, B, true, "b", 0).is_empty());
        assert!(c.register(3, ENV, C, true, "c", 0).is_empty());
        assert_eq!(c.loaded(1, 10), vec![Step::Open { opener: 1, url: B }]);
        // Only one request in flight: C waits until B's request has been answered.
        assert_eq!(c.accept(1, B), Some("b"));
        assert_eq!(c.loaded(2, 20), vec![Step::Open { opener: 1, url: C }]);
        assert_eq!(c.accept(1, C), Some("c"));
    }

    #[test]
    fn a_view_registering_after_the_anchor_loaded_opens_at_once() {
        let mut c = Coordinator::default();
        c.register(1, ENV, A, true, "a", 0);
        c.loaded(1, 5);
        assert_eq!(c.register(2, ENV, B, true, "b", 6), vec![Step::Open { opener: 1, url: B }]);
    }

    #[test]
    fn only_the_exact_url_from_the_chosen_opener_is_accepted() {
        let mut c = Coordinator::default();
        c.register(1, ENV, A, true, "a", 0);
        c.register(2, ENV, B, true, "b", 0);
        c.register(3, ENV, C, true, "c", 0);
        c.loaded(1, 1);
        c.loaded(3, 1);
        assert_eq!(c.accept(1, C), None, "wrong url");
        assert_eq!(c.accept(1, "https://example.com/"), None, "arbitrary url");
        assert_eq!(c.accept(1, "bowser-ui://app/index.html?view=b&x=1"), None, "not exact");
        assert_eq!(c.accept(3, B), None, "not the chosen opener");
        assert_eq!(c.accept(1, B), Some("b"));
        assert_eq!(c.accept(1, B), None, "one request, one answer");
    }

    #[test]
    fn nothing_is_accepted_without_a_request_in_flight() {
        let mut c: Coordinator<&str> = Coordinator::default();
        assert_eq!(c.accept(1, A), None);
        c.register(1, ENV, A, true, "a", 0);
        assert_eq!(c.accept(1, A), None);
    }

    #[test]
    fn environments_do_not_mix() {
        let mut c = Coordinator::default();
        c.register(1, 1, A, true, "a", 0);
        assert_eq!(c.register(2, 2, B, true, "b", 0), vec![Step::Navigate("b")], "own anchor in its own environment");
        c.loaded(1, 1);
        assert!(c.register(3, 2, C, true, "c", 1).is_empty(), "environment 2's anchor is not loaded yet");
    }

    #[test]
    fn a_closed_opener_is_not_needed_by_later_views() {
        let mut c = Coordinator::default();
        c.register(1, ENV, A, true, "a", 0);
        c.loaded(1, 1);
        c.register(2, ENV, B, true, "b", 1);
        assert_eq!(c.accept(1, B), Some("b"));
        c.loaded(2, 2);
        c.gone(1, 3);
        assert_eq!(c.register(3, ENV, C, true, "c", 4), vec![Step::Open { opener: 2, url: C }]);
    }

    #[test]
    fn opener_that_disappears_mid_request_hands_the_request_to_another_opener() {
        let mut c = Coordinator::default();
        c.register(1, ENV, A, true, "a", 0);
        c.register(2, ENV, B, true, "b", 0);
        c.loaded(1, 1);
        assert_eq!(c.accept(1, B), Some("b"));
        c.loaded(2, 2);
        c.register(3, ENV, C, true, "c", 3);
        // C asked opener 1; when 1 goes, the request is handed to opener 2.
        assert_eq!(c.gone(1, 4), vec![Step::Open { opener: 2, url: C }]);
        assert_eq!(c.accept(1, C), None);
        assert_eq!(c.accept(2, C), Some("c"));
    }

    #[test]
    fn all_views_gone_then_the_next_view_is_a_new_anchor() {
        let mut c = Coordinator::default();
        c.register(1, ENV, A, true, "a", 0);
        c.loaded(1, 1);
        c.gone(1, 2);
        assert_eq!(c.register(2, ENV, B, true, "b", 3), vec![Step::Navigate("b")]);
    }

    #[test]
    fn a_failed_renderer_is_never_chosen_as_opener() {
        let mut c = Coordinator::default();
        c.register(1, ENV, A, true, "a", 0);
        c.loaded(1, 1);
        c.failed(1, 2);
        // The only member is distrusted, so the recreated view becomes the new anchor.
        assert_eq!(c.register(2, ENV, B, true, "b", 3), vec![Step::Navigate("b")]);
        c.loaded(2, 4);
        assert_eq!(c.register(3, ENV, C, true, "c", 5), vec![Step::Open { opener: 2, url: C }]);
    }

    #[test]
    fn unanswered_open_falls_back_to_navigate_and_three_in_a_row_disable_it() {
        let mut c = Coordinator::default();
        c.register(1, ENV, A, true, "a", 0);
        c.loaded(1, 1);
        assert_eq!(c.register(2, ENV, B, true, "b", 2), vec![Step::Open { opener: 1, url: B }]);
        assert!(c.tick(2 + OPEN_TIMEOUT_MS - 1).is_empty());
        // Timeout distrusts opener 1; nobody else can open, so the view loads alone.
        assert_eq!(nav(&c.tick(2 + OPEN_TIMEOUT_MS)), vec!["b"]);
        assert!(!c.is_disabled());
        let mut c = Coordinator::default();
        for i in 0..3u64 {
            let (now, env) = (i * 10_000, i as usize + 1);
            c.register(10 + i, env, A, true, "x", now);
            c.loaded(10 + i, now);
            c.register(20 + i, env, B, true, "y", now);
            c.tick(now + OPEN_TIMEOUT_MS);
        }
        assert!(c.is_disabled());
        assert_eq!(c.register(99, ENV, C, true, "z", 99_000), vec![Step::Navigate("z")]);
    }

    #[test]
    fn a_successful_open_resets_the_timeout_count() {
        let mut c = Coordinator::default();
        c.register(1, ENV, A, true, "a", 0);
        c.loaded(1, 1);
        c.register(2, ENV, B, true, "b", 1);
        c.tick(1 + OPEN_TIMEOUT_MS);
        assert_eq!(c.timeouts, 1);
        c.register(3, ENV, C, true, "c", 5_000);
        c.loaded(3, 5_001);
        c.register(4, ENV, B, true, "d", 5_002);
        assert_eq!(c.accept(3, B), Some("d"));
        assert_eq!(c.timeouts, 0);
    }

    #[test]
    fn views_waiting_for_an_anchor_that_never_loads_load_alone() {
        let mut c = Coordinator::default();
        c.register(1, ENV, A, true, "a", 0);
        assert!(c.register(2, ENV, B, true, "b", 0).is_empty());
        assert_eq!(c.next_deadline(), Some(ANCHOR_WAIT_MS));
        assert!(c.tick(ANCHOR_WAIT_MS - 1).is_empty());
        assert_eq!(nav(&c.tick(ANCHOR_WAIT_MS)), vec!["b"]);
    }

    #[test]
    fn waiting_views_go_when_the_anchor_dies_before_loading() {
        let mut c = Coordinator::default();
        c.register(1, ENV, A, true, "a", 0);
        c.register(2, ENV, B, true, "b", 0);
        c.register(3, ENV, C, true, "c", 0);
        let steps = c.gone(1, 1);
        // The first waiting view becomes the anchor; the other keeps waiting for it.
        assert_eq!(nav(&steps), vec!["b"]);
        assert_eq!(c.loaded(2, 2), vec![Step::Open { opener: 2, url: C }]);
    }

    #[test]
    fn a_closed_requester_drops_its_request() {
        let mut c = Coordinator::default();
        c.register(1, ENV, A, true, "a", 0);
        c.loaded(1, 1);
        c.register(2, ENV, B, true, "b", 1);
        c.gone(2, 2);
        assert_eq!(c.accept(1, B), None);
        assert_eq!(c.next_deadline(), None);
    }

    #[test]
    fn the_split_host_is_used_as_opener_last() {
        let mut c = Coordinator::default();
        c.register(1, ENV, A, false, "split", 0);
        c.register(2, ENV, B, true, "b", 0);
        c.loaded(1, 1);
        // Opener 1 is the only loaded view, so it is used.
        assert_eq!(c.accept(1, B), Some("b"));
        c.loaded(2, 2);
        assert_eq!(c.register(3, ENV, C, true, "c", 3), vec![Step::Open { opener: 2, url: C }]);
    }

    #[test]
    fn open_script_only_embeds_plain_chrome_urls() {
        assert_eq!(open_script(A).as_deref(), Some("window.open(\"bowser-ui://app/index.html?view=a\");"));
        assert_eq!(open_script("\");alert(1);//"), None);
        assert_eq!(open_script("a b"), None);
        assert_eq!(open_script(""), None);
    }
}
