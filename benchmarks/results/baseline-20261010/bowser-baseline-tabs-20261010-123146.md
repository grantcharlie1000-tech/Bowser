# Bowser perf-probe: tabs

- Label: bowser-baseline
- Executable: C:\Users\charlie\AppData\Local\Temp\bb\bowser.exe
- SHA-256: 9d874d4cfe12eee3b38fb190d792b1e9030606b0073c33b14577e907414767b6
- Recorded: 2026-10-10T12:35:10
- Memory pressure flag: False

| Tabs | Loaded | Processes | Host WS MB | Host private MB | WV WS MB (settle) | WV private MB (settle) | WV WS MB (+50 s) | WV private MB (+50 s) | Idle 10 s host CPU ms | Idle 10 s WV CPU ms | tab_discarded |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | 1/1 | 7 | 16.24 | 3.25 | 229.1 | 118 | 44.6 | 115.9 | 0 | 31.2 | 0 |
| 5 | 5/5 | 11 | 16.43 | 3.38 | 288.4 | 243.5 | 96.8 | 243.9 | 0 | 125 | 0 |
| 10 | 10/10 | 12 | 16.91 | 3.56 | 370 | 388.4 | 123.8 | 273.3 | 0 | 218.8 | 4 |

Host = Bowser process only. WV = WebView2 child processes (renderers, GPU, utility). Samples at settle, +40 s and +50 s; idle CPU is the 10 s between +40 s and +50 s.
Raw rows: C:\Users\charlie\Documents\ChatGPT\Bowser\benchmarks\results\baseline-20261010\bowser-baseline-tabs-20261010-123146.csv. Event logs: C:\Users\charlie\Documents\ChatGPT\Bowser\benchmarks\results\baseline-20261010\bowser-baseline-tabs-20261010-123146-logs.
