//! Test-only executable specification. NOT an updater, native-success proof, or persisted format.
//! No production module imports this model and it cannot activate or recover an extension.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Generation { identity: u8, signing_id: u8, fingerprint: [u8; 32] }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase { Clean, Prepared, NativeIntent, NativeConfirmed, RollbackIntent, RollbackConfirmed, Committed, EnableIntent }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Authority {
    profile: u64,
    epoch: u64,
    selected: Generation,
    native: Generation,
    enabled: bool,
    phase: Phase,
    candidate: Option<Generation>,
    previous: Option<Generation>,
    authorization: Option<Consent>,
    native_enabled: bool,
    enable_review: Option<EnableConsent>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Consent { profile: u64, epoch: u64, old: Generation, new: Generation, authorize_both: bool }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Startup { Clean, OfflineMetadataOnly, ExtensionsOff }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EnableConsent { profile: u64, epoch: u64, selected: Generation }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Activation { EnableConfirmedNative, AddSelectedGeneration }

impl Authority {
    fn check(&self, consent: Consent) -> Result<Generation, &'static str> {
        if !self.valid_snapshot() { return Err("invalid authority snapshot"); }
        let candidate = self.candidate.ok_or("no prepared candidate")?;
        if consent.profile != self.profile || consent.epoch != self.epoch || consent.old != self.selected || consent.new != candidate {
            return Err("stale or foreign review");
        }
        if !consent.authorize_both { return Err("all possible native generations require authorization"); }
        Ok(candidate)
    }

    fn prepare(mut self, candidate: Generation) -> Result<Self, &'static str> {
        if !self.valid_snapshot() { return Err("invalid authority snapshot"); }
        if !matches!(self.phase, Phase::Clean | Phase::Committed) || self.selected != self.native {
            return Err("pending transaction or unactivated selection");
        }
        if candidate == self.selected { return Err("no new immutable generation"); }
        if candidate.signing_id != self.selected.signing_id { return Err("different signer cannot replace this stable identity"); }
        self.epoch = self.epoch.checked_add(1).ok_or("epoch exhausted")?;
        self.phase = Phase::Prepared; self.previous = Some(self.selected);
        self.authorization = None; self.candidate = Some(candidate); Ok(self)
    }

    fn cancel_prepared(mut self) -> Result<Self, &'static str> {
        if !self.valid_snapshot() || self.phase != Phase::Prepared { return Err("cancellation cannot cross a native-intent boundary"); }
        self.epoch = self.epoch.checked_add(1).ok_or("epoch exhausted")?;
        self.phase = Phase::Clean; self.candidate = None; self.previous = None;
        self.authorization = None; Ok(self)
    }

    fn intent(mut self, consent: Consent) -> Result<Self, &'static str> {
        if !self.valid_snapshot() { return Err("invalid authority snapshot"); }
        self.check(consent)?;
        if self.phase != Phase::Prepared || !self.enabled { return Err("native Add not permitted"); }
        self.authorization = Some(consent); self.phase = Phase::NativeIntent; Ok(self)
    }

    fn confirm_native(mut self, expected_epoch: u64, actual: Generation) -> Result<Self, &'static str> {
        if !self.valid_snapshot() { return Err("invalid authority snapshot"); }
        if expected_epoch != self.epoch || self.phase != Phase::NativeIntent || self.candidate != Some(actual) {
            return Err("native completion does not belong to this exact intent");
        }
        self.native = actual; self.phase = Phase::NativeConfirmed; Ok(self)
    }

    fn commit(mut self) -> Result<Self, &'static str> {
        if !self.valid_snapshot() { return Err("invalid authority snapshot"); }
        if self.phase != Phase::NativeConfirmed || self.candidate != Some(self.native) { return Err("native success not established"); }
        self.epoch = self.epoch.checked_add(1).ok_or("epoch exhausted")?;
        self.selected = self.native; self.phase = Phase::Committed; Ok(self)
    }

    fn select_disabled(mut self, consent: Consent) -> Result<Self, &'static str> {
        if !self.valid_snapshot() { return Err("invalid authority snapshot"); }
        let next = self.check(consent)?;
        if self.enabled || self.phase != Phase::Prepared { return Err("not disabled prepared state"); }
        self.epoch = self.epoch.checked_add(1).ok_or("epoch exhausted")?;
        self.authorization = Some(consent);
        self.selected = next; self.phase = Phase::Committed; Ok(self)
    }

    fn begin_enable(mut self, review: EnableConsent) -> Result<(Self, Activation), &'static str> {
        if !self.valid_snapshot() || self.enabled || !matches!(self.phase, Phase::Clean | Phase::Committed) { return Err("activation is not admitted"); }
        if review.profile != self.profile || review.epoch != self.epoch || review.selected != self.selected { return Err("stale or foreign Enable consent"); }
        let action = if self.selected == self.native { Activation::EnableConfirmedNative } else { Activation::AddSelectedGeneration };
        self.epoch = self.epoch.checked_add(1).ok_or("epoch exhausted")?;
        self.phase = Phase::EnableIntent; self.enabled = true;
        self.previous = Some(self.native); self.candidate = Some(self.selected);
        self.authorization = None; self.enable_review = Some(review); Ok((self, action))
    }

    fn confirm_enable(mut self, epoch: u64, actual: Generation) -> Result<Self, &'static str> {
        if !self.valid_snapshot() || self.phase != Phase::EnableIntent || epoch != self.epoch || actual != self.selected { return Err("wrong activation completion"); }
        self.epoch = self.epoch.checked_add(1).ok_or("epoch exhausted")?;
        self.native = actual; self.native_enabled = true; self.phase = Phase::Clean;
        self.previous = None; self.candidate = None; self.authorization = None; self.enable_review = None; Ok(self)
    }

    fn rollback_intent(mut self, consent: Consent) -> Result<Self, &'static str> {
        if !self.valid_snapshot() { return Err("invalid authority snapshot"); }
        self.check(consent)?;
        if !matches!(self.phase, Phase::NativeIntent | Phase::NativeConfirmed) { return Err("cannot roll back a committed generation or unstarted preparation"); }
        self.phase = Phase::RollbackIntent; Ok(self)
    }

    fn confirm_rollback(mut self, expected_epoch: u64, actual: Generation) -> Result<Self, &'static str> {
        if !self.valid_snapshot() { return Err("invalid authority snapshot"); }
        if expected_epoch != self.epoch || self.phase != Phase::RollbackIntent || actual != self.selected { return Err("wrong rollback completion"); }
        self.epoch = self.epoch.checked_add(1).ok_or("epoch exhausted")?;
        self.native = actual; self.phase = Phase::RollbackConfirmed; Ok(self)
    }

    fn startup(&self, fingerprints_valid: bool, snapshot_epoch_consistent: bool) -> Startup {
        if !fingerprints_valid || !snapshot_epoch_consistent || !self.valid_snapshot() { return Startup::ExtensionsOff; }
        match self.phase {
            Phase::Clean | Phase::Committed => Startup::Clean,
            Phase::Prepared | Phase::RollbackConfirmed => Startup::OfflineMetadataOnly,
            Phase::NativeIntent | Phase::NativeConfirmed | Phase::RollbackIntent | Phase::EnableIntent => Startup::ExtensionsOff,
        }
    }

    fn valid_snapshot(&self) -> bool {
        if self.selected.signing_id != self.native.signing_id { return false; }
        if self.phase == Phase::EnableIntent {
            return self.enabled && !self.native_enabled && self.previous == Some(self.native) && self.candidate == Some(self.selected) && self.authorization.is_none()
                && self.enable_review.is_some_and(|review| review.profile == self.profile && Some(review.epoch) == self.epoch.checked_sub(1) && review.selected == self.selected);
        }
        if self.enabled != self.native_enabled || self.enable_review.is_some() { return false; }
        if self.phase == Phase::Clean {
            return self.selected == self.native && self.candidate.is_none() && self.previous.is_none() && self.authorization.is_none();
        }
        let (Some(old), Some(new)) = (self.previous, self.candidate) else { return false; };
        if old == new || old.signing_id != new.signing_id || old.signing_id != self.selected.signing_id { return false; }
        if self.phase == Phase::Prepared {
            return self.selected == old && self.native == old && self.authorization.is_none();
        }
        let Some(review) = self.authorization else { return false; };
        let review_epoch = if matches!(self.phase, Phase::Committed | Phase::RollbackConfirmed) { self.epoch.checked_sub(1) } else { Some(self.epoch) };
        if review.profile != self.profile || Some(review.epoch) != review_epoch || review.old != old || review.new != new || !review.authorize_both { return false; }
        match self.phase {
            Phase::NativeIntent => self.enabled && self.selected == old && self.native == old,
            Phase::NativeConfirmed => self.enabled && self.selected == old && self.native == new,
            Phase::RollbackIntent => self.enabled && self.selected == old && (self.native == old || self.native == new),
            Phase::RollbackConfirmed => self.enabled && self.selected == old && self.native == old,
            Phase::Committed => self.selected == new && (self.native == new || (!self.enabled && self.native == old)),
            Phase::Clean | Phase::Prepared | Phase::EnableIntent => false,
        }
    }

    fn accepts_inventory(&self, captured_epoch: u64) -> bool {
        self.valid_snapshot() && matches!(self.phase, Phase::Clean | Phase::Committed) && captured_epoch == self.epoch
    }
}

