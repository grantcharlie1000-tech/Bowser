# Actual App in-modal same-URL Reload refusal probe

This is a distinct Windows-only, ignored, TEST-ONLY lane for PR #49's existing
production coordinator behind the default-OFF `experimental-engine-context-menu`
feature. It does not enable shipping features, change permissions, alter production
cancellation, or establish marketplace compatibility. The original actual-App
Cancel/Select test, its helper, fixture bytes, Copy/Paste zero-sequence refusal,
Note/Split lane, and old default-menu required failures remain unchanged.

## Independent process and consent

Use an unlocked interactive Windows desktop with no concurrent mouse/keyboard use.
The test checks its complete argv and refuses any run containing another test.
It creates a fresh ordinary profile, installs only the existing keyed
`tests/fixtures/extensions/probe-custom-context-menu` fixture, and retains its own
actual `App::run` / `BowserNativeWindow`. No initialized or real user profile,
WEBVIEW2 override, clipboard, updater, account, credential, browser termination,
release, deployment or permission change is involved.

PowerShell:

    $env:BOWSER_APP_MENU_RELOAD_PROBE_CONSENT = 'probe-custom-context-menu-reload-no-further-input'
    cargo +stable test --locked --lib --features html-chrome-default,experimental-engine-context-menu engine::app_context_menu_probe::reload::live_probe_real_app_context_menu_same_url_reload -- --exact --ignored --nocapture --test-threads=1

## Evidence and causal boundaries

- The source remains the original active ordinary controller, COM view, successful
  document proof and disposable environment. A separate same-URL negative view is
  retained. Native tab IDs are obtained only by the genuine extension worker's
  query/preflight. No URL/order/logical-ID mapping to either COM view is inferred.
- A dedicated test timer (`0xB0A8`, included in the main-window collision guard)
  is armed before the right-click. It retains a Weak to the exact original App
  lifetime plus its HWND, creating thread and process. Its state is taken out of
  TLS before native/App calls. A read-only pre-arm closure also binds the existing
  native Owner alive flag and nonreused registry token, so stale App.hwnd alone
  cannot authorize KillTimer on a same-thread reused HWND. The entire TIMERPROC
  boundary contains unwinds and latches failures. Actual KillTimer success and
  owner lifetime are required.
- A read-only test seam at the existing production before-show boundary retains
  the actual request token, owner token, request epoch, original proof, source
  view/controller, exact root HMENU and a current-request predicate. It does not
  supply a selection, fabricate a navigation or change production routing.
- A distinct helper retains the original helper's process/owner/source-input and
  fresh popup guards. Rust source preparation owns exactly one ordinary activation
  attempt and requires its actual successful return; the helper never activates.
  The helper sends exactly one right-click to the proven source control, then never
  sends another input. It
  writes a bounded unique native-visible witness for the exact request/HMENU.
- The timer corroborates that exact unique visible popup with native GUI thread,
  owner, process, class, geometry and HMENU evidence. It reads the trusted DOM
  right-click counter from the original retained source, verifies null effects,
  disarms, and rechecks original current request/proof/native popup after witness
  I/O before one ordinary `App::reload`. Final COM reads precede fresh native
  popup/GUI proof and a separate pure original-request/controller/document/token
  fence, with no more COM before Reload. It never rebinds the old request to the
  replacement document.
- Acceptance requires spontaneous destruction of that popup and capture release,
  a matching actual production Completed receipt, zero native selected-command
  setters, exactly one successful Complete and explicit menu cleanup. Helper exit
  or an expired wait is not any of these facts. Actual ContextMenuRequested native
  deregistration must later succeed exactly once for both retained source/negative
  view identities during normal App teardown.
- The existing native NavigationStarting handler supplies an independent read-only
  exact-source classification with the actual COM sender, tab, native ID, bounded
  URL-length check and equality classification. It never copies native URL strings;
  capture overflow or invalid source/length explicitly fails. Acceptance
  requires a new ID, a fresh successful proof from the same retained controller/gate
  bound to that ID, the same URL and a different document timeOrigin.
- The genuine fixture must retain its exact worker instance and creation receipt,
  emit no onClicked during a fixed quiet window, and produce fresh settled
  permission-specific denials for the same genuinely returned preflight IDs. Both
  exact source/negative effects stay null; the negative document stays unchanged.
  This is bounded denial evidence for those IDs, not an invented source-ID mapping,
  a no-activeTab matrix, host-only-fixture proof or an indefinite no-grant claim.

