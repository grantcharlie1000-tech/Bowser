# Bowser perf-probe: tab switch

- Label: ui-baseline
- Executable: C:\Users\charlie\Documents\ChatGPT\Bowser\.context\baseline-ui\bowser-baseline.exe
- SHA-256: aa2d86c001ec15a70e09cc336acf5da8188c4535039aa538836a01ffd35cf873
- Recorded: 2026-10-11T03:33:20
- Memory pressure flag: False

| Metric | Median (p25 to p75, n) | Failed to observe |
| --- | --- | --- |
| Ctrl+Tab to window title shows other tab (ms) | 16.3 (15.6 to 19.2, n=20) | 0 |

Raw rows: C:\Users\charlie\Documents\ChatGPT\Bowser\benchmarks\results\perf-probe\ui-baseline-switch-20261011-033208.csv. Polling interval is about 1 ms plus PowerShell overhead; the main-window title (active tab title: light vs demanding fixture) is polled, not a frame presentation.
