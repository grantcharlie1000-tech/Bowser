# Bowser performance savings: measurement and bounded experiments

**Status:** Design for review; implementation has not started.

## Purpose and agreed constraints

This design turns the audit's performance targets into controlled, reviewable experiments. It measures release startup and first-page readiness with phase attribution, compares WebView2 Balanced and Strict tracking prevention using disposable profiles, and applies WebView2's Low memory target while Bowser is minimized, returning to Normal when restored.

The target is at least 60% lower page-ready elapsed time and at least 20% lower memory in the controlled workloads. These are acceptance targets to test, not promises. Memory results must separate active use from minimized use. CPU and energy remain distinct metrics; no energy saving is claimed without direct measurement.

The existing product constraints remain in force: Bowser opens an empty native window and creates WebView2 only after the user's first navigation. Keep the current production profile and its behavior. Do not preload WebView2, disable sandboxing/site isolation/security updates, pass browser security flags, suspend the WebView, stop page scripts or media, or add a custom tracking blocklist. Strict tracking prevention remains a benchmark-only experiment unless evidence justifies a later, separately reviewed product choice and a user-understandable control.

## Current architecture and evidence

`src/win32/window.rs` owns native `WM_SIZE` handling and calls `App::resize_current`. `src/app.rs` creates the engine on first navigation; its asynchronous completion stores the engine and then submits the pending URL. `src/engine/mod.rs` exposes the `EngineAdapter` interface, and `src/engine/webview2.rs` creates the environment/controller, configures navigation and safety handlers, and sets controller bounds and visibility.

Benchmark-only launch settings are parsed in `src/performance.rs`: `--benchmark-log`, `--benchmark-user-data-dir`, and `--benchmark-url`. `benchmarks/run.ps1` drives isolated runs, collects process-group CPU and memory, and records host/runtime and input hashes. `PerfLog` already writes `event,elapsed_ms,qpc_ticks`; the WebView2 source already emits profile, environment, controller, configure, navigation, and page-usable events when benchmark logging is active.

The phase-0 result bundle is a usable aggregate baseline, but it is not a detailed phase baseline. Its JSON records `target/release/bowser.exe`, SHA-256 `c1a8b654574a4df64890a39c2a5898bf06fceda7ab8ff25735cc352bc5b1a0b6`, and `inputs_unchanged=true`. The harness currently prefers that generic path whenever it exists, ahead of the target-specific executable. The historical event CSVs have the coarse events through `engine_ready` and navigation, but no `webview_*` phase rows. Therefore no missing old phase duration may be inferred from source hooks or filled in synthetically. The current harness's `private` process metric is derived from `Win32_Process.PageFileUsage`; treat it as a private/commit proxy until validated against native per-process-group memory counters.

## Architecture

### 1. Pin the measured executable and attribute startup phases

Make the Bowser executable path an explicit benchmark input, with the default pointing only to the x64 MSVC release output built for that run. Resolve the path before any launch, fail if it is absent, and include the resolved path and SHA-256 in JSON. Hash all benchmark inputs before and after collection as today; abort comparison if any hash changes. Never choose the first existing binary from multiple target paths.

At the start of a benchmark collection, build or otherwise identify one immutable release executable. Record its SHA-256 in the collection metadata and use that exact binary for all runs. Save results to a new timestamped output directory. The existing phase-0 artifact is the frozen aggregate baseline. A detailed phase-to-phase comparison is allowed only if the same older source revision is separately rebuilt and rerun with the instrumentation; otherwise present the historical aggregate baseline and the new detailed phase breakdown as different evidence classes.

Parse each Bowser event CSV after runs complete. Use event names and the app's monotonic elapsed value/QPC ticks, verify required event presence and ordering, and calculate intervals within each run before summarizing. Keep the harness Stopwatch timings as the authoritative user-visible launch metrics. Do not add instrumentation to Edge or Chrome.

Required intervals for direct fixture navigation:

| Interval | Start → end | Interpretation |
|---|---|---|
| Shell ready | `process_start` → `window_visible`; `process_start` → `input_ready` | Empty native startup and ability to submit input |
| Profile preparation | `webview_profile_started` → `webview_profile_ready` | App-owned profile directory setup |
| Environment creation | `webview_environment_requested` → `webview_environment_result` | Asynchronous WebView2 environment creation |
| Controller creation | `webview_controller_requested` → `webview_controller_result` | Asynchronous controller creation |
| Controller configuration | `webview_configure_started` → `webview_configure_finished` | Existing event-handler and controller configuration |
| Engine creation | `webview_profile_started` → `webview_create_completed` | Total WebView2 setup after the user initiates navigation |
| First navigation | `navigation_started` → `navigation_completed` and `navigation_started` → `page_usable` | Page loading and user-visible readiness |
| First-page elapsed | harness process start → `page_usable`; app `submitted` → `page_usable` | End-to-end time, with start convention clearly labeled |