fn old() -> Generation { Generation { identity: 1, signing_id: 9, fingerprint: [1; 32] } }
fn new() -> Generation { Generation { identity: 2, signing_id: 9, fingerprint: [2; 32] } }
fn clean(enabled: bool) -> Authority { Authority { profile: 7, epoch: 10, selected: old(), native: old(), enabled, phase: Phase::Clean, candidate: None, previous: None, authorization: None, native_enabled: enabled, enable_review: None } }
fn consent(state: Authority) -> Consent { Consent { profile: state.profile, epoch: state.epoch, old: state.selected, new: state.candidate.unwrap(), authorize_both: true } }

#[test]
fn update_model_native_intent_requires_exact_profile_epoch_fingerprints_and_both_generations() {
    let state = clean(true).prepare(new()).unwrap(); let exact = consent(state);
    assert!(state.intent(exact).is_ok());
    for invalid in [Consent { profile: 8, ..exact }, Consent { epoch: 10, ..exact }, Consent { old: new(), ..exact }, Consent { new: old(), ..exact }, Consent { authorize_both: false, ..exact }, Consent { new: Generation { fingerprint: [9; 32], ..new() }, ..exact }] {
        assert!(state.intent(invalid).is_err());
    }
}

#[test]
fn update_model_preparation_cannot_claim_native_success_or_commit() {
    let prepared = clean(true).prepare(new()).unwrap();
    assert_eq!(prepared.native, old()); assert_eq!(prepared.selected, old());
    assert!(prepared.confirm_native(prepared.epoch, new()).is_err());
    assert!(prepared.commit().is_err());
    assert!(prepared.rollback_intent(consent(prepared)).is_err());
}

