# Performance pass — 9 October 2026 (branch `perf/memory-speed-ui`)

Medians; Windows 10 Home, i5-7300HQ, 15.9 GiB. Baseline exe = commit `ee7e58b`. Not merged.

| Measure | Baseline | Now | Change |
|---|---:|---:|---:|
| Typed flow: Enter → page ready (1.5 s typing, loopback fixture, n=5) | 760 ms | 66 ms | −91% |
| Light page working set (idle ≥3 s) | 290 MB | 240 MB | −17% |
| Demanding page working set (idle ≥3 s) | 357 MB | 250 MB | −30% |
| Private/commit bytes (light / demanding) | 116 / 171 MB | 115 / 161–165 MB | ≈0 to −6% |
| Launch-with-URL → page ready (light/demanding, warm) | 1054 / 968 ms | 772 / 1062 ms | within run-to-run noise |

What changed: engine creation starts when the user begins typing (no network before Enter);
idle trimming asks WebView2 for `MemoryUsageTargetLevel::Low` after 3 s without input and restores
Normal on activity/navigation; native address box uses the system font, has a cue banner and inset layout;
`configure_controller` clippy failure fixed.

Tried and dropped: `--disable-features=msWebOOUI,msPdfOOUI,CalculateNativeWinOcclusion`, `--disable-breakpad`,
`--enable-low-end-device-mode` — no memory or process-count change beyond noise (still 7 processes).

Limits: the working-set saving is reclaimed/paged-out memory, not lower committed memory; first interaction after
idle may fault pages back in. Launch-with-URL speed is unchanged (creation cannot overlap anything there).
Private-bytes floor is dominated by the WebView2 browser (~37 MB), GPU (~36 MB) and renderer (~26 MB) processes.
Smoke (11 checks) passes; `cargo test` 61 pass; clippy `-D warnings` clean.