`webview_environment_result` and `webview_controller_result` are callback arrival points, so their intervals include async scheduling and must not be described as pure runtime CPU time. `page_usable` is the existing deterministic fixture title marker, not a universal readiness guarantee for arbitrary websites. The report must show sample count, median, minimum/maximum, and all per-run values for each phase; with five samples it must avoid presenting precision or confidence unsupported by the sample size.

For normal navigation, preserve the current lazy path: no engine event may precede the submit/first-navigation action, and the shell-ready timings must not regress beyond noise. `blank-submit` still ends at Enter for its existing metric; it cannot also claim page-ready timing. Run the fixture workloads separately to measure first-page readiness. Add repeated navigation on the same WebView process only as a distinct repeat-navigation journey, with a new navigation ID or explicit event boundary so second-page intervals cannot be confused with initial creation.

### 2. Compare Balanced and Strict tracking prevention safely

Tracking prevention is a WebView2 profile property, and it persists in that profile's user data folder. Do not change or reuse `%LOCALAPPDATA%\Bowser\WebView2` for experiments. Add benchmark-only selection for `Balanced` and `Strict`, and use a unique, disposable `--benchmark-user-data-dir` for each policy/run pair. Use the existing profile ownership marker and cleanup discipline. Do not remove a profile until the associated WebView2 process group has exited; if shutdown is unconfirmed, retain the profile and record the failure. Use a fresh unique root for a reset.

After the controller exposes its WebView, obtain its profile and query the tracking-prevention interface (`ICoreWebView2Profile3`). Apply the requested level before the first navigation, read the level back, and log requested and effective values. Verify from the benchmark environment options and runtime behavior that tracking prevention is enabled and Balanced/Strict is effective. If the interface is unavailable, the feature is disabled, or get/set/readback fails, record the capability/error and leave the run on the runtime's existing default. Do not silently call a failed run Balanced or Strict. For the production profile, do not call the setter at all.

For each policy, pair a fresh profile and a reused profile: first run starts with a new owned directory; its paired restart uses the same directory and confirms the policy persists by reading it back before navigation. Balanced and Strict must never share a profile. Keep WebView2's other security and compatibility defaults unchanged.

The experiment should include deterministic loopback checks for a request that the pinned runtime classifies as a tracking candidate, plus essential same-origin JavaScript and image content. The test must establish that a blocked request did not reach the local server and that essential content still works. If the runtime does not classify the fixed candidate consistently, mark the tracker-blocking assertion inconclusive; do not fake a block or assume every third-party request is tracking. Do not create an app-maintained blocklist; list provenance, updates, exception handling, and false-positive ownership are outside this design.

Performance/correctness journeys should include: local light and demanding fixtures; a search-to-click flow; an ordinary article/news page; a sign-in or SSO flow; an embedded media/widget page; and a cross-origin transaction-like flow. Keep fixed local controls for deterministic regression checks. Before each dynamic-site journey, define an observable app-visible readiness milestone and a separate journey-completion condition. Examples include article body rendered with a known link usable, successful sign-in state with a controlled test account, or media reporting active playback. Do not use network-idle as a universal readiness rule. Exclude a run from readiness comparisons if its milestone is absent or ambiguous, but retain and report it as an incomplete journey outcome. Repeat real-site runs in paired order and record date, runtime version, network state, cache/profile condition, request count/bytes where available, CPU, memory, the declared readiness milestone, and journey completion. Dynamic-site evidence must be labeled with its run conditions and cannot alone support a general product claim.

### 3. Lower memory target only while minimized

Add a lifecycle signal from the native window to the current engine. On `WM_SIZE` with `SIZE_MINIMIZED`, record the app's desired memory target as Low and apply `COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW` through an available `ICoreWebView2_19::SetMemoryUsageTargetLevel`. On the transition from minimized to restored/non-minimized, restore the viewport bounds and apply `COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL`; ordinary non-minimized resize notifications update bounds without retrying the target setter. Do not set `IsVisible(false)` as a prerequisite: the API does not require it. Do not call `TrySuspend` or `Resume`; those APIs have suspension semantics and interact with memory target behavior. The Low target is a best-effort memory target; it is not a hard cap and may trade memory for disk/page faults or restore latency.

The desired state belongs to `App`, since minimize/restore can happen before asynchronous WebView creation finishes. When engine creation completes, read the current App state and apply Low before submitting any pending URL if the window is still minimized; otherwise apply/reassert Normal. This ensures startup while minimized does not briefly navigate at the wrong target. Repeated and rapid transitions apply the latest state on the UI thread. Minimize must not send zero-sized controller bounds; retain the last valid viewport bounds while minimized. On restoration, calculate and set current bounds, set Normal, then allow ordinary rendering and interaction.

If querying `_19` fails, log the unsupported capability once, keep the engine usable, and leave its runtime-default target unchanged; do not claim the Low-memory behavior was applied. If setting Low fails, log the error, retain Low as the desired state while minimized, and leave the engine usable without a suspension fallback. If setting Normal fails after restoration, log the error, keep Normal as the desired state, and treat the effective target as unknown and possibly still Low; never claim it is Normal. Keep the window interactive and retry the current desired target only at a later legitimate lifecycle boundary, such as a subsequent minimize/restore transition or engine-ready transition, without a hot loop or retry on duplicate size notifications. Target setter errors are nonfatal and must not block navigation. Do not show a modal error or fall back to `TrySuspend`/`Resume`.

