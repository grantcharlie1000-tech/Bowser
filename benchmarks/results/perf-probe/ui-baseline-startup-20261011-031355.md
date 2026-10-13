# Bowser perf-probe: startup

- Label: ui-baseline
- Executable: C:\Users\charlie\Documents\ChatGPT\Bowser\.context\baseline-ui\bowser-baseline.exe
- SHA-256: aa2d86c001ec15a70e09cc336acf5da8188c4535039aa538836a01ffd35cf873
- Recorded: 2026-10-11T03:15:55
- Memory pressure flag: False

| Profile | Metric | Median (p25 to p75, n) |
| --- | --- | --- |
| fresh | Outer launch to visible (ms) | 87.0 (81.5 to 93.1, n=5) |
| fresh | window_visible (ms) | n/a |
| fresh | input_ready (ms) | n/a |
| fresh | chrome_painted (ms) | n/a |
| fresh | Host working set (MB) | 11.0 (11.0 to 11.0, n=5) |
| fresh | Host private (MB) | 2.5 (2.5 to 2.5, n=5) |
| warm | Outer launch to visible (ms) | 76.2 (76.0 to 185.5, n=5) |
| warm | window_visible (ms) | n/a |
| warm | input_ready (ms) | n/a |
| warm | chrome_painted (ms) | n/a |
| warm | Host working set (MB) | 11.0 (11.0 to 11.0, n=5) |
| warm | Host private (MB) | 2.5 (2.5 to 2.5, n=5) |

Raw rows: C:\Users\charlie\Documents\ChatGPT\Bowser\benchmarks\results\perf-probe\ui-baseline-startup-20261011-031355.csv. Event logs: C:\Users\charlie\Documents\ChatGPT\Bowser\benchmarks\results\perf-probe\ui-baseline-startup-20261011-031355-logs. chrome_painted is n/a on builds without that event.
