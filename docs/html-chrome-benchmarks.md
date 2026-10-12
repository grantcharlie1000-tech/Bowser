# HTML chrome vs native chrome: measurements

Method: debug build (`target-chrome-a`), `--guest`, one tab on example.com, 12 s settle, then a 10 s idle sample. Process tree = bowser.exe plus all descendant processes. Windows 10 19045 dev machine. Two runs per mode; numbers vary a few percent. Debug builds exaggerate absolute Rust-side costs; compare the modes with each other.

## Baseline: separate `ChromeUI` environment (commit 9c7eaab)

| Metric | Native | HTML |
|---|---|---|
| Processes | 8 | 15 |
| Private memory | 130-135 MB | 268-271 MB |
| Working set | 282-283 MB | 632-636 MB |
| Idle CPU (10 s) | 0.02-0.04 s | 0.12-0.18 s |
| Toolbar painted | ~0.44 s | ~1.7 s (native chrome stays visible until swap) |

Verdict: a clear regression on every axis. Cause: a second WebView2 environment spawns its own browser, GPU, network and utility processes. Mitigation in progress: host the chrome in the tabs' environment as a private in-memory profile.

## After: chrome as a private in-memory profile inside the tab environment (commit da7943a)

Same method, two runs per mode.

| Metric | Native | HTML | Delta vs native |
|---|---|---|---|
| Processes | 8 | 10 | +2 (the two chrome renderers) |
| Private memory | 129-135 MB | 209-213 MB | about +80 MB (was +138 MB) |
| Working set | 282 MB | 433-435 MB | about +150 MB (was +350 MB) |
| Idle CPU (10 s) | 0.00 s | 0.01-0.02 s | negligible (was 0.12-0.18 s) |
| HTML toolbar replaces native | n/a (native ready ~0.44 s) | 1.6-2.9 s (single readings; probe is coarse) | native chrome stays visible and usable until then |

Remaining regressions (reported, not fixed): ~80 MB private memory and two extra renderer processes; HTML toolbar appears 1-2.5 s after the native one would. Ideas: one controller instead of two for the top chrome (saves a renderer), prewarm the environment earlier.

## With the rail view (three chrome controllers, commit a07f23d)

Same method, two runs per mode.

| Metric | Native | HTML | Delta vs native |
|---|---|---|---|
| Processes | 8 | 11 | +3 (one renderer per chrome view) |
| Private memory | 134-136 MB | 234-242 MB | about +100 MB |
| Working set | 281-284 MB | 491-498 MB | about +210 MB |
| Idle CPU (10 s) | 0.01-0.02 s | 0.03-0.05 s | small |

Each additional live chrome controller costs about 33-35 MB private memory and one renderer process. The plan's budget gate is <= 40 MB per added live host, so each view is within budget individually, but the total is a clear regression versus native (about +75%). Planned panel/popup/dialog views must be lazy (created on first use, suspended while hidden) to avoid adding to the idle cost. A structural alternative worth evaluating before making HTML chrome the default: a single full-window chrome controller z-ordered beneath the tab views (one renderer instead of three), which would need the native caption/overlay layer to stay above it.
