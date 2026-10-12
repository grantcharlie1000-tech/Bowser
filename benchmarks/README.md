# Browser benchmark and UI smoke

Run these scripts from Windows PowerShell 5.1 or a later PowerShell release after building the x64 release binary:

```powershell
cargo build --locked --release --target x86_64-pc-windows-msvc
& "$env:WINDIR\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File benchmarks\smoke.ps1
& "$env:WINDIR\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File benchmarks\run.ps1
```

**Data safety.** Every Bowser launch in these scripts passes `--data-dir <profile>-appdata` next to its throwaway WebView2 profile, so history, workspaces, tab groups, settings, session files, permissions, passwords and everything else Bowser persists land in a disposable folder instead of `%LOCALAPPDATA%\Bowser`. `--benchmark-user-data-dir` alone only isolates the WebView2 folder; Bowser itself refuses benchmark mode without an isolated data root and defaults to a temporary one (see the root README). Always pass `--data-dir` explicitly in new scripts.

Use `benchmarks\smoke.ps1 -IncludeWebSearch -OutputDirectory C:\path\to\new-smoke-results` to save `smoke.json` and `smoke.md` separately from existing evidence. Supply `-BowserExecutable C:\path\to\bowser.exe` to test a specific build, such as the MSVC-target output while the standard launcher binary is in use. The output directory defaults to `benchmarks\results`; supplied paths are normalized to absolute paths before reports are written.

The smoke uses a loopback HTTP server, a temporary WebView2 profile and a disposable, uniquely named HKCU protocol handler. It checks empty startup and lazy engine creation, address submission, result-link navigation and history, popup policy, geolocation denial, download cancellation, external URI cancellation (including proof that the registered handler did not run), and shutdown during a held request. It verifies Bowser's native EDIT text before pressing Enter. It tries keyboard-layout virtual keys, Windows Forms `SendKeys`, and `WM_CHAR`; if those do not populate the field, it uses and records `WM_SETTEXT` on the same native control. The protocol key is removed in `finally`; a failed process-group shutdown is recorded and no process is killed. Reports default to `benchmarks\results`; request and event logs remain in the temporary artifact directory referenced by the report. The optional `-IncludeWebSearch` gives the child process a temporary `LOCALAPPDATA` directory with no search-provider override, submits `Rust WebView2` to the default Bing provider, physically clicks a visible result using Windows UI Automation, then verifies the source URL and native address agree on the final non-Bing HTTP(S) destination with a usable page title in the same Bowser window. Provider or network errors are recorded as failures.

The benchmark runs five paired samples per workload by default. `blank-idle` samples an empty `about:blank` baseline; `blank-submit` measures launch, input readiness, verified phrase entry, and Enter, with `submitted_ms` captured immediately after key-up and no later samples; `light` and `demanding` open the local fixtures directly and measure their title markers. Input, page, idle, or memory metrics that do not apply to a workload remain blank in CSV and appear as `n/a` in the summary. `cold-profile` means a new isolated profile; `warm-profile` restarts the process with the same paired profile. Windows OS caches are not flushed, so this does not represent a true cold-OS-cache measurement. Edge and Chrome use isolated user-data directories and retain their default sandbox/security settings. Bowser's address input is read back from its native EDIT; verified SendInput, SendKeys, and WM_CHAR methods are attempted before a recorded WM_SETTEXT fallback. Chromium's focused address bar is read back with UI Automation ValuePattern (and uses ValuePattern.SetValue only as a recorded fallback). `-Runs`, `-IdleSeconds`, `-Browser`, and `-OutputDirectory` change the sample count, idle window, browser selection, and output location; `-KeepProfiles` preserves generated profiles. Otherwise profiles are removed only after every launched process group has exited. Raw samples, app event logs, host/software metadata, SHA-256 input verification and a median/range summary are written to the selected output directory. A short all-browser pilot can be run with `-Runs 1 -IdleSeconds 5` before the default 5-pair collection.

Use `-SelectedWorkloads light,demanding` to limit a run to the named local fixtures, or `-SelectedWorkloads blank-idle,blank-submit` for the two blank controls. Omit `-TrackingPrevention` for the runtime default; explicit `Balanced` and `Strict` are separate benchmark-only arms on the local light and demanding fixtures. These fixtures make no third-party requests, so policy readback and essential-content smoke results do not establish real-site request or performance savings. Keep invalid or incomplete samples in the raw output and exclude them from comparisons.