## Measurement protocol and acceptance

Use one pinned host and runtime for each paired comparison. Record OS/build, CPU, RAM, WebView2 version, executable path/hash, fixture hashes, harness version/hash, policy readback, and exact profile condition. Preserve phase-0 results as the historical baseline. For a direct before/after code comparison, build the baseline revision and candidate revision from their own sources, pin both executable hashes, and alternate run order to reduce machine-load and thermal bias. Use the same fixture, profile condition, idle duration, viewport, runtime, host power mode, and sample count. Start with five paired samples per condition; report every sample and range. Increase repetitions if variance prevents deciding whether a target was met.

Measure these outcomes separately:

- First-page elapsed: launch-to-visible, launch-to-input-ready, submit-to-engine-ready, submit-to-page-usable, navigation-start-to-page-usable.
- Resource use: whole Bowser/WebView2 process-group working set and private/commit bytes at stable active-page state, peak during load, and after a fixed settling window. Record each counter's source, process count, process details and role where identifiable, and collection timing. Validate the current `Win32_Process.PageFileUsage` private/commit proxy against native per-process-group memory counters before using it to claim the 20% target. Report active and minimized readings separately. Working-set samples are approximate and summed process working sets can count shared pages more than once.
- Minimized resource use: capture an active baseline, minimize for a fixed interval, sample group working/private bytes and hard faults/page-in activity over that interval if available, restore, and record time until the page is usable/responsive again. Record the counter source; if native hard-fault/page-in counters are unavailable, mark them unavailable instead of inferring them from CPU or memory totals. Report minimized-only and time-weighted values separately; never claim active-page memory savings from minimized-only reductions.
- CPU: process-group CPU over a fixed active workload window and a separate minimized window. Keep CPU percent distinct from elapsed time.
- Compatibility: report success/failure for each journey and deterministic essential-content assertion. A blocked, broken, or incomplete journey is a regression even if it is faster or smaller.
- Energy: unmeasured unless a separate calibrated energy measurement is implemented; do not infer from CPU, memory, or elapsed time.

The 60% target means candidate median `submit->page_usable` is no more than 40% of the comparable baseline median for the same workload and profile condition. For dynamic journeys, the measured endpoint is the predeclared, observable readiness milestone; journey completion is reported separately. The 20% memory target means at least 20% lower comparable whole-process-group private/commit bytes for the explicitly named state and workload, measured with validated native counters. If only `Win32_Process.PageFileUsage` is available, label it as a proxy and report any 20% reduction as a proxy result, not a confirmed private/commit saving. Calculate changes separately for each predeclared workload; do not pool different page types or choose only the best result. State whether the result applies to active use, minimized use, or both. Report working set separately because it counts shared pages and is not interchangeable with private/commit bytes. A target is met only when paired data and its spread support the direction; if the sample distributions overlap materially or the result is workload-specific, report that as inconclusive or limited rather than claiming the threshold.

Strict policy or Low memory target must not materially reduce journey completion, break essential page content, prevent scripts/network/media from continuing while minimized, or make restore unusable. The benchmark must verify an active JavaScript timer and network activity while minimized, and playback where the chosen media test supports reliable local playback. Restore validation checks the original page, continued navigation, focus/address input, and immediately usable viewport. API support failures are recorded separately from feature effectiveness.

## Failure handling and observability

Each event interval is valid only when both endpoints occur once and in order. A missing endpoint, duplicate endpoint, failed callback, changed executable/input hash, unknown policy readback, incomplete journey, unclosed process group, or contaminated profile marks the affected sample invalid with a specific reason. Do not interpolate missing phase data. A benchmark-only logging failure should remain visible in the sample notes and must not change ordinary launch behavior.

If environment/controller creation or a policy operation is unsupported on the installed Evergreen runtime, preserve normal browsing with the current production defaults, label that capability as unsupported, and do not claim the experiment's effect. Never change security switches to make an experiment pass.

## Scope and non-goals

This design covers benchmark executable pinning and phase analysis, a benchmark-only tracking-prevention comparison, and minimized/restore memory-target lifecycle behavior. It does not redesign Bowser's UI, add a settings surface, change the production profile's stored preference, add a blocklist, preload WebView2, disable security features, suspend or pause page content, introduce an energy claim, or promise that the 60%/20% targets will be achieved. A future decision to ship Strict tracking prevention needs its own design for user choice, reset behavior, exceptions, and compatibility support.

## API references

- [WebView2 `ICoreWebView2Profile3` tracking prevention](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2profile3)
- [WebView2 `ICoreWebView2_19` memory usage target](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_19)
- [WebView2 memory target behavior](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_19?view=webview2-1.0.4129.50)
