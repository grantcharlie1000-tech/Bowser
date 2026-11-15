# Council round 2: candidate 3 triage

Status: all three independent reviews are available; finding validation and repair continue.
This document is outside the immutable council input packet and does not change candidate 3.

## Frozen identity and acceptance

- Baseline executable SHA-256: `F0203D9F8F928C82598D13E8EF279315B80C12EA46828C7F8ED0A88AC00C327E`.
- Candidate 3 executable SHA-256: `423EE65B14AB9B8E4C9EBA9860BD326477C51B81E30841F0C7629A28DA60215D`.
- Council input manifest SHA-256: `68e42732c6c7a79f7402a0ed99b568aca6957bd91678d9dbc3c1f61aa69815ee`.
- Anthropic: overall **3.0/10**, scoped stability **6.9/10**, exhaustion **NO**.
- OpenAI Sol: overall **4.0/10**, scoped stability **7.5/10**, exhaustion **NO**.
- Gemini: overall **3.0/10**, scoped stability **6.5/10**, exhaustion **NO**.
- No accepted paired end-to-end task times or RAM measurements exist. Every per-area 50% gate remains open.
- The 3,092 passing Rust tests, build, and single visible recovery sequence establish different facts; none establishes a speedup.

Review: `C:/Users/charlie/.t3/artifacts/bowser-performance-20261115/council-round-2-candidate-3/review-anthropic-opus.md`.
The reviewer rehashed all 1,133 frozen inputs and reports no mismatch. It ran no builds, tests, or native probes.
Its runtime reported `claude-opus-5-5`; provider routing and application of requested `ultrathink` effort were not independently confirmed.

The OpenAI report is `review-openai-sol.md` and the Gemini report is `review-gemini.md` beside the Anthropic report. Both rehashed all 1,133 inputs successfully. OpenAI also independently reproduced the canonical source-manifest digest and rehashed the referenced executable. Gemini did not reproduce that composite digest with its chosen serialization; it nevertheless reports no underlying file-hash mismatch. This discrepancy does not establish source drift. Runtime metadata identifies the requested Sol/xhigh and Gemini/hydrogen-xhigh sessions; independent provider execution receipts remain unavailable.

All three reject overall acceptance and support further work. The 9.2 condition and unanimous exhaustion condition are both unmet. Scores concern frozen candidate 3, not subsequent source repairs.

## Anthropic findings and parent disposition

| Finding | Parent assessment | Required next evidence or change |
| --- | --- | --- |
| C1: no paired performance or RAM data | Act. This is the principal acceptance blocker. | Repair the existing runner's executable selection and provenance, then obtain matched measurements. Preserve failed attempts and separate resource runs. |
| W1: 1.5-second start-page gate | Code confirmed, inherited. `start_ui.rs:85-99` tests process age; `split_ui.rs:496` uses it. | Establish actual time spent waiting before replacing the age test with prerequisite readiness. Check complete toolbar and configured start page together. |
| W2: only-New-tabs browser loss leaves an empty viewport | Act. Visible in the owned candidate-3 probe. App notices the loss but no safe internal start page returns until user action. | Bounded recreation of the shared environment/internal hosts, with no content navigation or POST replay; verify visible recovery without user navigation. |
| W3: renderer exits require a manual card | Behavior change confirmed against the pinned baseline; not accepted under the no-tradeoff requirement. | Determine main-frame method reliably before considering bounded automatic recovery of known GET requests. Unknown or submitted requests must not be replayed. Run native renderer-exit comparisons. |
| W4: renderer unresponsive is log-only | Code confirmed at `app.rs:5729-5735`. Retaining the document avoids losing drafts, but no visible wait/reload choice exists. | A nonmodal affordance that preserves the document when waiting; native busy-renderer and recovery validation. |
| W5: clearing probe used diagnostic interaction instrumentation | Correct limitation, already disclosed in the native evidence. | Repeat with ordinary release behavior and no `--diagnostic-interaction` before acceptance. |
| W6: recovery button appearance and keyboard/accessibility | Appearance requires refinement and native verification. The review incorrectly limits accessibility exposure to candidate 2: candidate 3 also exposed the recovery button, in addition to visible click evidence. | Verify focus, Enter/Space, theme, DPI, IME, and accessibility in the ordinary launch. |
| W7: environment-only loss notification was slower | Approximate diagnostic observation only. Different process-collection teardown times are not proof of a bug. | Keep raw kill/event identities and distinguish notification delay from visible recovery time. |
| N1: public queue exposes a narrower lease type | Confirmed release warning. Visibility narrowed to `pub(super)` after the candidate-3 freeze; the test build no longer reports this warning. | Source fix compiled; candidate 3 remains unchanged. |
| N2: DragOver coalesces in place | Existing coalescing policy remains; this is not strict global FIFO. A visible consequence is speculative. | Correct descriptions of ordering; change policy only with a concrete behavior case. |
| N3: borrowed App during a pumping engine call could abort | Two added behavioral tests reproduced borrow panics through the public `call_engine` path: a deferred browser-loss dispatch and close during the call. This does not identify the spontaneous incident's trigger. | `call_engine` now owns the existing generation-guarded checkout implementation; `with_engine` delegates to it. The engine returns only to the same live generation, and pending notices are reposted after return. All 292 selected application and related tests pass, including both new regressions. |
| N4: underscores absent from HTML tab label | Provenance and consequence unestablished. | Compare baseline and exact DOM/title before treating as a regression. |

The spontaneous browsing crash trigger remains unknown. The actual native DIB/allocation/upload failure matrix has not run.

## Corrections to the review's measurement recommendations

