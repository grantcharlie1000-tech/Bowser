# Bowser perf-probe: startup

- Label: baseline
- Executable: C:\Users\charlie\AppData\Local\Temp\ws7-bowser\exe\bowser.exe
- SHA-256: 1b5a46205ca5e75963ca2539005bcd96e7988152e76ef2afdb4796c09cb06de4
- Recorded: 2026-10-10T13:01:43
- Memory pressure flag: False

| Profile | Metric | Median (p25 to p75, n) |
| --- | --- | --- |
| fresh | Outer launch to visible (ms) | 434.1 (343.6 to 788.6, n=5) |
| fresh | window_visible (ms) | 87.5 (82.9 to 214.3, n=5) |
| fresh | input_ready (ms) | 87.6 (83.0 to 214.4, n=5) |
| fresh | chrome_painted (ms) | 199.9 (91.3 to 217.8, n=5) |
| fresh | Host working set (MB) | 10.2 (10.1 to 10.2, n=5) |
| fresh | Host private (MB) | 2.2 (2.2 to 2.3, n=5) |
| warm | Outer launch to visible (ms) | 394.5 (372.3 to 514.6, n=5) |
| warm | window_visible (ms) | 113.6 (83.1 to 174.1, n=5) |
| warm | input_ready (ms) | 113.7 (83.2 to 174.3, n=5) |
| warm | chrome_painted (ms) | 117.4 (112.3 to 178.6, n=5) |
| warm | Host working set (MB) | 10.2 (10.2 to 10.2, n=5) |
| warm | Host private (MB) | 2.3 (2.2 to 2.3, n=5) |

Raw rows: C:\Users\charlie\Documents\ChatGPT\Bowser\benchmarks\results\baseline-run-20261010\baseline-startup-20261010-130118.csv. Event logs: C:\Users\charlie\Documents\ChatGPT\Bowser\benchmarks\results\baseline-run-20261010\baseline-startup-20261010-130118-logs. chrome_painted is n/a on builds without that event.
