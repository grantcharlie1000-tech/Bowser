# Bowser perf-probe: startup

- Label: bowser-baseline
- Executable: C:\Users\charlie\AppData\Local\Temp\bb\bowser.exe
- SHA-256: 9d874d4cfe12eee3b38fb190d792b1e9030606b0073c33b14577e907414767b6
- Recorded: 2026-10-10T12:27:45
- Memory pressure flag: False

| Profile | Metric | Median (p25 to p75, n) |
| --- | --- | --- |
| fresh | Outer launch to visible (ms) | 333.8 (308.1 to 363.7, n=7) |
| fresh | window_visible (ms) | 154.2 (106.5 to 231.9, n=7) |
| fresh | input_ready (ms) | 154.3 (106.6 to 232.1, n=7) |
| fresh | chrome_painted (ms) | 157.9 (110.5 to 251.9, n=7) |
| fresh | Host working set (MB) | 10.2 (10.1 to 10.2, n=7) |
| fresh | Host private (MB) | 2.3 (2.3 to 2.3, n=7) |
| warm | Outer launch to visible (ms) | 365.2 (304.1 to 386.9, n=7) |
| warm | window_visible (ms) | 81.9 (73.4 to 128.9, n=7) |
| warm | input_ready (ms) | 82.1 (73.6 to 129.1, n=7) |
| warm | chrome_painted (ms) | 85.6 (80.8 to 132.7, n=7) |
| warm | Host working set (MB) | 10.1 (10.1 to 10.2, n=7) |
| warm | Host private (MB) | 2.3 (2.2 to 2.3, n=7) |

Raw rows: C:\Users\charlie\Documents\ChatGPT\Bowser\benchmarks\results\baseline-20261010\bowser-baseline-startup-20261010-122722.csv. Event logs: C:\Users\charlie\Documents\ChatGPT\Bowser\benchmarks\results\baseline-20261010\bowser-baseline-startup-20261010-122722-logs. chrome_painted is n/a on builds without that event.
