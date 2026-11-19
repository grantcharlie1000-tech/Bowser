# Cloud verification for Bowser’s Helium comparison

These results describe the v0.0.26 implementation and its archived binaries. On v0.0.38, the upstream tab strip already batches geometry reads for tabs and group islands. The integration preserves that implementation and applies the native scheduling and snapshot changes. Use the Windows procedure to compare the integrated candidate with the pinned v0.0.38 reference.

## Inputs and scope

The supplied checkout was clean at `440575e0870fd35c257d4195ec990d3c02b837aa` under the provisioned branch name `work`. The cached `origin/main` at `3fddb958260a1de94e30429f1629f327f0d8b6fe` was an ancestor. The comparison uses the newer source supplied in the checkout. Its original tree was exported before editing. The four named historical baseline files were absent from the task inputs. The old results in the repository establish investigation priorities only.

The host ran Debian 13 x86-64 with Rust/Cargo 1.99.1, MinGW-w64 GCC 14.2.0, Node 24.19.0, npm 11.9.0, Python 3.12.14 and PowerShell 7.6.8. Chromium 154.0.8037.57 ran the paired benchmark. Debian Chromium Headless Shell 156.0.8078.79 ran the broader browser-backed UI tests. The host had no Windows desktop, WebView2 installation, Helium reference, GPU device or power sensors.

Both clean and candidate source trees built as x64 Windows GNU release executables with locked dependencies and `html-chrome-default` in disjoint target directories. SHA-256 identities follow.

| Build | Executable SHA-256 |
| --- | --- |
| Supplied source, untouched | `2df0d688b908d1d76c3996527df549dbd3fc89763eff2f47a547ce688bfa16e8` |
| Candidate | `28c5c698e0cdebabd68a6bd79852581ac9a0270d9043956bf39168a8356b5a92` |

The Rust dependency lockfile SHA-256 is `f7b24003d11e7dca3210b737f5c815778b9852578fec87d572b0acf71d34c0d3`. The candidate UI manifest SHA-256 is `d65d3e879a2416d2fa0b9b6f5ee9c8ddc9eff997b12683be322ec1552fd910ec`. The native campaign specifies MSVC builds.

## Product changes

- The horizontal tab strip batches final position reads before writing animation transforms. Chromium previously recalculated style between individual writes and subsequent position reads. The same tabs still move, and the duration, transforms, landing behavior and reduced-motion branch stay in the existing code paths.
- Hidden split, Settings, History and Downloads hosts now share a timer for the earliest three-minute idle deadline, rather than waking every 30 seconds. A drag still defers closure. Future research can use the optional timer-fire and close events.
- The migration offer waits for a chrome state change after its initial startup delay. The old 500 ms retry ran before checking whether a profile had answered the offer. A pending offer still appears after the active page becomes usable.
- HTML tab synchronization moves the already-built snapshots into its cache and reuses the favicon keys it just read. This avoids copying tab titles and group state again and rescanning for the same keys.

The blocking Win32 `GetMessageW` loop already sleeps when no work is queued. Startup already issues async WebView2 environment and controller creation and uses native fallback chrome while the HTML toolbar loads. The next native trace should attribute elapsed time and CPU to WebView2 creation, controller setup, chrome sync, and renderer/compositor work. A many-tab trace can then quantify whether the remaining per-tab model scans and mute readbacks matter. Measure private commit by process role and confirm actual screen presentation before pursuing engine or rendering changes.

## Matched component evidence

The benchmark serves the before and after production UI assets from loopback with the same WebView bridge shim. Chromium CDP counts renderer work through a tab snapshot update and two animation callbacks. Each scenario has 12 valid matched pairs and alternating order. Both builds are required to produce the right tab order, titles and selected state. The launch and setup happen outside the measured interval.

| Work | Before style recalculations, median [range] | After | Matched reduction, median [range] |
| --- | ---: | ---: | ---: |
| Reorder 16 tabs | 26 [25–26] | 12 [11–12] | 14 [13–14] |
| Reorder 64 tabs | 97 [97–97] | 35 [35–35] | 62 [62–62] |
| Update one of 16 titles | 2 [2–2] | 2 [2–2] | 0 [0–0] |
| Update one of 64 titles | 1 [1–1] | 1 [1–1] | 0 [0–0] |

The layout count for reorder was 1 per attempt in both builds. At 64 tabs, style calculation time had medians 11.207 ms and 10.235 ms. The paired time improvement ranged from −1.689 ms to +2.382 ms, with a +0.995 ms median. The clear result is the reduction in style passes on this component. CDP callbacks and counters supply no measurement of physical frames, process-tree resources or power.

```sh
mkdir -p /tmp/bowser-prechange
git archive 440575e0870fd35c257d4195ec990d3c02b837aa | tar -x -C /tmp/bowser-prechange
npm --prefix ui run build
node benchmarks/tab-strip-component.mjs /usr/bin/chromium /tmp/bowser-prechange/ui/dist ui/dist /tmp/bowser-tab-component-new.json
```

The evidence archive includes the initial invalid harness attempt, both complete matched runs, the final summary, identities, the product patch, and test and cross-build logs.

## Check results

- Windows GNU test targets compiled in both default and shipping configurations. The shipping configuration passed Clippy with warnings denied. Both locked release binaries linked. A Linux-native `cargo test` build of the unchanged source failed on the Windows-only dependency before tests could run.
- UI typecheck, lint, bundle rebuild and source encoding checks passed. The portable UI suites passed 711, with 33 browser-dependent skips. With Chromium Headless Shell the first suite passed 675 of 676 with no skips. The reader column-width test failed identically on the untouched source. The second suite passed all 68 in the portable run. The earlier distro Chromium `--dump-dom` run hit timeouts and was stopped.
- All 446 Shield tests passed. The repository's Python benchmark tests and the portable PowerShell production measurement, sidebar and bookmark lifecycle contracts passed. The older PowerShell runner test failed identically on both trees under Linux at its Windows-only runner expectation.
- The new reducer and fixture tests passed, including fixture semantics and all 900 animation callbacks in Chromium. The Windows build script parsed in PowerShell. Windows Job Object ownership, native visibility and launch behavior need the Windows pilot.

The historical attachment harness could not be inspected. The checked-in legacy runner locates its window by launcher PID, omits new or exited identities from interval CPU deltas, and includes expensive inventories during idle. It lacks an independent physical frame or watt endpoint and uses a different launch mode for Bowser. The new runner leaves the old benchmark intact. It uses job-lifetime counters, isolated production-path launches and a separately frozen analysis and capture contract.

## Native status against Helium

| Metric | Status |
| --- | --- |
| First contentful paint and first useful presentation | Native validation pending |
| Displayed frames and missed updates | Native validation pending |
| Private commit and summed working set | Native validation pending |
| Idle and fixed-work CPU | Native validation pending |
| Whole-PC watts and workload energy | Native validation pending |
| Native interactions, accessibility and visual quality | Native validation pending |

Run the calibration pilot, the separate Bowser diagnostic lane, then the balanced three-arm campaign with synchronized physical presentation and power measurements. The [Windows procedure](README.md) gives exact commands, the evidence contract and the next diagnostic experiments.
