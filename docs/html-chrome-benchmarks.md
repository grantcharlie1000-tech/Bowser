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
