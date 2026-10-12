# Phase-2 "before" baseline (2026-10-12)

**Read this first: these numbers are NOT a clean baseline.** During collection the 4-core i5-7300HQ was saturated by other agents' `rustc` builds and a runaway `find /` (system CPU 95-100% before every run; 0 of 48 runs met the "quiet" gate of <35% CPU). A quiet 3-run smoke earlier in the session (same exe, before app-data isolation was added, results not kept) gave fixture ~1.2-1.5 s and example.com ~1.4-1.5 s launch-to-usable, versus 5-9 s fixture here. Use these tables for phase structure and relative cost only, and re-run on a quiet machine with the command below (it records `cpu_before_pct`/`quiet` per run and reports a quiet-only column).

- Binary: `benchmarks/bin/bowser-before.exe` (not committed), built from commit 2395cbf with `cargo build --release --features html-chrome-default` (SHA-256 a5206312...fcad). UI mode: build default, i.e. HTML chrome (no BOWSER_UI override). Note the event log shows no separate chrome WebView2 controller (one controller_requested/result pair before navigation), and `engine_started_for_chrome` never fires on the `--open-url` path.
- Machine: Intel i5-7300HQ, 4 logical cores, 15.9 GiB, Windows 10 Home 19045, WebView2 runtime 154.0.4258.62, High performance power plan.
- Runs: fixture 8 cold + 8 warm; each real site 4 cold + 4 warm (fewer than the requested 15/8 because each run took ~50 s under load). All 48 runs reached usable.
- Every launch used a throwaway `LOCALAPPDATA` (`<profile>.appdata`) plus `--benchmark-user-data-dir`. Earlier smoke/tuning runs of the harness and the first two aborted full runs (not kept) were launched BEFORE this isolation existed and may have touched the real `%LOCALAPPDATA%\Bowser` (history.txt was found 0 bytes, mtime 14:41 on 2026-10-12). The kept results all used isolation.
- Cold = fresh profile dir per run; warm = fresh copy of a once-primed profile per run. OS file cache not flushed.
- "usable": `page_usable` event on the local fixture; real sites have no such event (Bowser only emits it for BOWSER_BENCH_READY titles), so first `navigation_completed` is used.

Full tables (median / p95, every mark and phase): `launch/summary.md`; raw: `launch/runs.csv`, `launch/summary.json`, `launch/logs/`. Reproduce:

    powershell -NoProfile -ExecutionPolicy Bypass -File benchmarks\launch-to-usable.ps1 -BowserExecutable benchmarks\bin\bowser-before.exe -Label before -Mode both -Runs 15 -RealSiteRuns 8 -OutputDirectory <dir>

Tab switch (perf-probe switch mode, 20 presses, loaded machine): median 26.7 ms (p25-p75 18.0-77.8), 0 unobserved; `perf-probe/`. Repeated navigation/new-tab: not measured (perf-probe `tabs` mode not run; it needs ~1 min per tab count).

## Headline (loaded machine; usable ms from process_start, median / p95)

| Site | Cold | Warm |
| --- | --- | --- |
| fixture (localhost) | 8668 / 13396 | 5100 / 6853 |
| example.com | 3703 / 7296 | 1578 / 3932 |
| Wikipedia | 6588 / 6951 | 7589 / 8473 |
| github.com | 8880 / 12260 | 5592 / 7230 |
| bbc.co.uk/news | 6403 / 10050 | 4715 / 6122 |

## Where the time goes
- window_visible and chrome_painted are small (about 50-250 ms; chrome paint adds ~50 ms).
- WebView2 environment creation is ~7-15 ms.
- **Content controller creation (webview_controller_requested to result) is the biggest pre-navigation cost** (quiet smoke ~650-780 ms; here 0.7-6 s). Navigation cannot start until it finishes because the initial URL is held until then.
- navigate_called to navigation_started is ~35-100 ms; navigation_started to content_loading and to completed are network/site bound (fixture still 0.4-0.8 s quiet; real sites 1-3 s+).
- completed_to_usable on the fixture is ~11 ms.