The probe releases its own App/Core/TLS borrows before ordinary App::reload.
The unchanged production engine.reload implementation itself holds its existing
Core RefMut across native Reload. This lane exercises that ordinary behavior;
it does not claim globally borrow-free native execution or alter that path.

## Fixed new-lane bounds and unavailable behavior

The distinct helper retains a 10-second native-popup readiness window after the
right-click. One monotonic observation starts before visibility publication and
permits **five seconds total**, rejecting late reads before success, for
spontaneous popup disappearance/capture release plus matching actual completion.
The exact-owner timer has a 25-second armed lifetime; the Rust child wait is 25
seconds (neither nested wait is claimed to be a hard wall-clock interrupt).
Fresh successful post-Reload document proof is allowed 30 seconds. The no-event
quiet window is exactly two seconds. Initial actual-App readiness is 60 seconds;
its existing-pattern overall test timer is 180 seconds. None changes an old lane's
deadline or replaces a failed prerequisite.

The current coordinator skips `Phase::Showing` in its tick; its cancellation path
invalidates a Showing request without ending TrackPopupMenu. Reload therefore
MAY NOT dismiss the modal menu. Missing spontaneous cancellation is an unavailable
or failed required gate, never a pass. There is no Escape, EndMenu, auto-selection,
SetSelectedCommandId substitution, focus repair or production cancellation fix to
rescue it. After bounded failure evidence, the helper may terminate ONLY its exact
retained original isolated single-test process handle, as in the existing helper;
it has no browser-termination authority. Failure teardown is not acceptance.

CI runs this exact lane independently after experimental compilation even if the
original positive or Copy/Paste tests fail. Its artifacts are separate under
`store-real-app-context-menu-reload`; all old steps remain required. Source-only
checks on Linux do not establish Rust/PowerShell compilation or native acceptance.
An audited later Windows run is still required.

## Bounded observation-only call journal

`reload/reload-observations.json` is best-effort diagnostic output, not a new gate
or authority. One retained CreateNew file stream writes BOM-free numeric/Boolean
snapshots of at most 1024 bytes, with at most six write attempts. No snapshot is
truncated. The fixed write sites are opened, pre-final-gates, first two observed
source-start classifications, App-returned and timer-error; the global cap also
contains repeated error paths. Only the first two native classifications/IDs are
retained. Later existing observations update bounded memory, which a return/error
snapshot may persist. `native_seen` saturates at 65; `recorded` is the existing
64-record capture count. Existing invalid/saturation flags remain independent.

`stage_bits` uses opened=1, pre-final-gates=2, native-observation=4,
App-returned=8 and timer-error=16. `call_phase` is 0 before an attempt, 1 set only
in memory directly beside App::reload after all final refusal/deadline checks,
and 2 only after App::reload returns. `engine` classifies a pure pre-final-gates
App engine observation: 0 unavailable, 1 missing, 2 present. That App borrow ends
before I/O. This checkpoint is intent before the existing final COM/native/pure
and deadline rechecks, not proof of call entry. No file I/O is inserted between
those final action fences and App::reload. Native-start/return snapshots may
persist the in-memory attempted phase. Fixed numeric `event_phase[2]` and
`event_elapsed_ms[2]` arrays retain each first-two native ID's call phase and
monotonic journal-observation time, so a later return/error snapshot cannot erase
its before/during/after-call classification. These are observation times, not
engine-provided event timestamps. A returned App call is not native Reload
or navigation success; a native-start record is not successful document proof.

The journal copies no URL text or topology and makes no extra native, script,
navigation, input or permission call. Its own SOURCE/STARTS/TLS borrows end before
writes; the sole file handle is taken out of Cell during I/O. Failures and panics
are isolated from the original verdict, and zero available I/O adds no pass or
failure. `io_failed_before_write` reports earlier known errors only. A missing,
partial, capped or broken journal is inconclusive, never evidence that Reload or
an event was absent, skipped or successful. The unchanged production Core borrow
may still surround a synchronous native callback, and synchronous disk latency
is not a hard wall-clock bound. No existing 5-second/25-second limit is extended.

The original 09e run did not observe the compound popup-destruction/menu-mode/
capture-release predicate or matching actual Complete within its fixed window.
Its false compound field does not independently prove the popup remained visible.
That required failure is preserved, not reclassified by adding this journal.