#[test]
fn update_model_commit_is_single_boundary_and_cleanup_failure_cannot_downgrade() {
    let prepared = clean(true).prepare(new()).unwrap(); let review = consent(prepared);
    let intended = prepared.intent(review).unwrap();
    assert!(intended.confirm_native(intended.epoch - 1, new()).is_err());
    assert!(intended.confirm_native(intended.epoch, old()).is_err());
    let confirmed = intended.confirm_native(intended.epoch, new()).unwrap();
    assert_eq!(confirmed.selected, old()); assert_eq!(confirmed.native, new());
    let committed = confirmed.commit().unwrap();
    assert_eq!(committed.selected, new());
    assert!(committed.rollback_intent(review).is_err());
    assert_eq!(committed.startup(true, true), Startup::Clean);
}

#[test]
fn update_model_only_precommit_exact_rollback_can_confirm_old_code() {
    let prepared = clean(true).prepare(new()).unwrap(); let review = consent(prepared);
    let intended = prepared.intent(review).unwrap();
    let rollback = intended.rollback_intent(review).unwrap();
    assert_eq!(rollback.startup(true, true), Startup::ExtensionsOff);
    assert!(rollback.confirm_rollback(rollback.epoch, new()).is_err());
    assert!(rollback.confirm_rollback(rollback.epoch - 1, old()).is_err());
    let confirmed = rollback.confirm_rollback(rollback.epoch, old()).unwrap();
    assert_eq!(confirmed.startup(true, true), Startup::OfflineMetadataOnly);
    assert_eq!(confirmed.native, old());
    assert!(confirmed.epoch > rollback.epoch);
    assert!(confirmed.epoch > clean(true).epoch);
    // There is deliberately no storage snapshot/restore claim in this code-state model.
}

