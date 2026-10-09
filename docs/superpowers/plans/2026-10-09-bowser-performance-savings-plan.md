# Bowser performance savings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` to execute task-by-task. Parent coordinates; every implementation/review agent must be a Luna subagent. Keep task file ownership disjoint and honor the dependency order below.

**Goal:** Measure Bowser's current performance with an immutable release baseline, then safely test tracking prevention and minimized-window memory targeting without weakening security or changing the production profile.

**Architecture:** First capture a reversible source/build/benchmark snapshot outside the repository and run it before any product edits. Then harden the benchmark harness, implement the WebView2 policy and memory-target boundary, wire minimized/restored state through App and Win32, and add disposable-profile smoke coverage. Finally run paired measurements and publish a qualified report; each candidate is evaluated separately and may fail to meet the stated targets.

**Tech Stack:** Rust 2024, Windows x64 MSVC, Win32, `windows` 0.62, `webview2-com` 0.39.1, PowerShell 5.1 or later, WebView2 Evergreen.

**Spec:** `docs/superpowers/specs/2026-10-09-bowser-performance-savings-design.md`

## Global Constraints

- Preserve the empty native startup and create WebView2 only after first user navigation.
- Do not preload WebView2, pass browser security flags, disable sandbox/site isolation/security updates, use `TrySuspend`/`Resume`, or add a custom blocklist.
- Do not modify the production `%LOCALAPPDATA%\Bowser\WebView2` profile or force Strict tracking prevention in normal launches.
- Reject benchmark policy mode when its profile path resolves to the production profile, comparing canonical Windows paths case-insensitively and resolving existing path aliases/reparse paths before comparison.
- Use separate disposable, app-owned profile roots for Balanced and Strict; remove them only after confirmed WebView2 process-group exit.
- Low memory target applies only while minimized; Normal is applied on a real restore transition and engine-ready reconciliation. Ordinary resize messages update bounds only.
- If `_19` is unsupported or a setter fails, keep browsing usable, record the capability/error, retain desired state, and never claim an unverified effective target.
- Treat phase-0 CSV/JSON as aggregate historical baseline only; its event CSVs lack the detailed `webview_*` events.
- Pin executable path and SHA-256; abort a comparison if binary, fixture, harness, or input hashes change.
- Validate `Win32_Process.PageFileUsage` against native process-group memory counters. Until validated, report it as a private/commit proxy only.
- Report CPU, memory, latency, and energy separately. Do not claim energy savings without calibrated direct measurement.
- The thresholds are targets, not promised outcomes: at least 60% lower page-ready elapsed time and at least 20% lower memory for a named, comparable workload/state.
- Preserve all supplied source and history. Do not create an initial whole-repository commit or a worktree that depends on a nonexistent base; do not stage or commit the whole repository.

## Review Focus

- Stale or wrong-architecture Bowser executable selected: exercise missing path, explicit path, and hash mismatch rejection in Task 2.
- Missing, duplicate, or out-of-order phase events: synthetic CSV cases in Task 2 must mark the sample invalid without inventing intervals.
- Native memory access denied or PID reused during collection: Task 2 tests source labeling, PID plus creation-time identity, and proxy fallback.
- Strict requested but not effective, or profile reused across policies: Task 3 parser tests and Task 5 disposable-profile readback/restart checks.
- Minimize/restore races during asynchronous engine creation or repeated `WM_SIZE`: Task 4 reducer and engine-ready tests verify desired/effective state, bounds ordering, and no resize-triggered retry loop.

---

## Task 1: Freeze and measure the unmodified source baseline

**Owner:** Luna A. This must finish before any product or harness source file changes.

**Files:**
- Read: `Cargo.toml`, `Cargo.lock`, `src/`, `benchmarks/run.ps1`, `benchmarks/smoke.ps1`, `benchmarks/fixtures/`, `benchmarks/README.md`, and the existing phase-0 result bundle.
- Create only outside the repository: a timestamped snapshot directory, source manifest, release binary hash record, and new baseline output directory.

**Interfaces:**
- Produces: an immutable snapshot root, `manifest.csv` with relative path/length/SHA-256, x64 release executable path/SHA-256, host/runtime metadata, and baseline rows for the unmodified source.
- No later task may edit the snapshot or overwrite `benchmarks/results/final-20261009-051814`.

