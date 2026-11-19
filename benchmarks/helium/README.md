# Bowser and Helium Windows comparison

## Build and pin the inputs

Use an interactive Windows 10/11 x64 session, Git, PowerShell 7, Python 3.11+ x64, MSVC C++ Build Tools, the Windows SDK, and a machine-installed Evergreen WebView2 runtime. Prepare an unpacked fixed Helium release, its archive, and its exact Chromium engine build. Use separate disposable builds and profiles.

Run from the candidate checkout and pass the exact untouched reference commit. For the v0.0.38 integration, use `2bd0cd825cacb1ab7c992666a0a9354eef317b14`. The build script uses Git archive and disjoint target directories. It records source, the product diff against the reference, executable hashes, toolchain, feature set, lockfile and UI bundle identity.

```powershell
rustup toolchain install 1.99.1 --profile minimal
rustup target add --toolchain 1.99.1 x86_64-pc-windows-msvc
$lab = 'C:\bowser-helium-lab'
$helium = 'C:\portable-helium\chrome.exe'
$wv = 'C:\Program Files (x86)\Microsoft\EdgeWebView\Application\VERSION\msedgewebview2.exe'
./benchmarks/helium/build-inputs.ps1 -Lab $lab -Helium $helium -HeliumVersion 'HELIUM_VERSION' -HeliumChromium 'CHROMIUM_VERSION' -HeliumArchive 'C:\downloads\helium.zip' -WebView2 $wv -BaselineRef '2bd0cd825cacb1ab7c992666a0a9354eef317b14'
$id = Get-Content "$lab\identities.json" -Raw | ConvertFrom-Json
notepad "$lab\setup.txt"
```

Substitute explicit versions and file paths. Retain the two Bowser builds, source archive, patch, Helium distribution, and identities together. The collector checks browser image timestamps and sizes between trials and SHA-256 at preflight and postflight. For a Helium launcher that re-executes a separate browser image, record its path, version and hash in the setup record and verify the window owner's image in the pilot.

## Collect the fixed campaign

Use one local, unlocked Windows session and one physical display. Set fixed resolution, scale, refresh rate, brightness, power plan and Battery Saver state. Record Windows build, CPU, RAM, GPU/driver, display mode, power setup, thermal conditions, network, active policies/extensions and time since boot in `setup.txt`. Leave the keyboard and pointer alone. Run AC and battery in separate sessions. Use ETW during the pilot to verify both browser launchers and helper ownership, including possible externally brokered processes.

Run a pilot with `--pilot`. Inspect `beacons.ready.viewport`. Calibrate Helium's initial outer dimensions to match Bowser's CSS viewport and DPR on both fresh and warm profiles. Repeat the pilot in a new directory with the calibrated size and the expected viewport, without resizing during a measurement. A separate `--diagnostic --pilot` run records Bowser event CSVs for startup and controller attribution.

```powershell
$common = @('--main', $id.main.exe, '--candidate', $id.candidate.exe, '--helium', $id.helium.exe,
            '--build-identities', "$lab\identities.json", '--setup-record', "$lab\setup.txt",
            '--power', 'battery')
py -3 ./benchmarks/helium/collect.py @common --helium-size '1000,700' --pilot --out "$lab\pilot-01"
# Replace both example sizes with values verified in the pilot.
$size = '1000,700'
$viewport = '980x590@1'
py -3 ./benchmarks/helium/collect.py @common --helium-size $size --viewport $viewport --pilot --out "$lab\pilot-02"
py -3 ./benchmarks/helium/collect.py @common --helium-size $size --viewport $viewport --pilot --diagnostic --out "$lab\diagnostic"
py -3 ./benchmarks/helium/collect.py @common --helium-size $size --viewport $viewport --out "$lab\battery"
```

- Light/idle, demanding/idle and demanding/animation each run with fresh and warm isolated profiles. Every scenario uses all six launch orders twice. Warm trials follow a validated priming launch and full shutdown of that arm on the same profile. OS file caches are uncontrolled. The isolated Bowser profiles pre-answer the first-run import offer to match normal browsing. Browser security and GPU defaults apply.
- The fixture bodies keep their original content and CPU work. The runner adds a unique title, Paint Timing and semantic checks. Idle starts five seconds after readiness and runs 30 seconds. Animation draws 900 sequential canvas updates using the existing fixture pixel payload. Changed viewport, foreground, display, power source or saver invalidate the trial. Ambient CPU above 5% of machine capacity invalidates the post-ready workload. A matched battery block requires charge readings within five percentage points.
- Each launcher starts suspended, joins a fresh Windows Job Object, then resumes. Its CPU tally includes exited members and descendants that inherit the job. The runner checks that Bowser's runtime matches the pinned WebView2 image. It requires an owned visible foreground window, normal `WM_CLOSE` shutdown and a fully drained job before the next launch. A profile-reference scan checks for surviving out-of-job processes. A brokered process gone before the scan requires separate pilot evidence.
- Each raw JSON has args, launcher and window identities, job counters, process images, clock boundaries, beacons and failure reasons. Keep all attempts and profiles until review. A failed drain stops the run. A return code of 1 indicates invalid attempts. `complete.txt` records a completed run with matching postflight executable hashes.
- Private commit and summed working set use native `GetProcessMemoryInfo` at the two workload boundaries. The reported value is the larger of those two snapshots. Working set includes shared pages and can count them more than once. The CPU score is a job core-second delta. System load and interval length serve as controls. Startup title timing and animation callbacks serve as diagnostics. FCP is relative to document navigation. Actual useful presentation is relative to process launch.

