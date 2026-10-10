# Bowser perf-probe: tab switch

- Label: bowser-baseline
- Executable: C:\Users\charlie\AppData\Local\Temp\bb\bowser.exe
- SHA-256: 9d874d4cfe12eee3b38fb190d792b1e9030606b0073c33b14577e907414767b6
- Recorded: 2026-10-10T12:27:11
- Memory pressure flag: False

| Metric | Median (p25 to p75, n) | Failed to observe |
| --- | --- | --- |
| Ctrl+Tab to window title shows other tab (ms) | 30.0 (23.1 to 42.9, n=20) | 0 |

Raw rows: C:\Users\charlie\Documents\ChatGPT\Bowser\benchmarks\results\baseline-20261010\bowser-baseline-switch-20261010-122649.csv. Polling interval is about 1 ms plus PowerShell overhead; the main-window title (active tab title: light vs demanding fixture) is polled, not a frame presentation.
