# Glass UI baseline: Bowser at d1e165e

Pre-change measurement of Bowser at commit `d1e165e` (Document performance pass results), so later UI work can be compared against it. Only the blank-idle and blank-submit workloads produced numbers. The `light` workload failed in all 10 runs; see [Failures](#failures).

Raw data: `bowser-tools\results-baseline\` (outside the repo) — `benchmark-20261009-230518-639-5a1fe6e6.csv`/`.json`, `phase-20261009-230518-639-5a1fe6e6.csv`, and per-run `event-Bowser-*.csv`.

## Results

Bowser `0.0.1`, 5 paired runs per condition, 15 s idle window. Each cell is median (min–max), n = 5. Cold = fresh profile; warm = restart on the same profile.

| Metric | Cold | Warm |
|---|---|---|
| `window_visible` ms (blank-idle) | 166.80 (156.22–537.91) | 180.47 (157.83–188.00) |
| `window_visible` ms (blank-submit) | 170.27 (149.60–217.48) | 163.84 (156.08–263.52) |
| `input_ready` ms (blank-submit) | 292.37 (274.20–362.36) | 287.62 (279.26–378.91) |
| Typed flow: launch → Enter (`submitted`) ms | 1,609.78 (1,517.37–2,110.31) | 1,654.86 (1,564.25–1,695.90) |
| Blank idle working set MB | 9.99 (9.98–10.91) | 9.98 (9.97–10.00) |
| Blank idle private MB | 1.88 (1.86–1.91) | 1.87 (1.87–1.92) |
| Blank idle CPU over 15 s window | 0.00 % (≈0 ms; below ~0.6 ms resolution) | 0.00 % (≈0 ms) |
| Blank idle process count | 1 (root only) | 1 (root only) |
| Light page-ready ms | n/a — all runs failed | n/a — all runs failed |
| Typed flow Enter → page-ready ms | not measured (see below) | not measured |

Notes on the rows:

- **Enter → page-ready:** not measured by this harness. The blank-submit workload ends its timer at Enter by design, so there is no page-ready interval. The typed-flow number above is launch → Enter, and most of it is the scripted keystroke injection (`submission` phase in `phase-*.csv`), not Bowser.
- **Idle CPU:** the harness rounds `idle_cpu_percent` to 4 decimals, and every run reads 0.0000. Over 15 s on 4 logical processors that bounds the CPU time below about 0.6 ms, so "0 ms" is the honest reading.
- **Process count:** blank-idle has only the root process; WebView2 is not created until a page is loaded (lazy engine creation).
- **Outlier:** cold run 1 `window_visible` was 537.9 ms (the others 156–167 ms). It was the first launch after the build.
- **Medians were computed outside the harness** from the raw CSV using the same median rule. The harness summary `.md` was not written (see [Failures](#failures)).

Blank-submit valid samples: 10/10. Address readback passed on every run and Enter was pressed after verified text entry.

## Host

- CPU: Intel Core i5-7300HQ @ 2.50 GHz, 4 logical processors
- RAM: 16 GiB (17,057,812,480 bytes)
- OS: Windows 10 Home 10.0.19045 (build 19045)
- PowerShell: 5.1.19041.6456
- Rust: rustc 1.99.0 (b940084d7 2026-09-28), target `x86_64-pc-windows-msvc`, `--locked --release`
- Bowser `bowser-baseline.exe`: SHA-256 `da6fd1414e7c4d2d6069a307274069f806064a9f9c6f03b42e54f4ad0c968797`, version 0.0.1
- WebView2 Evergreen runtime versions present: 152.0.4191.66; 154.0.4258.62

## Commands

Build the baseline and copy the exe out of the repo tree (run in `C:\Users\charlie\Documents\ChatGPT\bowser-baseline`, which is a worktree at `d1e165e`):

```powershell
cargo build --locked --release --target x86_64-pc-windows-msvc
Copy-Item target\x86_64-pc-windows-msvc\release\bowser.exe C:\Users\charlie\Documents\ChatGPT\bowser-tools\bowser-baseline.exe
```

Benchmark, run from `C:\Users\charlie\Documents\ChatGPT\Bowser` under the GUI lock (`with-gui-lock.ps1` runs the inner command under `Global\BowserGuiTest`):

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File C:\Users\charlie\Documents\ChatGPT\bowser-tools\with-gui-lock.ps1 -Command "& 'C:\Users\charlie\Documents\ChatGPT\Bowser\benchmarks\run.ps1' -BowserExecutable 'C:\Users\charlie\Documents\ChatGPT\bowser-tools\bowser-baseline.exe' -Browser Bowser -Runs 5 -IdleSeconds 15 -SelectedWorkloads 'blank-idle','blank-submit','light' -OutputDirectory 'C:\Users\charlie\Documents\ChatGPT\bowser-tools\results-baseline'"
```

To repeat against another exe, change only `-BowserExecutable` and `-OutputDirectory`. Keep the other flags identical, so the workload set and sample count match.

Deviations from the README: the wrapper was called with `-ExecutionPolicy Bypass`, and `run.ps1` was invoked in-process inside the wrapper rather than through a nested `powershell -File`, because `-File` does not reliably bind the comma-separated `-SelectedWorkloads` array. Both are environment choices and do not change the measured code path.

## Failures

1. **`light` workload — all 10 runs failed.**
   - Cold 1–4 and warm 1–3 (7 runs): `BOWSER_BENCH_READY_LIGHT` title marker not observed within 120 s.
   - Cold 5, warm 4, warm 5 (3 runs): `SetForegroundWindow` failed. The foreground window was the Windows `Search` UI (`SearchApp.exe`, PID 10180, running since 02:34 today), so the harness could not activate Bowser.
   - A single manual probe with the same flags (`--benchmark-url` to `light.html`, not via the harness) also stayed on the window title `Bowser` for 8 s and wrote no event log. The probe does not prove the cause; the baseline may not load local fixtures in this environment, or the probe may have hit the same window-state issue. Light numbers need a re-run after checking the baseline's fixture load path.
   - Event logs for the light runs were not written at all.
2. **Harness summary not written.** After the CSV, JSON and phase CSV were written, `run.ps1` threw `Argument types do not match` (ArgumentException) before `benchmark-*.md` and before profile cleanup. I could not reproduce it from the saved rows with the phase summary function alone. The raw data is complete for the blank workloads; the failure stopped the summary step only. The profile folder `profiles-20261009-230518-639-5a1fe6e6` was retained.
3. **Unexplained Bowser process.** A process named `bowser-ad729864b4897fcf` (PID 70544, started 23:25:16) was observed after the benchmark ended, and it had exited by the next check. I could not inspect its path or command line, so its origin is unknown.

## Caveats and noise

- One session, one laptop on power and thermal state I did not control. Medians of n = 5 are indicative only.
- Ambient processes were not controlled: `SearchApp.exe` was running and became foreground during light runs.
- The benchmark controls keyboard and mouse. Nothing else was intended to run during collection, but the run was not checked for interference beyond the failures listed above.
- Windows OS caches were not flushed (per the README), so "cold" means a fresh profile, not a cold OS.
- `window_visible` for cold run 1 (537.9 ms) is the first launch after the build and is kept in the median and range.
- Battery use is unmeasured.

## Harness fixes and A/B method

Raw data for this section is in `bowser-tools\` (outside the repo): `ab-noise-floor-5x15-v2\` (the self-check), `diag\` (probes), and `ab-noise-floor-5x15\` (an earlier self-check that failed for harness reasons; kept as evidence).

### Root causes

1. **Light workload never became ready (fixed in the harness, not Bowser).** `bowser-baseline.exe` compiles its fixture root from `CARGO_MANIFEST_DIR` (`src/performance.rs`, `LaunchOptions::parse`). It was built in `bowser-baseline`, so it accepts `--benchmark-url` only for files under `bowser-baseline\benchmarks\fixtures`. The harness passed the Bowser checkout's fixture, and the exe showed a modal MessageBox, `Benchmark URL must be a file inside benchmarks/fixtures`, before it created its event log. A probe confirmed a `#32770` dialog owned by the process, no log file, and the same exe reporting `BOWSER_BENCH_READY_LIGHT` when given the baseline's fixture path. The modal dialog also explains the `SetForegroundWindow` failures: the harness was activating a window it could not reach. The two fixture files differ only in line endings.
2. **Summary crash (fixed in `run.ps1`).** `Get-BowserPhaseSummary -Rows @($phaseRows)` threw `Argument types do not match` on Windows PowerShell 5.1 because `$phaseRows` is a `System.Collections.Generic.List[object]`. `@(list)` throws; `$list.ToArray()` and passing the list to a typed parameter both work. This happened after the CSV and phase CSV were written, so the summary and profile cleanup were skipped. The baseline's "could not reproduce" note is resolved: the failure reproduces with an empty or non-empty generic list.

### Harness changes (`benchmarks\run.ps1`, `benchmarks\test-measurements.ps1`)

- `-FixtureRoot` parameter. Use it when the exe was built from a different worktree than the one holding the fixtures.
- `Assert-BowserFixtureAccepted`: one short launch per fixture workload before the loop. A rejected fixture now stops the run in about 5 s with the dialog text, instead of 120 s per run.
- `Assert-NoErrorDialog` in the visible and title-marker wait loops: any error dialog of the benchmarked process fails that run immediately with its text.
- `Activate-MainWindow`: up to 5 attempts. Between attempts it sends an Alt tap, which gives this process the foreground right when a shell surface such as the Search flyout holds it. I did not reproduce a SearchApp foreground on demand, so this is unverified against that case.
- The preflight waits for the main window before it sends WM_CLOSE. Without this, WM_CLOSE went out before the window existed and the probe kept running. If the probe still will not exit, the preflight stops its own process tree.
- Regression tests: `test-measurements.ps1` covers the generic-list summary input and asserts that `run.ps1` no longer wraps `$phaseRows` in `@()`.

### Corrected light baseline (baseline exe, 5 rounds, A side of the self-check)

Valid samples only; median (min–max), n valid of 5 rounds. Light was not measured in the original run. The earlier single-round light probe (n=1) is superseded.

| Metric | Cold | Warm |
|---|---|---|
| `window_visible` ms (light) | 306.83 (234.29–475.36) n=4 | 327.25 (271.86–497.91) n=4 |
| Light page ready ms (title marker) | 1,315.25 (765.51–1,902.25) n=4 | 1,309.76 (930.81–1,540.36) n=4 |
| Idle private MB (light, 10 s after ready + idle window) | 113.57 (109.69–117.33) n=4 | 116.69 (112.08–123.03) n=4 |
| Idle working set MB (light) | 253.23 (241.27–291.19) n=4 | 249.63 (247.29–301.81) n=4 |
| Idle CPU % (light) | 0.05 (0.00–0.18) n=4 | 0.01 (0.00–0.04) n=4 |
| Idle process count (light) | 7 (7–7) n=4 | 7 (7–7) n=4 |

Page-ready spread is large (766 to 1,902 ms on the same exe), so treat one light page-ready value as a rough figure until more samples exist.

### A/B method (`bowser-tools\compare-ab.ps1`)

Usage, from any directory:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File C:\Users\charlie\Documents\ChatGPT\bowser-tools\compare-ab.ps1 `
  -Baseline C:\Users\charlie\Documents\ChatGPT\bowser-tools\bowser-baseline.exe `
  -Candidate <candidate.exe> `
  -BaselineFixtureRoot C:\Users\charlie\Documents\ChatGPT\bowser-baseline\benchmarks\fixtures `
  -CandidateFixtureRoot <fixtures of the candidate's build tree; the Bowser checkout's benchmarks\fixtures for a build from there> `
  -Runs 5 -IdleSeconds 15
```

- Runs in rounds. Each round runs A then B for `blank-idle`, `blank-submit`, and `light`, under the `Global\BowserGuiTest` mutex, so other GUI runs wait.
- Each side runs once per round, with a single retry if run.ps1 throws. Each attempt has its own folder; the report lists retries and failed attempts.
- Output: `comparison.md` and `comparison.json` in `bowser-tools\ab-<timestamp>\` (or `-OutputDirectory`), plus raw run.ps1 output in `raw\round-NN-{A,B}\`.
- Flags: a startup-class change above `-StartupThresholdPercent` (default 5 %) is flagged on window visible, input ready, and light page ready. An idle private-bytes change above `-PrivateBytesThresholdMB` (default 2 MB) is flagged. Improvements are flagged separately. Overlapping A/B ranges are reported, not used to suppress flags.
- Not measured: typed-flow Enter to page-ready. `blank-submit` ends at Enter by design, and Bowser's file-URL policy means the light fixture cannot be reached by typing.
- Unit tests for the statistics: `bowser-tools\ab-stats.tests.ps1`. Harness tests: `benchmarks\test-measurements.ps1`.

Single-workload check that also exercises the preflight:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File C:\Users\charlie\Documents\ChatGPT\bowser-tools\with-gui-lock.ps1 -Command "& 'C:\Users\charlie\Documents\ChatGPT\Bowser\benchmarks\run.ps1' -BowserExecutable 'C:\Users\charlie\Documents\ChatGPT\bowser-tools\bowser-baseline.exe' -FixtureRoot 'C:\Users\charlie\Documents\ChatGPT\bowser-baseline\benchmarks\fixtures' -Browser Bowser -Runs 1 -IdleSeconds 3 -SelectedWorkloads 'light' -OutputDirectory 'C:\Users\charlie\Documents\ChatGPT\bowser-tools\diag\light-check'"
```

### Noise floor: baseline against itself, 5 rounds, 15 s idle

Both sides are `bowser-baseline.exe` (SHA-256 `da6fd141…8797`). Any flag is noise. The corrected run is `ab-noise-floor-5x15-v2`.

| Metric (workload, launch) | A median | B median | Change % | Flag |
|---|---|---|---|---|
| Window visible (blank-idle, cold) | 331.34 | 255.73 | −22.82 | improvement |
| Window visible (blank-idle, warm) | 264.85 | 278.16 | +5.02 | REGRESSION |
| Window visible (blank-submit, cold) | 377.13 | 274.20 | −27.29 | improvement |
| Window visible (light, warm) | 327.25 | 562.94 | +72.02 | REGRESSION |
| Input ready (blank-submit, cold) | 542.02 | 397.80 | −26.61 | improvement |
| Light page ready (cold) | 1,315.25 | 1,192.67 | −9.32 | improvement |
| Idle private MB (light, cold) | 113.57 | 116.60 | +2.67 (+3.03 MB) | REGRESSION |

Result: all 28 compared rows have overlapping A/B ranges, and the identical binary produces 9 threshold flags (3 REGRESSION, 6 improvement). At n = 5 on this laptop, the 5 % startup threshold and the 2 MB private-bytes threshold are below the session's run-to-run spread, mostly because of single outliers of 500 ms or more in the startup metrics. A flag from this harness is therefore a prompt to look, not a verdict. Before a decision rests on a startup change, either raise the number of rounds or compare against this spread.

Known issues in this session:

- **Other agents' GUI runs.** One light sample (side B, warm) was invalidated by keystrokes from a `smoke.ps1` run that did not hold the mutex. The harness marked it invalid and excluded it. The mutex only works if every GUI run uses it.
- **WebView2 children after WM_CLOSE.** In the corrected self-check, 3 of 60 runs left `msedgewebview2.exe` children of their process group alive 30 s after WM_CLOSE (`shutdown_unconfirmed`); the earlier self-check had 4. Those samples were excluded. Whether this is Bowser's shutdown or WebView2's teardown is not determined here.
- **Fixture preflight stop.** In the corrected self-check, one preflight probe did not confirm exit after WM_CLOSE; `taskkill` ran, but the process group still reported members, so that attempt failed. The retry produced a valid sample.