The local fixtures have no remote resources or ongoing animation. The light page contains 24 list items and a 320x180 static canvas. The demanding page creates 2,400 cards (9,600 DOM nodes) and an opaque 1,200x675 canvas (~3.09 MiB of RGBA pixel storage). Their `BOWSER_BENCH_READY_LIGHT` and `BOWSER_BENCH_READY_DEMANDING` titles are set after `DOMContentLoaded` and two animation frames.

For page usability, the benchmark records the first 10 ms polling sample where the expected title is visible and the top-level window responds to `WM_NULL`. This outer elapsed time is comparable across Bowser, Edge, and Chrome; Bowser's internal `page_usable` event remains in its diagnostic CSV. Input readiness is a focused native Bowser EDIT control or focused Chromium address bar; submission timing ends at Enter and excludes provider response. Process RAM and CPU include the launched process and its descendants, including WebView2 children. RAM/CPU are read from one `Win32_Process` inventory: working set is bytes, `PageFileUsage` is converted from KiB to private/commit bytes, and user/kernel CPU counters are converted from 100 ns units. `CreationDate` distinguishes reused PIDs and rejects invalid parent edges. The inventory is an approximate near-snapshot; one-second CPU samples can miss child processes that start and exit between samples. Bowser disk footprint counts the executable and adjacent loader DLL separately from shared installed WebView2 runtime files and disposable profiles.

When Bowser runs with benchmark logging enabled, its event CSV also records the one-time WebView2 creation phases: `webview_profile_started/ready`, `webview_environment_requested/result`, `webview_controller_requested/result`, `webview_configure_started/finished`, and `webview_create_completed/failed`. These are elapsed timestamps from the same process epoch; subtract adjacent events to get phase durations. The request/result pairs show asynchronous environment and controller delays separately from controller configuration.

## Performance regression probe (`perf-probe.ps1`)