#[test]
fn update_model_disabled_selection_keeps_old_native_folder_and_requires_add_on_enable() {
    let prepared = clean(false).prepare(new()).unwrap(); let review = consent(prepared);
    assert!(prepared.intent(review).is_err());
    let selected = prepared.select_disabled(review).unwrap();
    assert_eq!(selected.selected, new()); assert_eq!(selected.native, old());
    assert!(!selected.enabled);
    let review = EnableConsent { profile: selected.profile, epoch: selected.epoch, selected: new() };
    let (intent, action) = selected.begin_enable(review).unwrap();
    assert_eq!(action, Activation::AddSelectedGeneration);
    assert_eq!(intent.startup(true, true), Startup::ExtensionsOff);
    assert!(intent.begin_enable(review).is_err());
    assert!(intent.confirm_enable(intent.epoch, old()).is_err());
    let active = intent.confirm_enable(intent.epoch, new()).unwrap();
    assert_eq!(active.native, new()); assert!(active.enabled);
    assert_eq!(active.startup(true, true), Startup::Clean);
    assert!(selected.prepare(Generation { identity: 3, signing_id: 9, fingerprint: [3; 32] }).is_err());
}

#[test]
fn update_model_every_uncertain_native_crash_and_invalid_authority_refuses_startup() {
    let prepared = clean(true).prepare(new()).unwrap(); let review = consent(prepared);
    let intent = prepared.intent(review).unwrap();
    let confirmed = intent.confirm_native(intent.epoch, new()).unwrap();
    let rollback = confirmed.rollback_intent(review).unwrap();
    let restored = rollback.confirm_rollback(rollback.epoch, old()).unwrap();
    let committed = confirmed.commit().unwrap();
    for state in [clean(true), prepared, intent, confirmed, rollback, restored, committed] {
        assert_eq!(state.startup(false, true), Startup::ExtensionsOff);
        assert_eq!(state.startup(true, false), Startup::ExtensionsOff);
    }
    assert_eq!(intent.startup(true, true), Startup::ExtensionsOff);
    assert_eq!(rollback.startup(true, true), Startup::ExtensionsOff);
    assert_eq!(confirmed.startup(true, true), Startup::ExtensionsOff);
    for state in [prepared, restored] { assert_eq!(state.startup(true, true), Startup::OfflineMetadataOnly); }
}

#[test]
fn update_model_old_inventory_cannot_be_relabelled_with_new_same_identity_epoch() {
    let before = clean(true); let prepared = before.prepare(new()).unwrap();
    assert!(!prepared.accepts_inventory(before.epoch));
    assert!(!prepared.accepts_inventory(prepared.epoch));
    let intent = prepared.intent(consent(prepared)).unwrap();
    let committed = intent.confirm_native(intent.epoch, new()).unwrap().commit().unwrap();
    assert!(!committed.accepts_inventory(prepared.epoch));
    assert!(committed.accepts_inventory(committed.epoch));
    assert!(Authority { epoch: u64::MAX, ..before }.prepare(new()).is_err());
}

#[test]
fn update_model_same_signing_identity_is_required_and_impossible_snapshots_fail_closed() {
    let initial = clean(true);
    assert_eq!(old().signing_id, new().signing_id);
    assert_ne!(old().identity, new().identity);
    assert!(initial.prepare(Generation { signing_id: 8, ..new() }).is_err());
    for invalid in [Authority { native: new(), ..initial }, Authority { candidate: Some(new()), ..initial }, Authority { native_enabled: false, ..initial }] {
        assert_eq!(invalid.startup(true, true), Startup::ExtensionsOff);
    }
    let prepared = initial.prepare(new()).unwrap(); let review = consent(prepared);
    let intent = prepared.intent(review).unwrap();
    let committed = intent.confirm_native(intent.epoch, new()).unwrap().commit().unwrap();
    for invalid in [Authority { native: old(), ..committed }, Authority { authorization: None, ..committed }, Authority { epoch: committed.epoch + 1, ..committed }] {
        assert_eq!(invalid.startup(true, true), Startup::ExtensionsOff);
    }
    let disabled = clean(false).prepare(new()).unwrap();
    let selected = disabled.select_disabled(consent(disabled)).unwrap();
    assert_eq!(selected.startup(true, true), Startup::Clean);
    assert_eq!(Authority { native_enabled: true, ..selected }.startup(true, true), Startup::ExtensionsOff);
}