## Capture physical presentation and power

Record the monitor with a synchronized external high-speed recorder at twice the refresh rate or faster. Validate temporal uncertainty with a display transition and account for scanout. Before capture, define useful presentation as the first monitor frame where the fixture heading and first populated content row are visibly present in the target viewport. The animated canvas has a white reference cell and 12 black/white frame-ID cells, least significant bit first. Retain the original recording and count unique displayed IDs in 1–900. ETW or PresentMon can explain DWM/GPU cost. The displayed-frame endpoint needs decoded fixture output or an equivalent scanout capture.

Export `presentation.csv` with `token,first_useful_qpc_ns,presented_frames,refresh_hz,trace`. Use the attempt token, timestamp on the collector's QPC nanosecond clock, actual unique displayed count, fixed display refresh and a trace path relative to the CSV. Leave the displayed count empty for idle. Keep calibrated synchronization and omit ambiguous observations. Throughput divides the displayed count by the entire fixed-work interval.

On battery, use calibrated system measurement downstream of the battery or equivalent whole-device telemetry. On AC, record supply and charging losses for any wall meter. Record sensor, uncertainty, calibration, sampling rate and synchronization. Sample from a separate device where feasible, with at most one second between samples. Retain raw device output and export `meter.csv` with `utc_ns,watts` aligned to the host UTC clock. The reducer integrates the bracketed work window offline and excludes shutdown, profile inventories and reporting. It rejects clock discontinuities and missing samples. Whole-PC watts include display and unrelated device consumption.

```powershell
py -3 ./benchmarks/helium/analyze.py "$lab\battery" --out "$lab\results.json"
py -3 ./benchmarks/helium/analyze.py "$lab\battery" --meter "$lab\meter.csv" --meter-uncertainty-watts 0.05 --presentation "$lab\presentation.csv" --out "$lab\results-with-telemetry.json"
```

Replace `0.05` with the meter's measured absolute uncertainty in watts. Missing telemetry stays pending.

## Acceptance

Use at least ten valid complete three-arm blocks per scenario, with each arm's position counts balanced to within one. Report median, minimum/maximum, paired differences and the exact two-sided 95% sign interval for the paired median. Each margin is the larger of its absolute and relative values.

| Metric | Absolute margin | Relative margin |
| --- | ---: | ---: |
| Launch to observed useful presentation | 20 ms | 5% |
| FCP relative to navigation | 10 ms | 5% |
| Private commit and summed working set | 5 MiB each | 5% |
| Job CPU over completed workload | 0.05 core-seconds | 5% |
| Whole-PC watts | 0.25 W or twice meter uncertainty | 5% |
| Whole-PC energy | 4 J or twice meter uncertainty × 90 s | 5% |
| Displayed frames per second | 1 fps | 2% |
| Missed display updates | 1 percentage point | 0% |

For each paired observation the reducer divides the signed improvement by the reference-relative margin. A win needs the entire interval above +1, a loss the interval below −1, and a tie an interval inside [−1, +1]. Report refresh-ceiling parity as a tie. Other observed cases are inconclusive. Full acceptance requires all wins over Helium, uncertainty ruling out meaningful regressions against main and separate native quality verification.

Run the repository's release verification and smoke for both Bowser builds. Exercise same-site and cross-origin navigation, history, keyboard and IME input, tab reorder and landing, vertical tabs, splits, Settings, History, Downloads, and both motion settings. Confirm the HTML first-run welcome appears on a blank start tab and the native import menu appears when the active page is ready. Open then hide History or Downloads and verify a single `page_host_idle_timer_fired` followed by `page_host_idle_closed` at the deadline, with a drag delaying closure. Confirm a hidden Settings or split host stays available unless Windows reports memory pressure. Keep these probes separate from the scored campaign. The local fixtures characterize controlled work. Real-site throughput and native quality require their own evidence.

```powershell
$ps51 = "$env:WINDIR\System32\WindowsPowerShell\v1.0\powershell.exe"
& $ps51 -NoProfile -ExecutionPolicy Bypass -File benchmarks/smoke.ps1 -BowserExecutable $id.main.exe -OutputDirectory "$lab\smoke-main"
& $ps51 -NoProfile -ExecutionPolicy Bypass -File benchmarks/smoke.ps1 -BowserExecutable $id.candidate.exe -OutputDirectory "$lab\smoke-candidate"
wpr -start GeneralProfile -filemode
py -3 ./benchmarks/helium/collect.py @common --helium-size $size --viewport $viewport --pilot --diagnostic --out "$lab\trace-diagnostic"
wpr -stop "$lab\startup-and-cpu.etl"
```

The WPR capture is a separate diagnostic run. Use its startup, process lifetime and CPU sampling timelines alongside Bowser's event CSVs. Record GPU/DWM activity separately when investigating compositing. If WebView2 environment/controller creation dominates, inspect Microsoft runtime and driver work first. Use a disposable first-run profile without the import marker and a held request from `benchmarks/scripts/production-fixture-server.mjs` to observe the idle interval, then navigate to its light fixture to verify the offer. For tab animations use 16 and 64 tabs, continue through drag landing, and capture both browser style work and physical presentation while the active page runs a fixed workload.
