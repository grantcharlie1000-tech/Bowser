//! Document identity and initial-state transactions for the three persistent chrome views.
//! No receipt or send success is an application acknowledgement.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChromeView { Toolbar, Tabs, Rail }
impl ChromeView {
    pub fn wire(self) -> &'static str { match self { Self::Toolbar => "toolbar", Self::Tabs => "tabs", Self::Rail => "rail" } }
    pub fn parse(value: &str) -> Option<Self> { match value { "toolbar" => Some(Self::Toolbar), "tabs" => Some(Self::Tabs), "rail" => Some(Self::Rail), _ => None } }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentToken(String);
impl DocumentToken {
    pub fn parse(value: &str) -> Option<Self> {
        (value.len() == 32 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))).then(|| Self(value.to_owned()))
    }
    pub fn wire(&self) -> &str { &self.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Revision(u32);
impl Revision {
    pub fn new(value: u32) -> Option<Self> { (value != 0).then_some(Self(value)) }
    pub fn get(self) -> u32 { self.0 }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateIdentity { pub view: ChromeView, pub document: DocumentToken, pub revision: Revision }
impl StateIdentity {
    fn fields(&self) -> String {
        format!(r#""view":"{}","doc":"{}","rev":{}"#, self.view.wire(), self.document.wire(), self.revision.get())
    }
    pub fn frames(&self, parts: Vec<String>) -> Vec<String> {
        let fields = self.fields();
        let mut frames = Vec::with_capacity(parts.len() + 2);
        frames.push(format!(r#"{{"v":1,"t":"chromeBegin",{fields},"count":{}}}"#, parts.len()));
        frames.extend(parts.into_iter().enumerate().map(|(i, state)| format!(r#"{{"v":1,"t":"chromePart",{fields},"i":{i},"state":{state}}}"#)));
        frames.push(format!(r#"{{"v":1,"t":"chromeCommit",{fields}}}"#));
        frames
    }
}

#[derive(Debug)]
enum Phase {
    Waiting,
    Ready(DocumentToken),
    Collecting { id: StateIdentity, parts: Vec<String>, bytes: usize },
    WaitingApplied { id: StateIdentity, dirty: bool },
    Applied { id: StateIdentity, dirty: bool },
    Live(StateIdentity),
    Failed,
}

/// Complete bundles resent because state changed while the page was applying the previous one.
/// A slow machine can take longer to apply a bundle than the state stays still during startup
/// (extensions, downloads, window activation), so an unbounded resend never converges. After this
/// many resends the last applied bundle is revealed and the newer state follows as live updates.
pub(crate) const MAX_STATE_REFRESHES: u8 = 2;

#[derive(Debug)]
pub(crate) struct Session { view: ChromeView, epoch: u64, revision: u32, refreshes: u8, phase: Phase }
impl Session {
    pub(crate) fn new(view: ChromeView) -> Self { Self { view, epoch: 0, revision: 0, refreshes: 0, phase: Phase::Waiting } }
    pub(crate) fn navigate(&mut self) -> Option<u64> {
        if matches!(self.phase, Phase::Failed) { return None; }
        let Some(epoch) = self.epoch.checked_add(1) else { self.fail(); return None; };
        self.epoch = epoch;
        self.refreshes = 0;
        self.phase = Phase::Waiting;
        Some(self.epoch)
    }
    pub(crate) fn epoch(&self) -> u64 { self.epoch }
    pub(crate) fn awaiting_initial(&self) -> bool { matches!(self.phase, Phase::WaitingApplied { .. } | Phase::Applied { .. }) }
    pub(crate) fn awaiting_document(&self) -> bool { matches!(self.phase, Phase::Waiting) }
    pub(crate) fn ready(&mut self, epoch: u64, document: DocumentToken) -> bool {
        if epoch != self.epoch || !matches!(self.phase, Phase::Waiting) { return false; }
        self.phase = Phase::Ready(document);
        true
    }
    pub(crate) fn document_current(&self, document: &DocumentToken) -> bool {
        match &self.phase {
            Phase::Ready(d) => d == document,
            Phase::Collecting { id, .. } | Phase::WaitingApplied { id, .. } | Phase::Applied { id, .. } | Phase::Live(id) => &id.document == document,
            _ => false,
        }
    }
    pub(crate) fn begin(&mut self) -> Option<StateIdentity> {
        let document = match &self.phase { Phase::Ready(d) => d.clone(), Phase::Applied { id, .. } => id.document.clone(), _ => return None };
        let revision = self.revision.checked_add(1).and_then(Revision::new)?;
        self.revision = revision.get();
        if matches!(self.phase, Phase::Applied { .. }) { self.refreshes = self.refreshes.saturating_add(1); }
        let id = StateIdentity { view: self.view, document, revision };
        self.phase = Phase::Collecting { id: id.clone(), parts: Vec::new(), bytes: 0 };
        Some(id)
    }
    /// None means normal live delivery; Some records whether the initial-state queue accepted it.
    pub(crate) fn capture(&mut self, json: &str) -> Option<bool> {
        match &mut self.phase {
            Phase::Collecting { parts, bytes, .. } => {
                if parts.len() >= 1100 || json.len() > 256 * 1024 || *bytes + json.len() > 16 * 1024 * 1024 { self.fail(); return Some(false); }
                *bytes += json.len(); parts.push(json.to_owned()); Some(true)
            }
            Phase::WaitingApplied { dirty, .. } | Phase::Applied { dirty, .. } => { *dirty = true; Some(false) }
            Phase::Live(_) => None,
            _ => Some(false),
        }
    }
    pub(crate) fn finish(&mut self) -> Option<(StateIdentity, Vec<String>)> {
        let Phase::Collecting { id, parts, .. } = std::mem::replace(&mut self.phase, Phase::Failed) else { return None; };
        if parts.is_empty() { return None; }
        self.phase = Phase::WaitingApplied { id: id.clone(), dirty: false };
        Some((id, parts))
    }
    pub(crate) fn acknowledge(&mut self, id: &StateIdentity) -> bool {
        let Phase::WaitingApplied { id: expected, dirty } = &self.phase else { return false; };
        if expected != id { return false; }
        self.phase = Phase::Applied { id: id.clone(), dirty: *dirty };
        true
    }
    pub(crate) fn needs_refresh(&self, id: &StateIdentity) -> bool {
        self.stale(id) && self.refreshes < MAX_STATE_REFRESHES
    }
    /// The applied bundle is current, but state changed while it was being applied.
    pub(crate) fn stale(&self, id: &StateIdentity) -> bool { matches!(&self.phase, Phase::Applied { id: current, dirty: true } if current == id) }
    /// Clean, or stale once the resend budget is spent: the caller then resends state live.
    pub(crate) fn can_reveal(&self, id: &StateIdentity) -> bool {
        matches!(&self.phase, Phase::Applied { id: current, dirty } if current == id && (!*dirty || self.refreshes >= MAX_STATE_REFRESHES))
    }
    pub(crate) fn revealed(&mut self, id: &StateIdentity) -> bool {
        if !self.can_reveal(id) { return false; }
        self.phase = Phase::Live(id.clone()); true
    }
    pub(crate) fn live(&self) -> bool { matches!(self.phase, Phase::Live(_)) }
    pub(crate) fn applied_identity(&self) -> Option<StateIdentity> {
        match &self.phase { Phase::Applied { id, .. } => Some(id.clone()), _ => None }
    }
    pub(crate) fn fail(&mut self) { self.phase = Phase::Failed; }
    pub(crate) fn accepts_failure(&self, id: &StateIdentity) -> bool {
        matches!(&self.phase, Phase::WaitingApplied { id: current, .. } | Phase::Applied { id: current, .. } if current == id)
    }
}

/// Every native operation can pump callbacks. Recheck all leases between operations and before
/// publishing ownership, with no state borrow held over the transport call.
pub(crate) fn reveal_transaction<E>(mut current: impl FnMut() -> bool, bounds: impl FnOnce() -> Result<(), E>, visible: impl FnOnce() -> Result<(), E>) -> Result<bool, E> {
    if !current() { return Ok(false); }
    bounds()?;
    if !current() { return Ok(false); }
    visible()?;
    Ok(current())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};
    fn token(n: u8) -> DocumentToken { DocumentToken::parse(&format!("{n:032x}")).unwrap() }
    fn pending(view: ChromeView) -> (Session, StateIdentity) {
        let mut s = Session::new(view);
        let epoch = s.navigate().unwrap();
        assert!(s.ready(epoch, token(1)));
        let id = s.begin().unwrap();
        assert_eq!(s.capture(r#"{"t":"state"}"#), Some(true));
        let (sent, parts) = s.finish().unwrap();
        assert_eq!(sent, id);
        assert_eq!(parts.len(), 1);
        (s, id)
    }
    #[test]
    fn every_view_waits_for_its_exact_applied_revision_and_successful_native_reveal() {
        for view in [ChromeView::Toolbar, ChromeView::Tabs, ChromeView::Rail] {
            let (mut s, id) = pending(view);
            assert!(!s.live());
            assert!(!s.can_reveal(&id));
            assert!(!s.revealed(&id));
            let mut wrong = id.clone(); wrong.document = token(2);
            assert!(!s.acknowledge(&wrong));
            wrong = id.clone(); wrong.revision = Revision::new(2).unwrap();
            assert!(!s.acknowledge(&wrong));
            wrong = id.clone(); wrong.view = if view == ChromeView::Toolbar { ChromeView::Rail } else { ChromeView::Toolbar };
            assert!(!s.acknowledge(&wrong));
            assert!(s.acknowledge(&id));
            assert!(!s.acknowledge(&id));
            assert!(reveal_transaction(|| s.can_reveal(&id), || Ok::<_, ()>(()), || Ok(())).unwrap());
            assert!(s.revealed(&id));
            assert!(s.live());
            assert_eq!(s.capture("normal update"), None);
        }
    }
    #[test]
    fn navigation_failure_and_close_revoke_old_readiness_and_acknowledgements() {
        let (mut s, id) = pending(ChromeView::Toolbar);
        let old_epoch = s.epoch();
        let next = s.navigate().unwrap();
        assert!(!s.ready(old_epoch, token(1)));
        assert!(!s.acknowledge(&id));
        assert!(!s.document_current(&id.document));
        assert!(s.ready(next, token(2)));
        let replacement = s.begin().unwrap();
        assert!(replacement.revision.get() > id.revision.get());
        s.fail();
        assert!(s.navigate().is_none(), "a failed controller cannot recover by reloading its document");
        assert!(!s.ready(next, token(2)));
        assert!(!s.acknowledge(&replacement));
        assert!(!s.revealed(&replacement));
        assert_eq!(s.capture("late"), Some(false));
    }
    #[test]
    fn updates_during_application_require_a_new_complete_revision() {
        let (mut s, id) = pending(ChromeView::Tabs);
        assert_eq!(s.capture("new state"), Some(false));
        assert!(s.acknowledge(&id));
        assert!(s.needs_refresh(&id));
        assert!(!s.can_reveal(&id));
        let newer = s.begin().unwrap();
        assert!(newer.revision.get() > id.revision.get());
        assert_eq!(s.capture("complete current state"), Some(true));
        s.finish().unwrap();
        assert!(!s.acknowledge(&id));
        assert!(s.acknowledge(&newer));
        assert!(s.revealed(&newer));
    }
    #[test]
    fn continuous_state_churn_stops_resending_and_reveals_after_the_budget() {
        let (mut s, mut id) = pending(ChromeView::Toolbar);
        for _ in 0..MAX_STATE_REFRESHES {
            assert_eq!(s.capture("changed while applying"), Some(false));
            assert!(s.acknowledge(&id));
            assert!(s.stale(&id));
            assert!(s.needs_refresh(&id));
            assert!(!s.can_reveal(&id));
            id = s.begin().unwrap();
            assert_eq!(s.capture("complete current state"), Some(true));
            s.finish().unwrap();
        }
        assert_eq!(s.capture("still changing"), Some(false));
        assert!(s.acknowledge(&id));
        assert!(s.stale(&id));
        assert!(!s.needs_refresh(&id), "the resend budget is spent");
        assert!(s.can_reveal(&id));
        assert!(s.revealed(&id));
        assert!(s.live());
        assert_eq!(s.capture("live update"), None, "newer state is delivered live after the reveal");
    }
    #[test]
    fn a_new_document_gets_a_fresh_resend_budget() {
        let (mut s, mut id) = pending(ChromeView::Tabs);
        for _ in 0..MAX_STATE_REFRESHES {
            assert_eq!(s.capture("changed while applying"), Some(false));
            assert!(s.acknowledge(&id));
            id = s.begin().unwrap();
            assert_eq!(s.capture("complete current state"), Some(true));
            s.finish().unwrap();
        }
        let epoch = s.navigate().unwrap();
        assert!(s.ready(epoch, token(9)));
        let id = s.begin().unwrap();
        assert_eq!(s.capture("state"), Some(true));
        s.finish().unwrap();
        assert_eq!(s.capture("changed while applying"), Some(false));
        assert!(s.acknowledge(&id));
        assert!(s.needs_refresh(&id));
        assert!(!s.can_reveal(&id));
    }
    #[test]
    fn empty_or_oversize_initial_transactions_never_wait_for_an_ack() {
        let mut s = Session::new(ChromeView::Rail);
        assert!(s.ready(0, token(1)));
        s.begin().unwrap();
        assert!(s.finish().is_none());
        assert!(s.begin().is_none());
        let mut s = Session::new(ChromeView::Rail);
        let epoch = s.navigate().unwrap(); s.ready(epoch, token(2)); s.begin();
        assert_eq!(s.capture(&"x".repeat(256 * 1024 + 1)), Some(false));
        assert!(s.finish().is_none());
    }
    #[test]
    fn native_reveal_failure_or_reentrant_lease_revocation_cannot_publish_ownership() {
        let lease = Rc::new(Cell::new(true));
        let visible_calls = Cell::new(0);
        assert_eq!(reveal_transaction(|| lease.get(), || Err("bounds"), || { visible_calls.set(1); Ok(()) }), Err("bounds"));
        assert_eq!(visible_calls.get(), 0);
        assert_eq!(reveal_transaction(|| lease.get(), || Ok(()), || Err("visibility")), Err("visibility"));
        assert_eq!(reveal_transaction(|| lease.get(), || { lease.set(false); Ok::<_, ()>(()) }, || { visible_calls.set(1); Ok(()) }), Ok(false));
        assert_eq!(visible_calls.get(), 0);
        lease.set(true);
        assert_eq!(reveal_transaction(|| lease.get(), || Ok::<_, ()>(()), || { lease.set(false); Ok(()) }), Ok(false));
    }
}