- [ ] Copy the current workspace to a new external directory without touching Git or the original files. Exclude `.git`, `target`, and old `benchmarks/results`; this leaves the snapshot with source and fixtures but no candidate binary:

```powershell
$repoRoot = (Resolve-Path '.').Path
$snapshotRoot = Join-Path $env:TEMP ('Bowser-performance-baselines-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Force -Path $snapshotRoot | Out-Null
$excludedPaths = @((Join-Path $repoRoot '.git'), (Join-Path $repoRoot 'target'), (Join-Path $repoRoot 'benchmarks\results'))
robocopy $repoRoot $snapshotRoot /E /COPY:DAT /R:1 /W:1 /XD $excludedPaths
if ($LASTEXITCODE -ge 8) { throw "Baseline snapshot copy failed with robocopy exit code $LASTEXITCODE" }
```

- [ ] Verify the snapshot contains `Cargo.toml`, `Cargo.lock`, `src/`, `benchmarks/run.ps1`, `benchmarks/fixtures/`, and `benchmarks/README.md`, and contains no `.git`, `target`, profile, or result directory.
- [ ] Hash every copied input and record the existing phase-0 metadata/CSV/report hashes in the external manifest.

```powershell
$sourceManifest = Get-ChildItem -LiteralPath $snapshotRoot -File -Recurse | ForEach-Object {
    [pscustomobject]@{
        relative_path = $_.FullName.Substring($snapshotRoot.Length).TrimStart('\')
        length = $_.Length
        sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}
$sourceManifest | Export-Csv -LiteralPath (Join-Path $snapshotRoot 'source-manifest.csv') -NoTypeInformation -Encoding UTF8
```

- [ ] Store the baseline executable's resolved path, length, SHA-256, OS/runtime versions, and `Cargo.toml`/`Cargo.lock` hashes in `baseline-executable.json` next to the external manifest.
- [ ] From the snapshot root, build the x64 release binary into its own `target` directory:

```powershell
Set-Location $snapshotRoot
cargo build --locked --release --target x86_64-pc-windows-msvc
$baselineExe = Join-Path $snapshotRoot 'target\x86_64-pc-windows-msvc\release\bowser.exe'
Get-FileHash -LiteralPath $baselineExe -Algorithm SHA256
```

- [ ] Verify that `$snapshotRoot\target\release\bowser.exe` does not exist and record the target-specific binary's full path, length, and SHA-256 before launching it.
- [ ] Run five Bowser-only samples per existing workload with a 30-second idle window into a new baseline directory. The clean snapshot ensures the current harness finds only the x64 target output:

```powershell
& "$env:WINDIR\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File (Join-Path $snapshotRoot 'benchmarks\run.ps1') -Browser Bowser -Runs 5 -IdleSeconds 30 -OutputDirectory (Join-Path $snapshotRoot 'baseline-results')
```

- [ ] Hash the executable, fixtures, and harness again. Mark baseline invalid if any input changed, a sample failed, or a process group did not close. Preserve output and profiles after an unconfirmed shutdown.
- [ ] Send the parent the snapshot path, source/binary hashes, baseline run location, and any host/runtime limitation. Do not start Task 2 before the baseline is saved.

## Task 2: Pin the executable and add phase/native-memory analysis

**Owner:** Luna B. Depends on Task 1. Owns only the benchmark measurement files.

**Files:**
- Create: `benchmarks/measurements.psm1`
- Create: `benchmarks/test-measurements.ps1`
- Modify: `benchmarks/run.ps1`

