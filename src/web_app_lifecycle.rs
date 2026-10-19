//! Pure ownership rules for asynchronous pinned-app creation. Native COM work is deliberately
//! outside these state transitions: reserving precedes dispatch, invalidation precedes cleanup.
use std::{cell::Cell, collections::HashMap, rc::Rc};

/// Only actual host handles clone this marker. Async callbacks may temporarily retain state but
/// are not owners and must not postpone retirement of the final host handle.
#[derive(Clone, Default)]
pub(crate) struct HostOwners(Rc<()>);
impl HostOwners {
    pub(crate) fn is_last(&self) -> bool { Rc::strong_count(&self.0) == 1 }
}

#[derive(Clone, Debug)]
pub(crate) struct Lease {
    generation: u64,
    live: Rc<Cell<bool>>,
}

impl Lease {
    pub(crate) fn is_live(&self) -> bool { self.live.get() }
    pub(crate) fn invalidate(&self) { self.live.set(false); }
}

struct Slot { lease: Lease, pending: bool }

#[derive(Default)]
pub(crate) struct CreationGate {
    next: u64,
    closed: bool,
    slots: HashMap<String, Slot>,
}

impl CreationGate {
    /// None coalesces an existing request or view. Exhaustion/closed is an explicit error;
    /// generation values never wrap and can never make an old callback current again.
    pub(crate) fn reserve(&mut self, key: &str) -> Result<Option<Lease>, ()> {
        if self.closed { return Err(()); }
        if self.slots.contains_key(key) { return Ok(None); }
        self.next = self.next.checked_add(1).ok_or(())?;
        let lease = Lease { generation: self.next, live: Rc::new(Cell::new(true)) };
        self.slots.insert(key.to_owned(), Slot { lease: lease.clone(), pending: true });
        Ok(Some(lease))
    }
    pub(crate) fn pending(&self, key: &str, lease: &Lease) -> bool {
        lease.is_live() && self.slots.get(key).is_some_and(|slot|
            slot.pending && slot.lease.generation == lease.generation
            && Rc::ptr_eq(&slot.lease.live, &lease.live))
    }
    pub(crate) fn commit(&mut self, key: &str, lease: &Lease) -> bool {
        if !self.pending(key, lease) { return false; }
        self.slots.get_mut(key).expect("pending slot exists").pending = false;
        true
    }
    /// A late synchronous/async error must not remove a replacement or an installed view.
    pub(crate) fn fail(&mut self, key: &str, lease: &Lease) -> bool {
        if !self.pending(key, lease) { return false; }
        self.remove(key);
        true
    }
    pub(crate) fn remove(&mut self, key: &str) {
        if let Some(slot) = self.slots.remove(key) { slot.lease.invalidate(); }
    }
    pub(crate) fn invalidate_all(&mut self) {
        for (_, slot) in self.slots.drain() { slot.lease.invalidate(); }
    }
    pub(crate) fn keys(&self) -> Vec<String> { self.slots.keys().cloned().collect() }
    pub(crate) fn close(&mut self) { self.closed = true; self.invalidate_all(); }
}

/// Privacy barrier shared by all hosts using one isolated profile, including retired hosts.
/// Completion and browser release are independent: neither alone authorizes deletion.
#[derive(Default)]
pub(crate) struct ProfileRetirement {
    next: u64,
    clearing: bool,
    unknown: bool,
    requests: HashMap<u64, Option<u64>>,
    environments: HashMap<u64, EnvironmentRelease>,
}
#[derive(Default)]
struct EnvironmentRelease {
    registered: bool,
    retired: bool,
    serial: u64,
    sessions: HashMap<u32, u64>,
    pending_identity: HashMap<u64, u64>,
    uncertain: bool,
    exits: HashMap<u32, u64>,
}
impl EnvironmentRelease {
    fn released(&self) -> bool {
        self.registered && !self.uncertain && self.pending_identity.is_empty()
            && !self.sessions.is_empty() && self.sessions.iter().all(|(pid, dispatch)|
                self.exits.get(pid).is_some_and(|exit| exit > dispatch))
    }
    fn unidentified(&self) -> bool {
        !self.registered || self.uncertain || (self.sessions.is_empty() && self.pending_identity.is_empty())
    }
}