`benchmarks\perf-probe.ps1` is a lightweight regression check for startup, multi-tab resource use and idle cost. It is not part of the smoke or the five-pair collection. It takes over the keyboard and foreground in `tabs` mode, so run it only when the desktop can be left alone:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File benchmarks\perf-probe.ps1 -BowserExecutable target\release\bowser.exe -Label before -Mode startup -Runs 9
powershell -NoProfile -ExecutionPolicy Bypass -File benchmarks\perf-probe.ps1 -BowserExecutable target\release\bowser.exe -Label before -Mode tabs -TabCounts 1,5,10,25 -MemoryPressure
powershell -NoProfile -ExecutionPolicy Bypass -File benchmarks\perf-probe.ps1 -BowserExecutable target\release\bowser.exe -Label before -Mode idle
```

- `-Label` names the output files, `-OutputDirectory` defaults to `benchmarks\results\perf-probe`, and each run writes `<label>-<mode>-<stamp>.csv`, a `.md` summary and a `-logs` folder with Bowser's event CSVs.
- The fixture server is an in-process `TcpListener` on `IPAddress.Any`, serving only `GET` for files directly inside `benchmarks\fixtures`. Traversal, encoded paths and other methods are refused. `-Port 0` (the default) picks a free port; a fixed `-Port` that cannot be bound stops the run rather than reusing a stale server. A self-check confirms the server returns the light fixture before any browser starts.
- `startup`: alternates fresh and warm profiles (the warm profile is primed by one untimed launch). Reports medians with p25 and p75 of `window_visible`, `input_ready` and `chrome_painted` from Bowser's event CSV, the outer launch-to-visible time, and host working set and private MB. `chrome_painted` is first chrome frame presented; it is `n/a` on builds without that event.
- `tabs`: opens `-TabCounts` tabs on distinct loopback hosts (`127.0.0.1` to `127.0.0.N`), so each tab gets its own renderer. It samples at settle, +40 s and +50 s and reports loaded tabs, host and WebView2 working set and private MB, and idle CPU over the final 10 s. `-MemoryPressure` passes `--benchmark-memory-pressure on`; the count of `tab_discarded` events is reported either way. The script aborts before sending Ctrl+T if Bowser is not the foreground window.
- `idle`: an empty window with the omnibox focused for 20 s, then one loaded page for 20 s, with host and WebView2 CPU over each window.

Read host and WebView2 numbers separately. **Host** is the Bowser process itself: its working set and private MB reflect the Rust shell, its native windows and its WebView2 controller bookkeeping. **WV** (WebView2) is the sum of child processes (renderers, GPU, utility) and grows with the number of loaded sites, so it is the figure to watch for per-tab cost. Working set counts resident pages, including shared pages, so it overstates per-process cost; private MB is the closer measure of memory committed to each side. Idle CPU in the 40 to 50 s window is the steady-state cost once background tabs have settled; a large host figure with a small WV figure points at the shell, and the reverse points at page or renderer work.

The script shuts Bowser down with `WM_CLOSE` and waits for the process group (the process, its descendants and any process whose command line names its profile) to exit. It never kills processes. If a group does not exit, the profile is kept and the run is marked as unclean. Profiles live under `%TEMP%` and are removed after a clean exit.

## Baseline and regression check (`baseline.json`, `compare-baseline.ps1`)

`benchmarks\baseline.json` records 12 medians from one real run of the probe on the copied release exe (`target\release\bowser.exe`, SHA-256 `1B5A4620…6DE4`, source commit `12717fc` plus uncommitted working-tree edits, so it may not match the tree exactly). Machine: Intel i5-7300HQ, 15.9 GiB, Windows 10 Home. Recorded 2026-10-10. Startup used 5 pairs, idle one run, tabs 1 and 5 tabs, tab switch 20 presses.

| Metric | Median |
| --- | --- |
| Fresh / warm window visible (ms) | 87.5 / 113.6 |
| Fresh / warm outer launch to visible (ms) | 434.1 / 394.5 |
| Idle empty private (MB) / CPU per 20 s (ms) | 2.5 / 0 |
| Idle loaded private (MB) / CPU per 20 s (ms) | 115.6 / 312.5 |
| 1 tab private (MB) / idle 10 s CPU (ms) | 113.2 / 125 |
| 5 tab private (MB) | 144.1 |
| Tab switch (ms) | 27 |

Not in the baseline: 5-tab idle CPU. The one run gave -31.2 ms because a WebView2 child exited inside the window; the reducer now omits negative sums. Startup spread is wide (fresh p25 to p75 about 107 to 232 ms in the earlier partial run), so the tolerances are loose.

`compare-baseline.ps1 -BowserExecutable <exe>` re-measures (startup, idle, tabs 1,5, switch) and compares; `-ResultsDirectory <dir>` compares an existing probe output. A metric fails when `current > baseline*(1+pct/100)+floor`: `*cpu*` 100%+50, `*_ms` 50%+20, `*_mb` 15%+5. Exit 0 = within tolerance, 1 = regression or missing metric, 2 = could not compare. Tested: unchanged results exit 0; a baseline with a tightened startup value exits 1 and names the metric; a missing baseline exits 2. `test-measurements.ps1` passes.

**Browser comparison (Bowser vs Chrome vs Edge, light fixture, `run.ps1 -Runs 3 -IdleSeconds 5`, output in `results\compare-browsers-ws7-20261010`).** Only one pair per cell is valid: the other two Bowser and Chrome/Edge pairs were marked `shutdown_unconfirmed` or otherwise invalid, so these are single samples, not medians. Single valid cold-profile samples: visible / page usable (ms), idle private (MiB): Bowser 451 / 1195, 110; Chrome 924 / 1991, 263 (idle CPU 10.6%); Edge 1281 / 2052, 253. Warm: Bowser 897 / 1856; Chrome 1336 / 2106; Edge 666 / 1464. Firefox is not installed and `run.ps1` does not support it, so it is not measured. Not measured: any demanding-fixture comparison and any real-site comparison. The harness reports `visible` on a different clock from the probe (`window_visible`), so the two sets of numbers are not interchangeable.

Known limit: `run.ps1` takes its `-FixtureRoot` and `-OutputDirectory` defaults from `$PSScriptRoot` inside `param()`, which was empty under `powershell.exe -File` here; pass `-FixtureRoot` explicitly until that is fixed. (`compare-baseline.ps1` now resolves its defaults in the body.)
