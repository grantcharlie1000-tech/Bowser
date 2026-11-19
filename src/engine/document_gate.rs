//! Controller-local evidence for a successfully completed current top-level document.
//! This never trusts an application's requested address. Ambiguous/coalesced host requests stay
//! ineligible rather than guessing which request a native event belongs to.
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

#[derive(Default)]
pub(super) struct DocumentGate {
    epoch: u64,
    source_revision: u64,
    pending: VecDeque<u64>,
    latest: Option<(u64, u64)>, // native NavigationId, host/native epoch
    seen: HashSet<u64>,
    confirmed: bool,
    closed: bool,
    saturated: bool,
    thumbnail_cancelled: Option<Arc<AtomicBool>>,
}
impl DocumentGate {
    fn advance(&mut self) -> Option<u64> {
        self.cancel_thumbnails();
        self.confirmed = false;
        self.epoch = match self.epoch.checked_add(1) { Some(value) => value, None => { self.saturated = true; return None; } };
        Some(self.epoch)
    }
    pub(super) fn request(&mut self) {
        if self.closed || self.saturated { return; }
        let Some(epoch) = self.advance() else { return; };
        if self.pending.len() >= 64 { self.saturated = true; return; }
        self.pending.push_back(epoch);
    }
    pub(super) fn invalidate(&mut self) { let _ = self.advance(); }
    pub(super) fn unreadable_event(&mut self) { self.invalidate(); self.latest = None; }
    pub(super) fn started(&mut self, id: u64) {
        self.confirmed = false;
        if self.closed || self.saturated { return; }
        // Redirects retain their native ID and cannot consume a newer host request.
        if self.latest.is_some_and(|(current, _)| current == id) { return; }
        if self.seen.contains(&id) { return; } // A stale earlier navigation cannot replace B.
        if self.seen.len() >= 4096 { self.saturated = true; return; }
        let epoch = match self.pending.pop_front() { Some(epoch) => epoch, None => match self.advance() { Some(epoch) => epoch, None => return } };
        self.seen.insert(id);
        self.latest = Some((id, epoch));
    }
    pub(super) fn completed(&mut self, id: u64, success: bool) {
        if self.closed || self.saturated { return; }
        let Some((current, epoch)) = self.latest else { return; };
        if current != id { return; }
        self.confirmed = success && epoch == self.epoch && self.pending.is_empty();
    }
    pub(super) fn eligible(&self) -> bool {
        !self.closed && !self.saturated && self.confirmed && self.pending.is_empty()
            && self.latest.is_some_and(|(_, epoch)| epoch == self.epoch)
    }
    pub(super) fn source_changed(&mut self) {
        self.cancel_thumbnails();
        match self.source_revision.checked_add(1) { Some(value) => self.source_revision = value, None => self.saturated = true }
    }
    pub(super) fn proof(&self) -> Option<(u64, u64, u64)> {
        if self.eligible() { self.latest.map(|(id, epoch)| (id, epoch, self.source_revision)) } else { None }
    }
    /// Captures can start at DOM readiness. The token is cancelled when this page
    /// changes, even if the next address is the same or its capture starts later.
    pub(super) fn thumbnail_token(&mut self) -> Arc<AtomicBool> {
        self.thumbnail_cancelled
            .get_or_insert_with(|| Arc::new(AtomicBool::new(self.closed)))
            .clone()
    }
    fn cancel_thumbnails(&mut self) {
        if let Some(cancelled) = self.thumbnail_cancelled.take() {
            cancelled.store(true, Ordering::Relaxed);
        }
    }
    pub(super) fn close(&mut self) {
        self.cancel_thumbnails();
        self.closed = true;
        self.confirmed = false;
        self.pending.clear();
    }
}
impl Drop for DocumentGate {
    fn drop(&mut self) {
        self.cancel_thumbnails();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn no_document_until_native_success() { let mut g=DocumentGate::default(); assert!(!g.eligible()); g.started(1); assert!(!g.eligible()); g.completed(1,true); assert!(g.eligible()); }
    #[test] fn requested_navigation_revokes_before_start_and_stale_completion() { let mut g=DocumentGate::default(); g.started(1);g.completed(1,true);g.request();assert!(!g.eligible());g.completed(1,true);assert!(!g.eligible());g.started(2);g.completed(2,true);assert!(g.eligible()); }
    #[test] fn a_start_b_start_a_complete_never_reopens_a() { let mut g=DocumentGate::default();g.request();g.started(10);g.request();g.started(20);g.completed(10,true);assert!(!g.eligible());g.completed(20,true);assert!(g.eligible());g.started(10);g.completed(10,true);assert!(!g.eligible()); }
    #[test] fn failed_cancelled_unreadable_and_stopped_are_ineligible() { let mut g=DocumentGate::default();g.started(1);g.completed(1,false);assert!(!g.eligible());g.started(2);g.completed(2,true);g.invalidate();g.completed(2,true);assert!(!g.eligible());g.unreadable_event();g.completed(2,true);assert!(!g.eligible()); }
    #[test] fn overlapping_same_url_host_requests_need_latest_native_success() { let mut g=DocumentGate::default();g.request();g.request();g.started(1);g.completed(1,true);assert!(!g.eligible());g.started(2);g.completed(2,true);assert!(g.eligible()); }
    #[test] fn redirects_do_not_consume_pending_newer_request() { let mut g=DocumentGate::default();g.request();g.started(1);g.request();g.started(1);g.completed(1,true);assert!(!g.eligible());g.started(2);g.completed(2,true);assert!(g.eligible()); }
    #[test] fn same_document_source_change_invalidates_only_the_old_query_token() { let mut g=DocumentGate::default();g.started(1);g.completed(1,true);let before=g.proof();g.source_changed();assert!(g.eligible());assert_ne!(g.proof(),before);g.source_revision=u64::MAX;g.source_changed();assert!(!g.eligible()); }
    #[test] fn proof_token_changes_even_when_a_new_document_is_also_eligible() { let mut g=DocumentGate::default();g.started(1);g.completed(1,true);let before=g.proof().unwrap();g.request();g.started(2);g.completed(2,true);assert!(g.eligible());assert_ne!(g.proof(),Some(before));g.close();assert!(g.proof().is_none()); }
    #[test] fn closed_controller_ignores_late_native_events() { let mut g=DocumentGate::default();g.request();g.started(1);g.close();g.completed(1,true);g.started(2);g.completed(2,true);assert!(!g.eligible()); }
    #[test] fn coalesced_or_excessive_request_mapping_fails_closed() { let mut g=DocumentGate::default();g.request();g.request();g.started(2);g.completed(2,true);assert!(!g.eligible());for _ in 0..65 {g.request();}g.started(3);g.completed(3,true);assert!(!g.eligible()); }
    #[test]
    fn thumbnail_work_is_cancelled_by_navigation_source_changes_and_close() {
        let mut gate = DocumentGate::default();
        gate.started(1);
        let loaded = gate.thumbnail_token();
        gate.completed(1, true);
        assert!(!loaded.load(Ordering::Relaxed));
        gate.source_changed();
        assert!(loaded.load(Ordering::Relaxed));
        let same_document = gate.thumbnail_token();
        gate.request();
        assert!(same_document.load(Ordering::Relaxed));
        gate.started(2);
        let reloaded = gate.thumbnail_token();
        gate.close();
        assert!(reloaded.load(Ordering::Relaxed));
        assert!(gate.thumbnail_token().load(Ordering::Relaxed));
    }

    #[test]
    fn dropping_a_document_cancels_queued_thumbnails() {
        let mut gate = DocumentGate::default();
        let cancelled = gate.thumbnail_token();
        drop(gate);
        assert!(cancelled.load(Ordering::Relaxed));
    }
}