impl ProfileRetirement {
    #[cfg(test)]
    pub(crate) fn request_pending_for_test(&self, request: u64) -> bool {
        self.requests.contains_key(&request)
    }
    pub(crate) fn request(&mut self, environment: Option<u64>) -> Option<u64> {
        if self.clearing { return None; }
        self.next = self.next.checked_add(1)?;
        if let Some(id) = environment {
            let env = self.environments.get_mut(&id)?;
            env.serial = env.serial.checked_add(1)?;
            env.pending_identity.insert(self.next, env.serial);
        }
        self.requests.insert(self.next, environment);
        Some(self.next)
    }
    pub(crate) fn environment(&mut self) -> Option<u64> {
        self.next = self.next.checked_add(1)?;
        self.environments.insert(self.next, EnvironmentRelease::default());
        Some(self.next)
    }
    pub(crate) fn attach_environment(&mut self, request: u64, id: u64) {
        if let Some(pending) = self.requests.get_mut(&request) { *pending = Some(id); }
        else { self.unknown = true; }
    }
    pub(crate) fn registration(&mut self, id: u64, success: bool) {
        if let Some(env) = self.environments.get_mut(&id) { env.registered = success; }
        else { self.unknown = true; }
    }
    pub(crate) fn browser_pid(&mut self, id: u64, request: u64, pid: Option<u32>) {
        if let Some(env) = self.environments.get_mut(&id) {
            if let Some(pid) = pid.filter(|pid| *pid != 0) {
                if let Some(serial) = env.pending_identity.remove(&request) {
                    env.sessions.entry(pid).and_modify(|latest| *latest = (*latest).max(serial)).or_insert(serial);
                } else { env.uncertain = true; }
            }
        } else { self.unknown = true; }
    }
    pub(crate) fn released(&mut self, id: u64, pid: Option<u32>) {
        if let Some(env) = self.environments.get_mut(&id) {
            if let (Some(serial), Some(pid)) = (env.serial.checked_add(1), pid.filter(|pid| *pid != 0)) {
                env.serial = serial;
                env.exits.insert(pid, serial);
            } else { self.unknown = true; }
        } else { self.unknown = true; }
    }
    pub(crate) fn retire_environment(&mut self, id: u64) {
        if let Some(env) = self.environments.get_mut(&id) { env.retired = true; }
    }
    /// Released retired subscriptions can be detached at a safe outer boundary. Unidentifiable
    /// retired failures are also detached to bound COM retention, but leave a sticky privacy error.
    pub(crate) fn reapable(&mut self) -> Vec<u64> {
        let ids = self.environments.iter().filter_map(|(id, env)| {
            (env.retired && !self.requests.values().any(|pending| *pending == Some(*id))
                && (env.released() || env.unidentified())).then_some(*id)
        }).collect::<Vec<_>>();
        for id in &ids {
            if let Some(env) = self.environments.remove(id) { self.unknown |= !env.released(); }
        }
        ids
    }
    pub(crate) fn uncertain(&mut self) { self.unknown = true; }
    pub(crate) fn finish(&mut self, request: u64, environment_observed: bool) {
        if let Some(environment) = self.requests.remove(&request) {
            if environment.is_none() && !environment_observed { self.unknown = true; }
            if let Some(env) = environment.and_then(|id| self.environments.get_mut(&id)) {
                // Collapse failed/unidentified requests to a sticky error, rather than retaining
                // an unbounded row for every retry. Successful requests share one entry per PID.
                if env.pending_identity.remove(&request).is_some() { env.uncertain = true; }
            }
        }
    }
    pub(crate) fn begin_clear(&mut self) { self.clearing = true; }
    pub(crate) fn is_clearing(&self) -> bool { self.clearing }
    pub(crate) fn can_erase(&self) -> bool {
        self.clearing && !self.unknown && self.requests.is_empty()
            && self.environments.values().all(EnvironmentRelease::released)
    }
    pub(crate) fn erased(&mut self) {
        assert!(self.can_erase());
        self.environments.clear();
        self.clearing = false;
    }
}

/// Transactional detach: callers retain failures and must not erase after a partial failure.
/// Called outside state borrows, since native unsubscription/destruction can reenter.
pub(crate) fn detach<T, E>(entries: HashMap<u64, T>, mut remove: impl FnMut(&T) -> Result<(), E>) -> HashMap<u64, T> {
    entries.into_iter().filter_map(|(id, entry)| remove(&entry).err().map(|_| (id, entry))).collect()
}