**Interfaces:**
- `Resolve-BowserExecutable([string] $Path) -> resolved absolute executable path`; rejects a missing file and never falls through to another candidate.
- `Get-BowserEventIntervals([object[]] $Rows, [long] $QpcFrequency) -> per-run interval object`; first-use endpoints must occur exactly once and be ordered; optional repeat-navigation intervals begin at `repeat_navigation_boundary` and pair with the next navigation/page-ready events.
- `Get-NativeProcessMemory([int] $ProcessId) -> { WorkingSetBytes, PrivateBytes, PageFaultCount, Source, Error }`; source is `native` or `unavailable`.
- `Get-ProcessGroupSnapshot([int] $RootPid, [string] $Profile) -> process-group totals plus WMI proxy totals, native totals, creation-time identity, process roles/command-line type fields where identifiable, per-process rows, sample time, and coverage errors.

- [ ] Create the test script with synthetic event rows covering valid first-use intervals, a repeat-navigation boundary, missing endpoint, duplicate endpoint, and reversed QPC order; assert invalid rows never produce durations and repeat rows cannot pair with the first navigation. Add path tests for explicit/missing executable and a synthetic parent/PID-reuse inventory where a child created before the current parent is excluded. Run it and confirm the new helper calls fail before implementation.
- [ ] Add native memory collection through `GetProcessMemoryInfo(PROCESS_MEMORY_COUNTERS_EX)` and a safely closed process handle. Capture private usage, working set, and page faults per PID, preserving the existing PID plus process-creation-time identity check. Keep `PageFileUsage` as a separately labeled WMI proxy; if native access fails, report unavailable rather than relabeling the proxy as native.
- [ ] Implement the pure event interval function using `qpc_ticks` and `QpcFrequency`, and return the named spec intervals: profile setup, environment creation, controller creation, controller configuration, total engine creation, first navigation, and first-page elapsed. Use app elapsed values only as a cross-check; retain outer Stopwatch values for user-visible launch timings.
- [ ] Update `run.ps1` with explicit `-BowserExecutable` defaulting only to `target\x86_64-pc-windows-msvc\release\bowser.exe`. Resolve once before the first sample; record path/hash and harness/module/fixture hashes before and after collection; abort if any differ.
- [ ] Preserve all raw per-run event rows and emit a phase CSV/report section with per-run intervals, sample count, median, min/max, and invalid-sample reason. Require events only for applicable Bowser workloads; blank-submit remains submit-only and has no page-ready endpoint.
- [ ] Add native-vs-WMI same-snapshot values, counter source, process count, creation time, role if known, sample timing, and collection coverage to process detail output. Mark group snapshots approximate and leave hard-fault/page-in metrics unavailable unless a named native source is collected.
- [ ] Extend the test script to query the current PowerShell PID and a disposable child process; verify native rows have source `native`, positive identity, and non-null available counters, and proxy fallback remains labeled `wmi_pagefileusage_proxy`. Run:

```powershell
& "$env:WINDIR\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File benchmarks\test-measurements.ps1
```

- [ ] Validate hash mismatch rejection by changing a copied test input between before/after hash calls in the self-test, without touching source fixtures or the frozen baseline.

## Task 3: Implement WebView2 policy and capability boundaries

**Owner:** Luna C. Depends on Task 2's event names and Task 1's baseline. Owns the shared WebView2/Rust API files so separate agents do not collide in `webview2.rs`.

**Files:**
- Modify: `src/performance.rs`
- Modify: `src/engine/mod.rs`
- Modify: `src/engine/webview2.rs`

**Interfaces:**
- Add `TrackingPreventionLevel { Balanced, Strict }` and `MemoryUsageTarget { Low, Normal }` in `src/engine/mod.rs`.
- Add `MemoryTargetApply { Applied, Unsupported }`; `EngineAdapter::set_memory_usage_target(MemoryUsageTarget) -> windows::core::Result<MemoryTargetApply>` returns `Err` for HRESULT failure, `Unsupported` for missing `_19`, and `Applied` only after a successful setter call.
- Add `LaunchOptions::benchmark_tracking_prevention: Option<TrackingPreventionLevel>` parsed only from `--benchmark-tracking-prevention balanced|strict`; reject it unless both `--benchmark-log` and `--benchmark-user-data-dir` are present. Before creating or writing an owner marker, reject any profile path that canonicalizes to `%LOCALAPPDATA%\Bowser\WebView2`, even if that path contains a marker. Normalize canonical Windows paths case-insensitively; resolve existing paths and reparse aliases with `std::fs::canonicalize`, or canonicalize the nearest existing parent and append the remaining components when the leaf does not yet exist. Then require the existing Bowser benchmark-profile owner marker/empty-root check. Reject duplicate/unknown values and leave normal launch default as `None`. The harness creates a unique run-owned root under its output/temp directory; the app marker records Bowser ownership of that disposable profile.
- Keep the current `WebView2Engine::begin_create(...)` signature as a compatibility wrapper that delegates to `begin_create_with_tracking_policy(..., None, ...)`. Add `begin_create_with_tracking_policy(..., tracking_prevention: Option<TrackingPreventionLevel>, ...)` so the App call site can pass benchmark policy after Task 3 builds; any requested setting is applied/read back before the completion callback and before App submits its pending URL.

- [ ] Add parser tests first: no option gives `None`; balanced and strict parse exactly with an owner-marked disposable profile; missing/invalid/duplicate values fail; policy without benchmark logging, without `--benchmark-user-data-dir`, or with a production-profile alias fails; path comparison treats Windows case variants and canonical aliases as equivalent; a non-owned nonempty profile is rejected; production argument parsing never sets a policy. Confirm the tests fail.
- [ ] Add `MemoryUsageTarget`/`MemoryTargetApply` and the adapter method, with default implementations only if required to keep test doubles source-compatible. Do not put COM interface types in App or Win32.
- [ ] In `webview2.rs`, query `ICoreWebView2_13::Profile()` to obtain the profile and attempt `ICoreWebView2Profile3` only when a benchmark policy was requested. Set the level before first navigation, read it back, and emit requested/effective/enabled/unsupported/error diagnostic event names accepted by `PerfLog` (`[a-z_]` only). Do not call the setter for normal launches or the production profile.
- [ ] Preserve the existing environment tracking-prevention option. Record its configured state; do not silently enable/disable the feature to make Strict appear effective. Missing profile interface, disabled feature, setter failure, or readback mismatch must be reported as unsupported/invalid for that policy sample while creation and browsing continue on the runtime default.
- [ ] In `WebView2Engine`, cache `_19` query outcome once. Map Low/Normal to `COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW/NORMAL`; return `Unsupported` when the interface is absent, and propagate HRESULT failures. Do not use controller visibility, `TrySuspend`, or `Resume` as a fallback.
- [ ] Add unit-testable mapping/event helpers to cover enum values and unsupported/error distinctions without pretending a COM mock proves Evergreen runtime behavior. Run the focused parser/engine tests and then:

```powershell
cargo test --locked
cargo build --locked --release --target x86_64-pc-windows-msvc
```

## Task 4: Wire minimized/restored state through App and Win32

**Owner:** Luna D. Depends on Task 3's adapter interfaces. Owns only App and native window lifecycle files.

**Files:**
- Modify: `src/app.rs`
- Modify: `src/win32/window.rs`

**Interfaces:**
- App tracks `minimized`, `desired_memory_target`, last successfully accepted target (or unknown), and last valid nonzero viewport bounds.
- Add a pure `WindowMemoryTransition` reducer that consumes prior minimized state plus the new `WM_SIZE` state and returns whether to resize bounds and whether a target setter may be attempted. Initial/restored is Normal; first minimize transition requests Low; duplicate minimized/restored notifications do not retry; restore transition requests Normal; ordinary resize updates bounds only.
- `App::run` retains the benchmark policy from `LaunchOptions` and passes it into engine creation. It must not change the production profile when that option is `None`.

- [ ] Add reducer tests before wiring: initial normal, first minimize, duplicate minimize, restore, duplicate restored resize, maximize after minimized, and minimize/restore/minimize rapid sequence. Assert zero-sized minimized bounds are never sent and ordinary resize does not request a target setter. Add a startup test proving ordinary production launch keeps the engine absent until explicit submit and emits no WebView2 phase event before submit. Confirm the new tests fail.
- [ ] Add App tests for engine-ready reconciliation with a recording `EngineAdapter`: read current state after async creation; resize to the latest cached nonzero bounds before applying Low/Normal; then take and navigate the pending URL. Verify a resize from bounds A to B while creation is pending yields `resize(B)` before target and navigation. Verify minimize/restore while pending retains the latest valid bounds and chooses the current desired target (Low if still minimized, Normal after restore), not the captured initial state. Assert a second benchmark-logged navigation emits `repeat_navigation_boundary` before `navigation_started`.
- [ ] Implement minimized state in App and retain the most recent valid viewport bounds. Treat `MemoryTargetApply::Unsupported` and setter `Err` as nonfatal, log one diagnostic, retain desired state, clear accepted/known state on setter error, and continue navigation.
- [ ] Change `WM_SIZE` handling to detect actual minimized/restored transitions. While minimized, do not send zero-size bounds. On restore, set fresh nonzero bounds first, then request Normal. On every ordinary non-minimized resize, update bounds without another setter call.
- [ ] Engine-ready callback reads App's current state rather than a captured launch-time value. Apply its latest cached nonzero bounds first, then the desired target, then call `App::navigate_current` for the pending URL. Do not block page loading on unsupported/error status.
- [ ] Emit one explicit `repeat_navigation_boundary` event before a benchmark-logged second navigation on an existing engine. Keep the current per-navigation start/completed events; the boundary lets the report distinguish repeat navigation from first engine creation.
- [ ] Run focused tests and the Windows release build from Task 3's command. Do not run a benchmark yet; Task 5 adds runtime smoke checks before performance collection.

## Task 5: Add disposable-profile and lifecycle smoke coverage

**Owner:** Luna E. Depends on Tasks 3 and 4. Owns only the smoke driver and new deterministic fixtures.

**Files:**
- Modify: `benchmarks/smoke.ps1`
- Create: `benchmarks/fixtures/tracking-policy.html`
- Create: `benchmarks/fixtures/minimized-lifecycle.html`

**Interfaces:**
- Add a `-IncludePerformancePolicies` smoke switch. Each scenario launches the explicitly supplied `-BowserExecutable`, passes benchmark logging/profile arguments, captures event CSVs, and owns a unique temp profile directory.
- The policy fixture exposes essential same-origin script/image readiness and a tracking-candidate request result. The lifecycle fixture exposes a JavaScript counter, periodic loopback fetch, and muted looping local media state so minimized continuity can be checked without pausing the page.

- [ ] Add fixture assertions/content first and a smoke result case for profile isolation. Run the new smoke switch against a deliberately missing executable/profile argument and confirm it fails before touching any user profile.
- [ ] Add separate Balanced and Strict roots under the smoke run's unique temp directory. For each: launch fresh, assert the requested and effective/read-back events agree before navigation, close and confirm the entire process group exited, restart the same owned root, assert persistence, close and confirm, then delete only the verified non-reparse owned root inside the run temp directory. A profile is never shared between policies; on failed shutdown retain it and report it. Add CLI misuse cases for missing log/profile, production path case/canonical aliases, and an unowned nonempty path; verify they fail before any production-profile write.
- [ ] Verify essential first-party JS/image content for both levels. Check that the fixed tracking candidate is blocked only if the pinned runtime classifies it; if classification is inconsistent, report the candidate assertion inconclusive and keep the policy result distinct. Do not add a blocklist or browser flags.
- [ ] Minimize after the page is usable; verify Low was accepted when `_19` exists, the JavaScript counter advances, loopback request count advances, and media current time advances where local playback is supported. Restore and verify nonzero bounds, Normal accepted/read back where supported, address focus, original page, continued navigation, and immediate responsiveness.
- [ ] Exercise minimize during environment/controller creation: start from a window minimized before engine completion; verify engine-ready uses current desired Low before pending navigation. Exercise setter failure/unsupported path through the capability seam and assert browsing remains available, desired state is retained, Normal is never claimed after a failed setter, and retries do not occur for duplicate resize messages.
- [ ] Run the Windows release build and the focused smoke suite:

```powershell
cargo build --locked --release --target x86_64-pc-windows-msvc
& "$env:WINDIR\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File benchmarks\smoke.ps1 -IncludePerformancePolicies -BowserExecutable target\x86_64-pc-windows-msvc\release\bowser.exe -OutputDirectory (Join-Path $env:TEMP 'bowser-performance-smoke')
```

## Task 6: Run paired measurements and publish qualified evidence

**Owner:** Luna F. Depends on Tasks 1–5. Owns report/runbook updates and new result artifacts; does not rewrite frozen phase-0 evidence.

**Files:**
- Modify: `benchmarks/README.md`
- Modify: `docs/performance-audit-2026-10-09.md`
- Create only in a new output directory: raw CSV/JSON/phase output and follow-up summary, e.g. `benchmarks/results/performance-followup-<timestamp>/`.

**Interfaces:**
- Consumes: Task 1's frozen source baseline and hash manifest; Task 2's pinned runner/native counters; Task 3's requested/effective policy events; Task 4's state-transition events; Task 5's smoke report.
- Produces: per-workload paired comparison with source hashes, policy/profile readback, counter source/coverage, per-run intervals, compatibility outcomes, and target verdicts.

- [ ] Before any candidate measurement, verify Task 1's baseline artifact and Task 2's executable/harness/fixture hashes. Build a candidate release from the current working source into `target\x86_64-pc-windows-msvc\release\bowser.exe`, record SHA-256, and pass that exact path to the runner.
- [ ] Use the same machine, Windows build, Evergreen runtime, viewport, power mode, fixture bytes, and 30-second CPU window. Alternate baseline/candidate order for five paired runs; increase pairs if spread prevents a decision. Keep the OS cache condition explicit; a fresh browser profile is not a cold OS cache.
- [ ] Run the unmodified baseline and candidate against `blank-idle`, `blank-submit`, `light`, and `demanding` with fresh and reused profiles. Run a second local page navigation in the same process as a distinct journey; calculate `repeat_navigation_boundary -> next page_usable` separately from initial engine creation. Run All browsers only where the question is comparative; never instrument Edge/Chrome. Keep phase-0 historical data separate from the new same-source baseline.
- [ ] Compare tracking prevention one variable at a time: run matched Balanced-vs-Strict pairs for each same workload and profile condition, alternate which policy runs first across pairs, and keep fresh-profile and reused-profile strata separate. If runtime default is also measured, report it as a distinct third arm; never conflate it with explicit Balanced. Use independent roots and readback. Do not combine Strict with memory Low in the same attribution run.
- [ ] Compare memory policy one variable at a time: same workload and profile, active baseline, fixed minimized interval, post-restore readiness, and whole-group private/commit plus working set. Report native validated counters and PageFileUsage proxy separately. Record process kinds and hard faults/page-in only when a named source is available.
- [ ] Before any real-site journey, record URL, profile/policy, visible readiness milestone, separate completion condition, run date/runtime/network/cache state, and whether a controlled test account is used. For SSO or transaction-like flows, use a local controlled test service or disposable test account; do not use personal credentials, real purchases, or uncontrolled account state. Missing/ambiguous readiness excludes latency comparison but remains an incomplete compatibility outcome. Never use network-idle as a universal readiness signal.
- [ ] Report each workload separately. A 60% speed target is met only when candidate median page-ready elapsed time is at most 40% of the comparable baseline median. A 20% memory target is confirmed only with validated native process-group private/commit counters; PageFileUsage-only results are explicitly proxy results. State active/minimized, sample count, median/range, CPU separately, and energy as unmeasured unless directly measured.
- [ ] If a target is missed, uncertain, or met only for one named workload/state, report that exact outcome without extrapolating. Do not add prewarming, flags, custom blocking, or a production Strict setting as a reaction to a failed target.
- [ ] Update README/runbook and append the new evidence to the audit report, including executable hashes, method, failures, unavailable counters, compatibility regressions, and target verdict. Preserve all existing phase-0 CSV/JSON/report files byte-for-byte.
- [ ] Send the parent the result directory and report path. Leave source changes unstaged and uncommitted pending the user's integration decision; do not create an initial whole-repository commit.

## Required Windows validation commands

Run after the implementation tasks, not while preparing the plan:

```powershell
cargo test --locked
cargo build --locked --release --target x86_64-pc-windows-msvc
& "$env:WINDIR\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File benchmarks\test-measurements.ps1
& "$env:WINDIR\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File benchmarks\smoke.ps1 -IncludePerformancePolicies -BowserExecutable target\x86_64-pc-windows-msvc\release\bowser.exe
& "$env:WINDIR\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File benchmarks\run.ps1 -Browser Bowser -Runs 5 -IdleSeconds 30 -BowserExecutable target\x86_64-pc-windows-msvc\release\bowser.exe -OutputDirectory (Join-Path $env:TEMP 'bowser-performance-followup')
```

Do not start implementation until the user reviews this written plan. The method-selection question is already answered: use Luna subagents only, with the parent coordinating and enforcing the task dependencies above.
