# Candidate 4 preparation

No candidate-4 executable has been built. Candidate 3 and its council packet remain immutable.

## App engine checkout

Anthropic round-2 N3 identified a plausible borrow/reentry hazard. The private `with_engine` helper already detached the engine, released the App borrow before the operation, guarded its return by engine generation and closing state, and reposted queued failure notices. The public `call_engine` path still held App's mutable borrow throughout arbitrary engine operations, including script execution and zoom/settings queries.

The public path now owns the shared checkout implementation. The Result-typed private helper delegates to it, preserving its existing type inference and call sites. No App borrow survives the operation or retirement Close. A nested operation still finds no checked-in engine and cannot release an outer lease.

Two new fixtures use the existing pumping fake engine through `App::call_engine`: one dispatches the deferred browser-loss notice during the operation; the other closes App during it. Against candidate-3 production source, both failed with `RefCell` borrow panics. Raw log:

`C:/Users/charlie/.t3/artifacts/bowser-performance-20261115/candidate-4-app-checkout-red.log`

Command: `cargo +stable test --locked --offline --jobs 1 --lib --features html-chrome-default engine_recovery_public_call -- --test-threads=1`.
Result: **0 passed, 2 failed**, exit 101. Both failures reached the intended borrowed-state access. These fixtures model a pumping engine call; they do not reproduce the user's ordinary-browsing incident.

The post-fix application and related test selection passed **292 tests, 0 failures**, exit 0:

`cargo +stable test --locked --offline --jobs 1 --lib --features html-chrome-default app::tests -- --test-threads=1`

Raw log: `C:/Users/charlie/.t3/artifacts/bowser-performance-20261115/candidate-4-app-checkout-green.log`.
This selection includes the two new public-call regressions and the existing generation, deferred-failure, close, navigation, reader, picture-in-picture, zoom, and related application coverage. It is not a complete new library-suite or native acceptance run.

## Queue visibility

`start_ui::queue` now has `pub(super)` visibility, matching its Win32-only lease parameter and actual caller. This addresses the new private-interface warning without widening recovery authority.

## Remaining acceptance

No timing, RAM, native incident, visual, accessibility, IME, or complete fault-matrix claim follows from these changes. Candidate-3 recovery gaps and every paired per-area performance gate remain open. Source changes require a newly hashed release artifact before native or performance validation.

## Controller browser-loss authority

The replacement-browser review claim is substantiated. An old controller's browser identity can differ from the newest environment attachment while its own tab/controller lifetime is still authoritative. Ignoring that old controller's failure loses a real affected tab; forwarding it as global loss would instead retire the healthy replacement.

Integrated the three-file engine package from `handoffs/browser-loss-authority-20261115T085013Z`, after parent verification of all preimages and exact forward/reverse patch bytes. The package adds a typed `ControllerBrowserExited` notice, revokes the affected controller's token before callback dispatch, and prevents its queued configuration/navigation/POST publication. Shared-browser identity and shutdown guards remain separate.

Parent App integration:

- Scoped browser failure first passes the existing engine and controller-generation checks, then fails/closes only its owning tab with `ContentFailure::BrowserLost`. It does not retire the environment or automatically navigate.
- Scoped notices remain distinct from global-loss coalescing. They bypass the ordinary best-effort queue cap so each affected controller receives mandatory cleanup; the engine emits one per revoked token. Exact duplicates still coalesce, and genuine current-browser loss still dominates that engine generation.
- A separate `engine_controller_browser_exited` diagnostic avoids relabelling a controller-only event as a global recovery.

Five parent regressions were added for affected-only cleanup and POST cancellation/explicit GET retry, same-tab replacement before notice dispatch, 131 affected tabs beyond the ordinary 128-entry cap while a healthy tab survives, duplicate/global-loss coalescing, and stale engine/shutdown events. The package adds five engine regressions exercising production COM adapters and controller authority, including PID reuse and creation-time changes.

At package integration, all ten new regressions were uncompiled and unexecuted while the separate baseline build held the heavy lane. Five direct Rust parser checks exited 0 without changing bytes; whitespace passed. Parent compared all 840 frozen preparation inputs: only `src/app.rs`, `src/app/tests/engine_recovery.rs`, `src/engine/mod.rs`, `src/engine/webview2.rs`, and `src/engine/webview2/browser_loss_tests.rs` differed at that stage. Parser and scope evidence were not compilation/test evidence. The combined execution below supersedes that pending test status.

Evidence in the artifact root: `candidate-4-controller-parse.log`, `candidate-4-controller-integration.json`.
When the baseline lane releases and safe-start-page source integration is ready, run the actual library tests:

`cargo +stable test --locked --offline --jobs 1 --lib --features html-chrome-default engine_recovery_ -- --test-threads=1`

The child handoff suggested `--bin bowser`; parent will use `--lib`, where App/engine tests actually reside. Native same-environment replacement ordering and preserved healthy-page drafts/history remain to be verified separately. No candidate-3 executable or frozen council input changed.

## Automatic safe-host recovery

Integrated the three-file `blank-only-recovery-20261115T085027203901Z` package using the frozen preparation snapshot as the merge base. Both App files merged without conflicts over the parent's controller-only failure changes; the window file matched its expected preimage. Byte-bound base, parent, child and merged copies and the merge report are retained under `candidate-4-blank-merge-20261115T090758Z` in the artifact root.

After a shared-browser loss, the window may automatically create one replacement engine for its lifetime. Generation ownership, a 30-second creation deadline and close cancellation bound the attempt. The automatic path restores safe internal/chrome hosts and synchronizes the existing start-page model; it does not replay failed content, discarded pages or old POST bodies. A new explicit user request retains its own ownership and supersedes obsolete recovery. A second shared loss still requires explicit recovery. This budget and behavior remain subject to native and council acceptance.

Nine new production-transition fixtures cover queued/deferred recovery, lifetime-budget persistence, pending creation and timeout, stale completion, close, explicit navigation/POST ownership and teardown reentry. Together with the controller/App fixtures above and the checkout fixes, the shipping-feature full library suite passed:

`cargo +stable test --locked --offline --jobs 1 --lib --features html-chrome-default -- --test-threads=1`

**3113 passed, 0 failed, 42 ignored**, exit 0; test execution 92.58 seconds. Raw log: `C:/Users/charlie/.t3/artifacts/bowser-performance-20261115/candidate-4-rust-validation-1.log`. One shared build mutex, one build job and one test thread were used. This is combined source-test evidence; a new release build and actual automatic recovery/presentation/close checks remain necessary.