/// Reconcile only real state changes; no sleeps or unbounded COM retry. A newly created view is
/// installable only after a stable application. Churn/retirement leaves it uninstalled for cleanup.
pub(crate) fn apply_current<T: Eq, E>(mut snapshot: impl FnMut() -> Option<T>,
    mut apply: impl FnMut(&T) -> Result<(), E>) -> Result<bool, E> {
    for _ in 0..4 {
        let Some(target) = snapshot() else { return Ok(false); };
        apply(&target)?;
        if snapshot().as_ref() == Some(&target) { return Ok(true); }
    }
    Ok(false)
}

pub(crate) fn retry_route<T>(has_app: bool, reload: impl FnOnce() -> T, create: impl FnOnce() -> T) -> T {
    if has_app { reload() } else { create() }
}

/// A synchronous completion may already have consumed this reservation (or reentered a retry)
/// before the native begin call returns. Only its still-owned pending token can report an error.
pub(crate) fn dispatch_outcome<E>(result: Result<(), E>, retire_pending: impl FnOnce() -> bool) -> Result<(), E> {
    match result {
        Ok(()) => Ok(()),
        Err(error) => if retire_pending() { Err(error) } else { Ok(()) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    fn begin(gate: &mut CreationGate, id: &str) -> Lease { gate.reserve(id).unwrap().unwrap() }
    #[test]
    fn duplicate_and_reentrant_requests_are_single_flight() {
        let mut gate = CreationGate::default(); let a = begin(&mut gate, "A");
        assert!(gate.reserve("A").unwrap().is_none());
        assert!(gate.commit("A", &a));
        assert!(gate.reserve("A").unwrap().is_none());
        assert!(!gate.fail("A", &a)); assert!(a.is_live());
    }
    #[test]
    fn a_b_a_and_out_of_order_completions_retain_both_legitimate_views() {
        let mut gate = CreationGate::default(); let a = begin(&mut gate, "A");
        let b = begin(&mut gate, "B"); assert!(gate.reserve("A").unwrap().is_none());
        assert!(gate.commit("B", &b)); assert!(gate.commit("A", &a));
        assert!(a.is_live() && b.is_live());
    }
    #[test]
    fn remove_readd_rejects_stale_success_and_failure() {
        let mut gate = CreationGate::default(); let old = begin(&mut gate, "A");
        gate.remove("A"); let new = begin(&mut gate, "A");
        assert!(!old.is_live()); assert!(!gate.commit("A", &old));
        assert!(!gate.fail("A", &old)); assert!(gate.pending("A", &new));
        assert!(gate.commit("A", &new));
    }
    #[test]
    fn sync_and_async_failure_allow_retry_without_clearing_reentrant_replacement() {
        let mut gate = CreationGate::default(); let first = begin(&mut gate, "A");
        assert!(gate.fail("A", &first)); let retry = begin(&mut gate, "A");
        assert!(!gate.fail("A", &first)); assert!(gate.commit("A", &retry));
    }
    #[test]
    fn clear_invalidates_pending_and_ready_without_resetting_generation() {
        let mut gate = CreationGate::default(); let a = begin(&mut gate, "A");
        let b = begin(&mut gate, "B"); assert!(gate.commit("B", &b));
        gate.invalidate_all(); let new = begin(&mut gate, "A");
        assert!(!a.is_live() && !b.is_live()); assert!(new.generation > b.generation);
        assert!(!gate.commit("A", &a)); assert!(gate.commit("A", &new));
    }
    #[test]
    fn shutdown_and_generation_exhaustion_fail_closed() {
        let mut gate = CreationGate::default(); let a = begin(&mut gate, "A");
        gate.close(); assert!(!a.is_live()); assert!(gate.reserve("B").is_err());
        let mut exhausted = CreationGate { next: u64::MAX, ..Default::default() };
        assert!(exhausted.reserve("A").is_err());
    }
    #[test]
    fn clear_waits_for_both_callback_retirement_and_native_release_in_either_order() {
        for release_first in [true, false] {
            let mut profile = ProfileRetirement::default();
            let request = profile.request(None).unwrap();
            let env = profile.environment().unwrap(); profile.registration(env, true);
            let controller = profile.request(Some(env)).unwrap();
            profile.browser_pid(env, controller, Some(42)); profile.finish(controller, true); profile.begin_clear();
            assert!(profile.request(None).is_none());
            if release_first { profile.released(env, Some(42)); }
            else { profile.finish(request, true); }
            assert!(!profile.can_erase());
            if release_first { profile.finish(request, true); }
            else { profile.released(env, Some(42)); }
            assert!(profile.can_erase()); profile.erased();
            assert!(profile.request(None).is_some());
        }
    }
    #[test]
    fn repeated_clear_and_new_requests_cannot_reuse_old_release_evidence() {
        let mut p = ProfileRetirement::default(); let e = p.environment().unwrap();
        p.registration(e, true); let initial = p.request(Some(e)).unwrap();
        p.browser_pid(e, initial, Some(42)); p.finish(initial, true); p.released(e, Some(42));
        let r = p.request(Some(e)).unwrap(); p.browser_pid(e, r, Some(42)); p.begin_clear(); p.begin_clear();
        p.finish(r, true); assert!(!p.can_erase());
        p.released(e, Some(41)); assert!(!p.can_erase());
        p.released(e, Some(42)); assert!(p.can_erase());
    }
    #[test]
    fn unknown_registration_pid_or_unobserved_environment_never_authorizes_erase() {
        let mut p = ProfileRetirement::default(); let e = p.environment().unwrap();
        p.released(e, Some(42)); p.begin_clear(); assert!(!p.can_erase());
        p.registration(e, true); assert!(!p.can_erase());
        let mut missing = ProfileRetirement::default(); let r = missing.request(None).unwrap();
        missing.finish(r, false); missing.begin_clear(); assert!(!missing.can_erase());
    }
    #[test]
    fn old_exit_after_new_failed_request_cannot_establish_current_session_release() {
        let mut p = ProfileRetirement::default(); let e = p.environment().unwrap(); p.registration(e, true);
        let first = p.request(Some(e)).unwrap(); p.browser_pid(e, first, Some(42)); p.finish(first, true);
        let failed = p.request(Some(e)).unwrap(); p.finish(failed, true); p.begin_clear();
        p.released(e, Some(42)); assert!(!p.can_erase());
    }
    #[test]
    fn environment_only_and_one_success_cannot_hide_another_unidentified_request() {
        let mut p = ProfileRetirement::default(); let e = p.environment().unwrap(); p.registration(e, true);
        p.released(e, Some(42)); p.begin_clear(); assert!(!p.can_erase());
        let mut p = ProfileRetirement::default(); let e = p.environment().unwrap(); p.registration(e, true);
        let failed = p.request(Some(e)).unwrap(); let good = p.request(Some(e)).unwrap();
        p.finish(failed, true); p.browser_pid(e, good, Some(42)); p.finish(good, true);
        p.begin_clear(); p.released(e, Some(42)); assert!(!p.can_erase());
    }

    #[test]
    fn out_of_order_completions_require_release_for_every_identified_session() {
        let mut p = ProfileRetirement::default(); let e = p.environment().unwrap(); p.registration(e, true);
        let old = p.request(Some(e)).unwrap(); let new = p.request(Some(e)).unwrap();
        p.browser_pid(e, new, Some(43)); p.finish(new, true);
        p.browser_pid(e, old, Some(42)); p.finish(old, true); p.begin_clear();
        p.released(e, Some(42)); assert!(!p.can_erase());
        p.released(e, Some(43)); assert!(p.can_erase());
    }
    #[test]
    fn retired_failures_do_not_retain_native_environments_or_become_clear_success() {
        let mut p = ProfileRetirement::default(); let e = p.environment().unwrap();
        p.registration(e, true); p.retire_environment(e);
        assert_eq!(p.reapable(), vec![e]); p.begin_clear(); assert!(!p.can_erase());
        let mut p = ProfileRetirement::default(); let e = p.environment().unwrap(); p.registration(e, true);
        let r = p.request(Some(e)).unwrap(); p.browser_pid(e, r, Some(42)); p.retire_environment(e);
        assert!(p.reapable().is_empty()); p.finish(r, true); assert!(p.reapable().is_empty());
        p.released(e, Some(42)); assert_eq!(p.reapable(), vec![e]); p.begin_clear(); assert!(p.can_erase());
    }

    #[test]
    fn partial_detach_retains_failure_for_explicit_retry() {
        let entries = HashMap::from([(1, false), (2, true)]);
        let failed = detach(entries, |success| if *success { Ok(()) } else { Err(()) });
        assert_eq!(failed, HashMap::from([(1, false)]));
        assert!(detach(failed, |_| Ok::<(), ()>(())).is_empty());
    }

    #[test]
    fn repeated_same_browser_successes_and_failures_do_not_grow_per_attempt_state() {
        let mut p = ProfileRetirement::default(); let e = p.environment().unwrap(); p.registration(e, true);
        for _ in 0..1000 {
            let r = p.request(Some(e)).unwrap(); p.browser_pid(e, r, Some(42)); p.finish(r, true);
            let r = p.request(Some(e)).unwrap(); p.finish(r, true);
        }
        let env = &p.environments[&e];
        assert_eq!(env.sessions.len(), 1); assert!(env.pending_identity.is_empty()); assert!(env.uncertain);
    }

    #[test]
    fn completion_applies_latest_active_bounds_and_hide_after_reentrant_change() {
        for latest in [(Some("B"), [0, 0, 500, 600]), (None, [0, 0, 20, 30])] {
            let state = RefCell::new(Some((Some("A"), [0, 0, 100, 100])));
            let calls = Cell::new(0); let applied = RefCell::new(None);
            assert!(apply_current(|| *state.borrow(), |target| {
                calls.set(calls.get() + 1); *applied.borrow_mut() = Some(*target);
                if calls.get() == 1 { *state.borrow_mut() = Some(latest); }
                Ok::<(), ()>(())
            }).unwrap());
            assert_eq!(*applied.borrow(), Some(latest)); assert_eq!(calls.get(), 2);
        }
    }
    #[test]
    fn completion_never_installs_after_clear_remove_or_unbounded_layout_churn() {
        let state = Cell::new(Some(0)); let calls = Cell::new(0);
        assert!(!apply_current(|| state.get(), |_| {
            state.set(None); Ok::<(), ()>(())
        }).unwrap());
        state.set(Some(0));
        assert!(!apply_current(|| state.get(), |_| {
            calls.set(calls.get() + 1); state.set(Some(calls.get())); Ok::<(), ()>(())
        }).unwrap());
        assert_eq!(calls.get(), 4);
    }
    #[test]
    fn retry_routes_missing_and_pending_views_to_create_but_existing_views_to_reload() {
        let calls = RefCell::new(Vec::new());
        for has_app in [false, false, true] {
            retry_route(has_app, || calls.borrow_mut().push("reload"), || calls.borrow_mut().push("open"));
        }
        assert_eq!(*calls.borrow(), vec!["open", "open", "reload"]);
    }

    #[test]
    fn outer_dispatch_error_cannot_overwrite_reentrant_completion_or_replacement() {
        let mut gate = CreationGate::default(); let first = begin(&mut gate, "A");
        assert!(gate.commit("A", &first));
        assert!(dispatch_outcome(Err("old HRESULT"), || gate.fail("A", &first)).is_ok());
        gate.remove("A"); let failed = begin(&mut gate, "A");
        assert!(gate.fail("A", &failed)); let retry = begin(&mut gate, "A");
        assert!(dispatch_outcome(Err("old HRESULT"), || gate.fail("A", &failed)).is_ok());
        assert!(gate.pending("A", &retry));
        assert_eq!(dispatch_outcome(Err("current HRESULT"), || gate.fail("A", &retry)), Err("current HRESULT"));
    }

    #[test]
    fn final_host_owner_retires_even_while_callback_state_is_retained() {
        #[derive(Clone)]
        struct Host { owners: HostOwners, state: Rc<RefCell<CreationGate>> }
        impl Drop for Host {
            fn drop(&mut self) { if self.owners.is_last() { self.state.borrow_mut().close(); } }
        }
        let state = Rc::new(RefCell::new(CreationGate::default()));
        let lease = begin(&mut state.borrow_mut(), "A");
        let host = Host { owners: HostOwners::default(), state: state.clone() };
        let clone = host.clone(); let callback_state = state.clone();
        drop(host); assert!(lease.is_live()); // ordinary host clone still owns the state.
        drop(clone); assert!(!lease.is_live());
        assert!(!callback_state.borrow_mut().commit("A", &lease));
        assert!(callback_state.borrow_mut().reserve("B").is_err());
    }

}
