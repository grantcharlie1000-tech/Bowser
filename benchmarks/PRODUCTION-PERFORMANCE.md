# Production-path performance baseline and bounded UI experiment

The pass-0 baseline is measurement-only. The first per-surface UI optimization is a separate experiment; no gain is established until its actual candidate and corrected matched comparison pass native verification and raw-data review. Do not use the older baseline.json as this baseline.

## Frozen inputs

- Untouched main: 80eabd7ae6d421e61c81985c34023bfc824a2e58, HTML-default release build, locked Cargo dependencies.
- External released v0.0.19: SHA-256 341486070d2e74cf9a9e5d5fbeffa90c7dd2ec4fd36bc284a8a3878a7a9bcec1.
- Candidate: measurement-only source, built separately. Run diagnostic-off versus diagnostic-on to quantify logging overhead; compare untouched main against diagnostic-off to quantify instrumentation-code overhead.
- Workflow artifact freezes all three EXEs, hashes, sizes, Cargo.lock, UI manifest, source/tree identities, toolchain and instrumentation patch. It does not publish or install a release.

## Launch contract

Use only ordinary --data-dir and --open-url, plus --diagnostic-log on one arm. The diagnostic switch requires an explicit isolated root and does not select any benchmark behavior: profile selection, session locking/persistence, privacy initialization, downloads, blocker, updater and normal HTML chrome remain production paths. Synthetic 10,000-entry history and 818-entry bookmark seeds are fixed per runner session; only the first-run import choice is pre-answered to avoid an unrelated modal. Security defaults are unchanged. LOCALAPPDATA is isolated for updater safety. No real user profile is read.

The diagnostic navigation stream carries tab identity, WebView2 navigation identity, explicit IsSuccess and a bounded fixture token, never a URL or arbitrary page title. Duplicate completion logging and failure-as-completion are removed. A fixture acknowledgement requires its unique document token, the current navigation and successful completion, in either completion/title ordering. Failed, hung, nonfixture, wrong-tab and superseded/stale streams are rejected by contract tests. Double requestAnimationFrame acknowledges document progress, not a guaranteed displayed frame. The untouched executable uses the same document's loopback acknowledgement plus owned-window title as an external readiness proxy, separately labelled.

## Pilot coverage and limits

The initial five-pair pilot alternates arm order on one pinned windows-2025 runner session. It covers fresh/reused light/demanding/redirect startup, owned-window UI Enter, one-to-five tabs, title-only tab switching proxy, foreground/minimized idle, graceful shutdown, profile disk growth and endpoint-sampled whole-process-tree resources via measurements.psm1. Released-reference runs and negative fixtures are bounded validation references rather than five-pair performance arms. Every failed sample is retained and prevents acceptance. Each sample records sampler wall time; no opaque overhead subtraction occurs.

CPU accounting uses PID plus creation identity, includes accumulated CPU when first observing a process, and discloses missing short-lived processes and post-last-sample CPU. This is not ETW-complete. Native private bytes are distinguished from WMI proxies and summed working sets count shared pages more than once. Near-zero CPU should be reported in absolute core-seconds. Runner image/version, CPU/GPU, runtime, screen, power scheme and noise are recorded. Hosted runner results are matched relative evidence, not laptop/battery promises.

This pilot is not the full audit matrix or an optimization denominator yet. Remaining before the baseline review: at least 10 valid primary pairs after pilot noise sizing; blank/start-page startup and real-site smoke; first-key before history load, steady typing, paste/IME; back/forward/restore; first/warm menus/panels; 25/100 tabs; layout transitions; 819/1,000/5,000 bookmark scaling; controller counts and background/confirmed-suspended/resumed states; validated production initialization evidence and shared runtime disk size. Missing metrics must stay missing, not be inferred from source bytes or title proxies. Astra review and parent greenlight are required before optimization.

## Focused pilot recovery

Run 39086719160 built both binaries successfully but stopped before timing when a provenance diff referenced a commit absent from the shallow candidate checkout. The complete frozen archive is artifact 12121088896, SHA-256 7067133916748c8726a74bf48a599834f82adadfebf6c0ef1c69f2afd763ccef. The recovered lane reuses those exact binaries rather than rebuilding, fetches both source identities explicitly, validates the archive and individual file digests, and refuses any non-harness code change since diagnostic source 8a60dca. Untouched main EXE is 7,793,152 bytes, SHA-256 d0075f2807fd94000cb51ace647eaf335c71ab37c8302a91f0a52d3380dbe0fa; diagnostic EXE is 7,798,272 bytes, SHA-256 2996defa9c9118309181f3ac4cda6f460f885372223aadfb4e55e4786d5e9021. Do not call either identical to released EXE.

