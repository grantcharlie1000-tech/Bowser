# UI overhaul baseline (pre-overhaul build)

Measured 2026-10-11 with `benchmarks\perf-probe.ps1 -Label ui-baseline`. Executable: `.context\baseline-ui\bowser-baseline.exe` (SHA-256 `aa2d86c0…f873`). Raw outputs: `benchmarks\results\perf-probe\ui-baseline-*`.

Machine: LAPTOP-EMHV4OCC, Acer Predator PH317-51, Intel Core i5-7300HQ @ 2.50 GHz, 15.9 GiB RAM, Windows 10 Home 10.0.19045.

| Metric | Baseline |
| --- | --- |
| Startup fresh: window_visible (ms, median; p25–p75) | 38.2 (37.8–43.9), n=5 (event log) |
| Startup warm: window_visible (ms, median; p25–p75) | 43.6 (38.9–56.4), n=5 (event log) |
| Startup fresh / warm: outer launch to visible (ms) | 87.0 / 76.2 |
| Idle empty: host private / WV private (MB) | 2.5 / 0 |
| Idle loaded: host private / WV private (MB) | 3.4 / 114.8 |
| Idle CPU, 20 s, empty / loaded (ms, host + WV) | 0 + 0 / 0 + 0 |
| Tabs 1: WV private MB at +50 s (host private) | 111.9 (3.4) |
| Tabs 5: WV private MB at +50 s (host private) | 211.7 (3.5) |
| Tab switch, Ctrl+Tab to title change (ms, median; p25–p75, n=20) | 16.3 (15.6–19.2) |
| Exe size (bytes) | 2,618,368 |

Notes:
- `window_visible` is blank in the probe's CSV and summary, because this build writes its event CSV only at shutdown. Values above come from the per-run event logs.
- The tabs-mode "Loaded" column reads 0/N for the same reason. Event logs show `page_usable` 1/1 and 5/5.
- Idle CPU is 0 for every window, which is lower than the earlier `baseline.json` figures (312.5 ms loaded). Treat it as suspect until re-run.

Not measured at baseline (no probe exists): menu-open latency, resize responsiveness, split-view overhead. `cargo test` was not run, because it would build and the baseline run was build-free.