#[test]
fn update_model_enable_requires_new_exact_review_and_rejects_uncertain_phase() {
    let disabled = clean(false);
    let review = EnableConsent { profile: disabled.profile, epoch: disabled.epoch, selected: disabled.selected };
    let (intent, action) = disabled.begin_enable(review).unwrap();
    assert_eq!(action, Activation::EnableConfirmedNative);
    assert!(disabled.begin_enable(EnableConsent { epoch: review.epoch + 1, ..review }).is_err());
    assert!(disabled.begin_enable(EnableConsent { profile: 9, ..review }).is_err());
    assert!(disabled.begin_enable(EnableConsent { selected: new(), ..review }).is_err());
    assert!(intent.confirm_enable(intent.epoch - 1, old()).is_err());
    assert!(intent.begin_enable(review).is_err());
    let prepared = clean(true).prepare(new()).unwrap();
    let uncertain = prepared.intent(consent(prepared)).unwrap();
    assert!(uncertain.begin_enable(review).is_err());
}

#[test]
fn update_model_invalid_snapshots_cannot_cross_any_native_or_commit_boundary() {
    let prepared = clean(true).prepare(new()).unwrap(); let review = consent(prepared);
    let invalid_prepared = Authority { native_enabled: false, ..prepared };
    assert!(invalid_prepared.intent(review).is_err());
    assert!(invalid_prepared.select_disabled(review).is_err());
    assert!(Authority { native: new(), ..clean(true) }.prepare(new()).is_err());
    let intent = prepared.intent(review).unwrap();
    let invalid_intent = Authority { authorization: None, ..intent };
    assert!(invalid_intent.confirm_native(intent.epoch, new()).is_err());
    assert!(invalid_intent.rollback_intent(review).is_err());
    let confirmed = intent.confirm_native(intent.epoch, new()).unwrap();
    assert!(Authority { native: old(), ..confirmed }.commit().is_err());
    let rollback = confirmed.rollback_intent(review).unwrap();
    assert!(Authority { native_enabled: false, ..rollback }.confirm_rollback(rollback.epoch, old()).is_err());
}

#[test]
fn update_model_prepared_cancel_preserves_native_and_invalidates_old_review_without_rollback() {
    let initial = clean(true); let prepared = initial.prepare(new()).unwrap(); let review = consent(prepared);
    let cancelled = prepared.cancel_prepared().unwrap();
    assert_eq!(cancelled.selected, initial.selected); assert_eq!(cancelled.native, initial.native);
    assert_eq!(cancelled.startup(true, true), Startup::Clean);
    assert!(cancelled.epoch > prepared.epoch); assert!(cancelled.intent(review).is_err());
    let intent = prepared.intent(review).unwrap();
    assert!(intent.cancel_prepared().is_err());
    assert!(intent.confirm_native(intent.epoch, new()).unwrap().cancel_prepared().is_err());
    assert!(Authority { native_enabled: false, ..prepared }.cancel_prepared().is_err());
}

/// Abstract exact declarations only, not a manifest parser or a host-pattern containment engine.
/// Real integration must supply strict canonical complete content-script objects, including
/// explicit defaults/unknown fields, and display the actual old/new declarations to the user.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Declarations {
    required_api: Vec<&'static str>,
    required_hosts: Vec<&'static str>,
    content_scripts: Vec<&'static str>,
    optional_api: Vec<&'static str>,
    optional_hosts: Vec<&'static str>,
}