To get an early valid focused baseline, the recovery runs a bounded one-pair validity pass then a five-pair light-fixture noise/overhead pilot, with redirect and failed/hung/error validation retained. Other fixtures remain selectable. It stops after two invalid samples instead of repeating an infrastructure failure across the matrix. This remains a pilot, not full-matrix acceptance or proof of an optimization. Disposable renderer caches are not archived; reproducible seeds, raw measurements, failures, binary inputs and provenance are retained.

The first native validity attempt (run 39087819630, artifact 12121607087) retained two invalid fresh samples and one completed released/reused sample. Startup acknowledgement and UI Enter succeeded; the extra five-tab keyboard workload lost the .NET main-window handle. Those failures are not a baseline or an optimization win. The focused pilot now enumerates the owned BowserNativeWindow directly and keeps the first resource workload at one tab. Five-tab scaling remains an explicit optional -IncludeTabFlow probe, with the earlier failure retained for diagnosis; its metric is absent from the one-tab pilot, never zero. No application behavior or browser feature is disabled.

## First per-surface UI experiment, corrected comparison contract

The immutable pass-0 pilot remains exploratory. For the first bounded UI split, the workflow
builds the actual candidate with Rust 1.99.1 and records its source/tree/EXE SHA256/size and
lockfile/asset manifest, while retaining the frozen diagnostic baseline and untouched/released
EXEs unchanged. Native instrumentation is required to remain identical for this slice.

`-CandidateExe` selects two identically logged arms, frozen `baseline_on` and actual
`candidate_on`, alternating AB/BA per pair. `-MeasurementMode timing` uses 15ms readiness
polls and does not inventory processes before startup/input timing endpoints. The launch QPC
is immediately before `Start-Process`, after profile setup/disk inventory. The separate
`resource` arm observes whole-process-tree private bytes/CPU and sampled peaks, with readiness
checked before its expensive inventory. Neither arm subtracts diagnostic or sampling overhead.
The workflow requires 10 matched pairs per fresh/reused condition for each arm, plus a corrected
untouched/diagnostic-off/on control pass. All failures and outliers remain in the artifact.

Root shutdown and owned/profile helper-tree drain are separate endpoints. A subsequent launch
is forbidden unless the previous tree has exited, including identities recorded before close;
profile files remain for the reused condition. A timeout invalidates/stops the run, rather than
silently converting reused launches into surviving-helper launches. Resource snapshots use the
same settled one-tab light fixture phase. Working sets include shared pages; sampled peaks are
not continuous ETW peaks; short-lived processes can escape endpoint inventories.

A fast fixture alone cannot validate a candidate: diagnostic rows must additionally contain
`html_toolbar_ready` and `html_chrome_shown`. Actual HTTP module/CSP first/warm load smoke covers
all 12 lazy surfaces in Edge; native candidate startup proves embedded custom-scheme module loading
and HTML activation. The HTTP smoke's external browser-launch/load durations are only regression
proxies, not in-app first-open interaction benchmarks. Those native first-open/warm surface
interaction latencies remain a separate acceptance check before claiming a startup win.
The title/navigation attribution is valid here only for isolated sequential single-tab fixtures
with unique tokens, independent loopback acknowledgement and owned-window checks, not arbitrary
concurrent navigation races. Reduced JS closure bytes are build evidence only, not RAM or speed
measurements; shared chunks do not share JavaScript state between WebViews.


The dedicated native validity arm optionally opens an actual HTML popup and internal Settings
page, first and warm, through owned-window keyboard input. It requires the owned visible HTML
popup class and the ready-gated expanded Settings container (not native fallback), retains
screenshots, and returns to an independently acknowledged fixture between page opens. These
activation proxies include foreground verification and are not precise interaction or cold
module timings. Popup prewarming remains normal production behavior. This probe is not enabled
in primary timing/resource arms, so additional first-use controllers do not contaminate their
settled one-tab phase.

Edge surface smoke loads the existing lazy start-page module through a valid host snapshot,
not merely the split host bootstrap. Its optional in-document timing fields are explicitly
virtual-time diagnostics because the headless smoke uses a virtual-time budget. Only its
external spawn/load endpoint is wall-clock, still dominated by a full browser launch and a
single first/warm sample per surface. It is exploratory load/behavior evidence, not native
latency or statistical no-regression proof.


Harness-only follow-ups may reuse the already-built actual optimized bc82c994 executable,
not the pre-optimization diagnostic executable, only when a strict source diff permits solely
benchmark, UI-test and this measurement-workflow changes. Both its artifact ZIP and native EXE
are hash-verified; the original candidate source/tree/identity remain unchanged and the newer
harness source is recorded separately. Any product/build-input change forces a fresh build.
The drain uses a negative nonexistent PID sentinel so Windows Idle/System ancestry is never
selected, and retains remaining process identities if exit cannot be confirmed. Native tabs/rail
activation is checked through UIAutomation rooted at the exact owned window, with named visible
HTML roles and rectangles, in the dedicated validity arm only. Provider unavailability on the
frozen baseline is a harness limitation, not evidence of a candidate regression.
