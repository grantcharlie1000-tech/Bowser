# Bowser perf-probe: idle

- Label: bowser-baseline
- Executable: C:\Users\charlie\AppData\Local\Temp\bb\bowser.exe
- SHA-256: 9d874d4cfe12eee3b38fb190d792b1e9030606b0073c33b14577e907414767b6
- Recorded: 2026-10-10T12:29:02
- Memory pressure flag: False

| State | Page loaded | Host CPU over 20 s (ms) | WV CPU over 20 s (ms) | Host WS MB | Host private MB | WV WS MB | WV private MB |
| --- | --- | --- | --- | --- | --- | --- | --- |
| empty_focused_omnibox | 0 | 0 | 0 | 10.16 | 2.32 | 0 | 0 |
| loaded_page | 1 | 0 | 359.4 | 16.38 | 3.35 | 227.7 | 116.2 |

Raw rows: C:\Users\charlie\Documents\ChatGPT\Bowser\benchmarks\results\baseline-20261010\bowser-baseline-idle-20261010-122748.csv. Event logs: C:\Users\charlie\Documents\ChatGPT\Bowser\benchmarks\results\baseline-20261010\bowser-baseline-idle-20261010-122748-logs.