#[derive(Debug, Eq, PartialEq)]
struct DeclarationChange { category: &'static str, added: Vec<&'static str>, removed: Vec<&'static str> }

fn declaration_changes(old: &Declarations, new: &Declarations) -> Vec<DeclarationChange> {
    use std::collections::BTreeSet;
    [
        ("required API", &old.required_api, &new.required_api),
        ("required hosts", &old.required_hosts, &new.required_hosts),
        ("content-script declarations", &old.content_scripts, &new.content_scripts),
        ("optional API", &old.optional_api, &new.optional_api),
        ("optional hosts", &old.optional_hosts, &new.optional_hosts),
    ].into_iter().filter_map(|(category, before, after)| {
        // Content blocks may execute in order or more than once. Preserve the complete arrays,
        // not a set that would hide a reordered or duplicated declaration.
        if category == "content-script declarations" {
            return (before != after).then(|| DeclarationChange { category, added: after.to_vec(), removed: before.to_vec() });
        }
        let before: BTreeSet<_> = before.iter().copied().collect();
        let after: BTreeSet<_> = after.iter().copied().collect();
        let added: Vec<_> = after.difference(&before).copied().collect();
        let removed: Vec<_> = before.difference(&after).copied().collect();
        (!added.is_empty() || !removed.is_empty()).then_some(DeclarationChange { category, added, removed })
    }).collect()
}

#[test]
fn update_model_optional_to_required_is_visible_even_when_union_does_not_change() {
    let old = Declarations { optional_api: vec!["tabs"], ..Default::default() };
    let new = Declarations { required_api: vec!["tabs"], ..Default::default() };
    assert_eq!(declaration_changes(&old, &new), vec![
        DeclarationChange { category: "required API", added: vec!["tabs"], removed: vec![] },
        DeclarationChange { category: "optional API", added: vec![], removed: vec!["tabs"] },
    ]);
}

#[test]
fn update_model_script_scope_changes_are_not_hidden_by_identical_matches() {
    let old = Declarations { content_scripts: vec![r#"{"matches":["https://a.test/*"],"all_frames":false,"world":"ISOLATED"}"#], ..Default::default() };
    for changed in [
        r#"{"matches":["https://a.test/*"],"all_frames":true,"world":"ISOLATED"}"#,
        r#"{"matches":["https://a.test/*"],"all_frames":false,"world":"MAIN"}"#,
        r#"{"matches":["https://a.test/*"],"all_frames":false,"match_about_blank":true,"world":"ISOLATED"}"#,
        r#"{"matches":["https://a.test/*"],"all_frames":false,"match_origin_as_fallback":true,"world":"ISOLATED"}"#,
        r#"{"matches":["https://a.test/*"],"all_frames":false,"world":"ISOLATED","future_scope_flag":true}"#,
    ] {
        let new = Declarations { content_scripts: vec![changed], ..Default::default() };
        let changes = declaration_changes(&old, &new);
        assert_eq!(changes.len(), 1); assert_eq!(changes[0].category, "content-script declarations");
        assert_eq!(changes[0].added, vec![changed]); assert_eq!(changes[0].removed, old.content_scripts);
    }
}

#[test]
fn update_model_host_changes_are_exact_categories_not_guessed_pattern_containment() {
    let old = Declarations { required_hosts: vec!["https://a.test/*"], optional_hosts: vec!["<all_urls>"], ..Default::default() };
    let new = Declarations { required_hosts: vec!["<all_urls>"], optional_hosts: vec!["https://a.test/*"], ..Default::default() };
    let changes = declaration_changes(&old, &new);
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].category, "required hosts"); assert_eq!(changes[0].added, vec!["<all_urls>"]);
    assert_eq!(changes[1].category, "optional hosts");
}

#[test]
fn update_model_script_declaration_order_and_duplicates_remain_explicit() {
    let old = Declarations { content_scripts: vec!["exact block A", "exact block B"], ..Default::default() };
    for blocks in [vec!["exact block B", "exact block A"], vec!["exact block A", "exact block B", "exact block B"]] {
        let new = Declarations { content_scripts: blocks.clone(), ..Default::default() };
        let changes = declaration_changes(&old, &new);
        assert_eq!(changes.len(), 1); assert_eq!(changes[0].added, blocks);
        assert_eq!(changes[0].removed, old.content_scripts);
    }
}