1. **The existing pair command cannot simply be run unchanged.** `production-baseline.ps1` requires BaselineExe, DiagnosticExe, and ReleaseExe even in CandidateExe mode. Its candidate pair selects DiagnosticExe as `baseline_on` (`:74`), and its unconditional release check (`:99-100`) pins an unrelated v0.0.19 executable. These need an explicit baseline/candidate path with exact executable and source manifests.
2. **PowerShell compatibility needs repair.** The runner uses `$IsWindows` under strict mode (`:45`), but the current shell is Windows PowerShell 5.1, where that automatic variable is absent.
3. **Capture duration is not complete sampling duration.** The approximately 34-ms BeforeQpc/AfterQpc interval surrounds only CopyFromScreen. The one-shot helper also starts PowerShell, loads System.Drawing, compiles native bindings, verifies identity, and writes artifacts. Observed complete invocations take roughly 1-2 seconds. No 35-70-ms sampling resolution has been established. Calibrate a complete persistent or other approved capture path before using it as an endpoint.
4. **Capture permission does not authorize alternative input.** The user's exception permits local disposable-test-window capture. Input remains through Computer Use. Do not introduce SendInput, SendKeys, UI Automation, synthetic window messages, or foreground manipulation through the benchmark harness without additional applicable authorization. Prepare a concrete external-UI-driver path or an exact approval request if one is necessary.
5. **The gate is relative to process creation.** It does not impose an extra 1.5 seconds on every first-use or later Ctrl+T. Only the remaining process-age interval can contribute; environment/controller work may already exceed it.

Readiness callbacks, titles, double requestAnimationFrame, operator delay, and diagnostic phase records must remain labelled separately from presented, usable task completion.

## OpenAI and Gemini disposition

- **OpenAI A1/A2/A3:** agree with the missing measurement, safe internal start-page restoration, and broader native failure/input gaps. This independently corroborates Anthropic C1/W2 and the disclosed acceptance limits. Do not infer immediate recovery from captures taken roughly 14-15 seconds after injection; those delays include operator/observation time.
- **OpenAI C1:** retain foreground/preparation and polling costs separately from app task time. Its recommendation to add input dispatch stamps still needs to respect the existing Computer Use-only input boundary. No current capture exception authorizes SendInput.
- **OpenAI C2:** confirmed inherited fixture-logger defect: `production-fixture-server.mjs:11` assigns status 200 to most request arrivals before routing; `/ready` actually returns 204, redirect 302, unknown routes 404, network-failure aborts, and hung never finishes. Arrival rows cannot establish successful completed responses. Add response finish/abort records before using these logs for sample error counts; retain arrival evidence and existing negative-fixture rejection.
- **Gemini finding 2:** substantiated source ordering gap; no native reproduction yet. A new browser identity replaces the watch's current identity; callbacks from an old, still-authoritative controller can then be rejected by identity alone. The missing distinction is an actually closed/replaced controller versus a still-current tab bound to the failed browser. A narrow engine package and parent App hook are integrated in post-freeze source: typed controller-only failure retires only the affected tab, while preserving shared-browser identity guards. Five new engine and five parent regressions are written but uncompiled/unexecuted pending the baseline build lane. Parser/byte/scope checks pass; see candidate-4-integration.md. This does not clear native acceptance or identify the spontaneous incident's trigger.
- **Gemini finding 2 corrections:** its observed PID 3884 does not prove reuse of the same environment: App deliberately requested a new environment in that probe. The Microsoft Environment5 contract independently permits replacement processes on the same environment and unspecified event order. The chrome host's independent fatal-failure callback is also not the content engine's App-notice path. Neither correction by itself disproves the content-controller ordering concern.
- **Gemini finding 3:** the private-interface warning is real and now repaired in post-freeze source; candidate 3 compiled successfully, so this was not a demonstrated compilation failure. The Atomic API warning is inherited. No newly applicable strict-warning gate was demonstrated.
- **Gemini finding 4, multi-window atomics:** unsupported as a production multi-window regression. `profile::spawn_window_inner` and `spawn_window_with_urls` create a separate process with `--new-window`; process-global atomics are therefore not shared between these windows. The claim also overlooks `ready()` becoming true by age on subsequent paints. The raw-HWND sleeper and fixed gate remain reviewable lifecycle/measurement candidates, but a same-process second-window stall has not been established.
- **Gemini finding 4, metadata checks:** potential bounded synchronous host work, not a measured dominant bottleneck. Do not reduce freshness guarantees merely to remove checks.
- **Gemini finding 5:** broader native acceptance and spontaneous incident diagnosis remain outstanding. A new full JavaScript test run is not required solely because candidate 3 reuses byte-identical already-tested UI/assets. Native Rust/UI boundary validation remains distinct and necessary.
- **Gemini finding 6:** no documented universal module-size bound establishes a defect. Keep concrete borrow/lifecycle fixes scoped; a broad file-size refactor has no demonstrated performance benefit.

Primary API source checked by the parent: [Microsoft ICoreWebView2Environment5](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2environment5#add_browserprocessexited). It explicitly allows a replacement browser before a prior exit handler runs and does not promise the ordering of environment and per-view failure notifications.

## Current work sequence

1. Finish independent council review triage; verify source claims before acting.
2. Make the smallest runnable paired measurement path and validate its contracts without launching unrelated applications.
3. Collect an initial ordinary startup pair and resource pair in the disposable lane; use that pilot to validate workload identity, endpoint resolution, failures, and artifact provenance before scaling.
4. Address measured critical-path work and the remaining visible recovery gaps. New product changes require a new candidate identity; keep baseline and candidate 3 immutable.
5. Repeat native acceptance, paired per-area measurement, and independent council review. Missing evidence is not exhaustion, and no current council stop condition has been met.
