# Bowser 0.0.1 results

## Status

The Phase 0 vertical slice is implemented and usable on x64 Windows with Microsoft's Evergreen WebView2 Runtime installed. Bowser starts with an empty native window, a focused native address/search input, and no renderer process. On first submission it searches through the configurable provider or navigates to an HTTP(S) URL in the one viewport. Search-result links, Alt+Left/Right history, and the safe default policies passed the UI smoke checks.

The user interface and navigation/security checks passed, including a real DuckDuckGo HTML search for `Rust WebView2`, clicking the visible Docs.rs result, verifying the destination URL and title in the same Bowser window, and closing Bowser during a held request. The smoke also verified popup containment/blocking, permission denial, download cancellation, external protocol blocking, and clean WebView2 process/profile shutdown. `cargo test` passed 30 tests, including actionable missing-runtime guidance. The installed runtime on this host means the full missing-runtime UI path was not exercised manually.

No launch, fixture, metric, or shutdown failures were recorded in the final 120-row browser benchmark. Blank-submit intentionally has no post-submit CPU/RAM sample because its timer stops at Enter; this is documented as not applicable. Battery energy is unmeasured.

## Benchmark run

The full data is in [`benchmark-20261009-051815.md`](../benchmarks/results/final-20261009-051814/benchmark-20261009-051815.md), with raw rows in the adjacent CSV and host/version/hash metadata in the JSON. It is five paired fresh/reused profile trials for each of four workloads and Bowser, Edge, and Chrome, with 30-second CPU windows. The machine was Windows 10 Home 22H2, Intel Core i5-7300HQ at 2.50 GHz (4 logical processors), and 15.9 GiB RAM. Bowser 0.0.1 used WebView2 152.0.4191.66/154.0.4258.62; Edge was 154.0.4258.62 and Chrome 154.0.8037.98. Input hashes matched before and after collection.

`cold-profile` is a new browser profile; `warm-profile` is a restart using the paired profile. Windows filesystem/standby caches were not flushed, so these are not cold-OS starts. The page tests use local, static fixtures. The host had 15.9 GiB RAM, so the results do not demonstrate memory pressure behavior on a 4 GiB budget laptop.

| Workload | Bowser cold / warm | Edge cold / warm | Chrome cold / warm |
|---|---:|---:|---:|
| Empty window visible (ms) | 188 / 194 | 890 / 912 | 921 / 991 |
| Address ready to submit (ms) | 323 / 310 | 1,032 / 887 | 978 / 940 |
| Phrase entered and Enter sent (ms) | 1,569 / 1,620 | 2,119 / 1,997 | 2,046 / 2,014 |
| Lightweight page usable (ms) | 1,080 / 878 | 1,436 / 1,438 | 1,455 / 1,215 |
| Demanding page usable (ms) | 1,177 / 1,101 | 1,921 / 1,604 | 1,769 / 1,607 |

Values are medians of five samples; full ranges are in the linked report. Address-ready timing is the first time the focused input can accept a search. Phrase-entry timing includes synthetic typing of a fixed phrase and ends immediately at Enter; it excludes provider response and is sensitive to input injection timing.

| Bowser process group | CPU median | Working set median | Private/commit median | Process count |
|---|---:|---:|---:|---:|
| Empty `about:blank`, cold / warm | 0.00% / 0.00% | 9.42 / 9.44 MiB | 1.76 / 1.77 MiB | 1 |
| Lightweight page, cold / warm | 0.06% / 0.03% | 292 / 290 MiB | 117 / 115 MiB | 7 |
| Demanding page, cold / warm | 0.10% / 0.10% | 356 / 359 MiB | 171 / 174 MiB | 7 |

These are sums across the Bowser/WebView2 process group. Working sets can count shared physical pages more than once; private/commit bytes are reported separately. The benchmark reads `Win32_Process` counters in one approximate snapshot, uses process creation time to distinguish reused PIDs, and samples CPU once per second. A very short-lived child that starts and exits between CPU samples may not contribute its lifetime CPU. See the benchmark README for the method and units.

## Largest measured bottleneck

The dominant measured resource cost is the first active WebView2 page, not Bowser's native shell. Before navigation the window is one process at about 9.4 MiB working set and 1.8 MiB private/commit. With a page loaded, Bowser has seven processes and uses about 290–359 MiB summed working set and 115–174 MiB private/commit. The light fixture alone adds roughly 115 MiB private/commit over the empty shell; the demanding fixture adds roughly 169–172 MiB. This points to the embedded runtime/renderer as the main resource floor, though process-level attribution and shared pages need finer measurement.

The first-use delay is the next visible cost. Bowser's internal event logs show WebView2 ready at a median 463 ms (cold) / 480 ms (warm) for the demanding fixture; initial navigation then takes about 545 / 563 ms. For the lightweight fixture, engine ready is 606 / 458 ms and navigation takes about 375 / 281 ms. The native window itself is visible much earlier. These timings identify engine creation and first navigation as the main contributors to first-page latency; they do not justify disabling security or preloading the engine.

## Disk footprint

- Bowser release executable: **666,112 bytes (0.64 MiB)**; no adjacent `WebView2Loader.dll` was present.
- Shared installed Evergreen WebView2 version directories: **1,963,805,232 bytes (1.83 GiB)** on this host.
- Runtime files are shared system software, not part of Bowser's executable footprint. User profile/cache data is also excluded.

## Next performance work

1. **Measure the WebView2 floor by process role.** The measured page group has seven processes and costs 290–359 MiB working set. Capture process roles and a true per-process memory breakdown across page types, then test whether any documented, security-preserving engine configuration reduces fixed memory. If the runtime floor remains dominant, compare only maintained engines with sandboxing, site isolation, and timely security updates intact. Any reduction is a hypothesis until measured.
2. **Break down first-use initialization.** Engine readiness currently takes roughly 0.46–0.61 seconds, followed by 0.28–0.56 seconds to navigation completion in these fixtures. Add spans around environment creation, controller creation, policy-handler setup, and first navigation; optimize the largest measured safe phase while keeping the current no-preload empty startup.
3. **Validate under actual memory pressure and measure energy.** This run used a 15.9 GiB machine and did not record battery energy. Repeat on a 4 GiB low-power laptop and measure system/package energy during blank, light, and demanding workloads. Hypothesis: the measured 115–174 MiB private/commit floor may trigger paging and battery cost on target hardware; this run cannot establish that.
