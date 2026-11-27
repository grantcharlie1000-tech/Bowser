# Bowser and Helium on Windows

Run from the repository root in an unlocked, interactive Windows desktop session.
Use Python 3.14 x64 and a primary screen at least 1330 × 950. Keep other activity
low and leave the measured window unobstructed. The video fixture requires FFmpeg with
`libx264` on PATH the first time it is generated.

```powershell
$bowser = (Resolve-Path target/release/bowser.exe).Path
$installed = "$env:LOCALAPPDATA\Programs\Bowser\Bowser.exe"
$helium = "$env:LOCALAPPDATA\imput\Helium\Application\chrome.exe"
$log = '<path to the externally run release build log>'
$marker = "$env:USERPROFILE\.t3\artifacts\bowser-vs-helium\MEASURING"
py -3 -B benchmarks/vs-helium/bench.py --arm "bowser=$installed" --arm "helium=$helium" --quick --sites local
py -3 -B benchmarks/vs-helium/bench.py --arm "bowser=$bowser" --arm "helium=$helium" --build-log $log --measurement-marker $marker --runs 5 --sites real
py -3 -B benchmarks/vs-helium/bench.py --arm "bowser=$bowser" --arm "helium=$helium" --build-log $log --measurement-marker $marker --runs 5 --sites local --scenarios memory_tabs,power
py -3 -B benchmarks/vs-helium/bench.py --arm "bowser=$bowser" --arm "nosp=$bowser" --arm "helium=$helium" --arm-setting "nosp:spoiler_protection=off" --measurement-marker $marker --diagnostic --runs 2 --sites local --scenarios speedometer
```

Additional arms use `--arm candidate=<exe>` for Bowser or
`--arm candidate:helium=<exe>` for Chromium. `--out` takes a new output directory.
Otherwise reports go under `benchmarks/results/vs-helium/<mode>-<sites>-<timestamp>`.
`--arm-env "candidate:BOWSER_EXP_ARGS=--foo"` adds an environment variable to
candidate launches. `--arm-setting "candidate:spoiler_protection=off"` writes a
`key=value` line to that Bowser arm's default profile `settings.txt`, directly
under its disposable `--data-dir`. Both options are repeatable and recorded
in the manifest. Profile location overrides are reserved for the harness.
`--measurement-marker` creates an exclusive coordination file containing the run
label and UTC start time and removes it when the recorded process exits. An existing
marker prevents a second run from starting. To extend a recorded run, pass the same
`--out` and controls with `--resume --runs 5`. The harness checks the stored identities,
reuses valid trials, keeps invalid attempts, and continues the schedule by trial ordinal.
For an unavailable real site, `--resume-site-update` retains the previous list and
permits the revised input only when accepted workload trials used the same pages.
`--resume-after-fix "reason"` records a harness repair, prior source hashes and the
accepted trial files retained under the same measurement methods. The resume
summary identifies historical 30-second idle trials superseded by the 60-second
protocol. Their original raw files remain available.
`--scenarios` takes a comma-separated selection of `memory_tabs,newtab,launch_window,
launch_restore,speedometer,power`. Full runs measure five launches per arm and scenario,
except window and restore, which use ten each (`--launch-window-runs` and
`--restore-runs`). Odd repetitions use argument order. Even repetitions
reverse it. Smoke uses one launch per arm except restore, which also has a priming
launch. Smoke uses three Ctrl+T samples, shorter delays, six-second power windows and
one Speedometer iteration. Its timings are for exercising the pipeline.
`--diagnostic` uses full workloads, permits battery or Saver, and labels the
observed power state by arm on every result and role row.
`--require-ac` applies the AC and Saver-off guard to diagnostics and smoke runs.

## Isolation and controls

- Each attempt uses a unique temporary profile and an OS job. Windows assigns the
  suspended launcher atomically using the job-list process attribute before any browser
  code runs. Children inherit the job. There is no profile reuse across trials, except
  the two launches in a restore attempt.
- Memory, power, Speedometer and new-tab trials copy a settled template for their
  arm. The template starts with a single local fixture and a fresh profile. It
  requires four consecutive five-second job CPU samples below 2% of one core,
  with a three-minute deadline. The harness closes the browser normally, removes
  Bowser's saved priming tab while keeping its warmed data, and records a content
  hash and timestamps under `profile-templates`. A resume revalidates that
  template. Cold window and cold-restore runs still start with independent profiles.
- Bowser receives an absolute `--data-dir`, a disposable `LOCALAPPDATA` and the
  product's `import-offer.txt` marker with `answered\n`. Helium receives an absolute
  `--user-data-dir`, its First Run sentinel, its default-browser suppression switch
  and the browser's `helium.completed_onboarding` preference in the disposable profile.
  Helium restore uses the normal Chromium Continue where you left off
  setting. The priming launch selects it through the settings UI and exits with
  `helium://quit/` while the fixture is open. The harness checks for
  `session.restore_on_startup = 1` in the browser's preference stores, including
  `Default/Secure Preferences` on Windows.
  Bowser uses its ordinary session restore. Blocker and tab-sleeping settings stay
  at each product's defaults. Inherited Bowser/WebView2 test overrides are removed.
- Every keyboard chord, UTF-16 character and mouse event goes through a foreground
  HWND and OS job membership guard. The harness retries activation of its own window
  on focus loss. If Windows denies activation, it briefly attaches the input queues
  to request activation and detaches. On Windows 10 it can also request an owned
  window switch through the window manager. It verifies the target before sending.
  A chord's down/up events use one `SendInput` packet, and wheel routing has a separate
  cursor hit test. Raw trials record recovered focus changes, including any recovery
  inside a memory-tab sampling window. Desktop input has a scheduling gap between
  verification and dispatch. Leave the test window unattended during samples.
  During passive memory and idle sampling, the harness keeps its window visible
  and verifies on-screen ownership without requiring keyboard focus. A temporary
  Windows execution-state request keeps the display awake during the idle waits.
  Between timed windows, the harness can restore or raise its own window without
  activating it if a probe gets covered. It rejects an obscured timed window.
  Subsequent navigation and close keystrokes still require verified foreground.
- The harness requests WM_CLOSE for owned windows and verifies the job is empty
  before deleting a profile or starting the next launch. A failed graceful drain uses
  job-only termination, retains the disposable profile and stops the suite. A foreground
  recovery that exhausts 15 seconds invalidates the individual trial. A process
  ownership or input-safety failure stops the suite.
- The standard outer window rectangle is 1280 × 900 at (50, 50) physical pixels.
  Cold launches use 1000 × 720 to match Bowser's native initial dimensions. Cold
  geometry is verified at presentation without moving the browser first. Matched
  outer window geometry and monitor DPI allow product chrome to take its normal space.
- Each trial waits up to 30 minutes for a 10-second average of at most 15% of total
  machine CPU after subtracting the benchmark job and harness process. The raw
  trial records the measured duration, total CPU, excluded CPU and ambient CPU.
  Set `--quiet-wait`, `--ambient-sample` and `--max-ambient-cpu` to override
  the defaults. Idle, power and Speedometer windows also record job-adjusted CPU.
  Their rejection ceiling defaults to the pretrial limit and can be set separately
  with `--max-workload-ambient-cpu`. The power workload gate excludes the separately
  sampled DWM CPU delta. The raw sample also retains the outside-job CPU total
  with DWM included.
  Raw trials record AC line status, battery percentage and Battery Saver at
  entry, exit and observed changes. Recorded runs wait for AC with Saver off.
  During a power interruption the active trial is invalidated and its job drains.
  The harness samples the power state once per minute for up to two hours
  (`--power-wait-seconds`), then restarts the affected repetition once ready.
  Display geometry and executable SHA-256 are checked between trials.
  Supply `--build-log` to require an `EXIT=0` last line before measurement.
- `sites.json` freezes the real URLs and expected document-title markers. A real
  page must show the designated title for two seconds. This verifies the intended
  document became active, with limited visibility into subresources, consent prompts,
  redirects and subsequent work. Network conditions and real content can vary.
  Local tabs use loopback pages of increasing HTML and canvas weight with unique
  title and load/visibility beacons. Both modes open additional tabs with Ctrl+T,
  focus the address bar, type the URL and press Enter.

## Metrics

| Metric | Definition |
| --- | --- |
| Private working set and commit | Sum Windows `SystemProcessInformation` private working set and `PagefileUsage` across verified job members at 1, 10, 20 and back to 1 tab after 19 Ctrl+W events. Sample about 30 s after each state reaches readiness. MiB uses 1,048,576 bytes. |
| New tab first paint | Time from the guarded Ctrl+T packet to the first composed-screen sample in which a 16 × 16 center patch ceases to be at least 95% magenta. The starting loopback page is verified magenta and checked again after closing the new tab. |
| New tab visually settled | Time from Ctrl+T until the patch is unchanged for 300 ms. Fifteen attempts form one launch median. The raw file preserves individual samples and maximum capture gaps. |
| Cold window presentation | Time from the call to create a suspended process to the first composed-screen patch that differs from the harness's solid-color backdrop and belongs to the owned browser window. This times the central part of the window. |
| Cold restore | Time from process creation call to a magenta patch in the owned window. The previous clean launch opened exactly one magenta fixture and waited for the normal session writer. Bowser closes by WM_CLOSE and Helium uses its browser-wide quit action. The measured launch gets no URL and must report a new fixture beacon. |
| Speedometer 3.1 | Unrounded WebKit score from the final successful result, with all default suites and ten iterations in a fresh browser. The loopback server appends a reporter to the pinned `main.mjs`. The harness releases the start once the matched-size owned window is unobstructed and validates a visible result. The window is placed topmost without requesting keyboard focus. The reporter leaves the benchmark page after CPU sampling, so this scenario uses no synthetic input, DevTools or CDP. |
| Power proxies | Calibrated browser CPU and PDH `GPU Engine(*)\\Running Time` raw busy-time deltas over 60 s after eleven tabs load and settle. Add CPU core-ms/s and GPU engine-ms/s for activity. Record desktop-session DWM tick CPU and GPU separately. |
| Idle wakeup proxy | Sum per-thread `ContextSwitches` deltas from `SystemProcessInformation`, divided by wall seconds. At one and twenty tabs, sample 60 s from 10 s after readiness and again 60 s from five minutes, or from the specified `--settle-seconds`. |
| Idle CPU | `QueryProcessCycleTime` deltas for verified job members over the same four windows, calibrated to an estimated percentage of one logical core. The secondary job CPU column uses the original scheduler tick accounting, including exited processes. |

Power runs keep ten background tabs and an eleventh scenario tab. The five cases
are minimized idle, visible idle, a local long article scrolled ten wheel notches
per second, a muted looping 1080p30 H.264 video, and Ctrl+Tab once per second. The
article offset, video frames and tab title sequence verify the workload ran.

`--power-cases "idle visible,switching tabs"` selects and orders power cases.
`--trace-switch-renderers` adds per-renderer cycle reads immediately before each
Ctrl+Tab and about 400 ms after dispatch. The raw record links the reads to the
verified active tab and reuses the full window's cycle calibration. The summary
shows the busiest renderer PIDs and their early CPU distribution by activated tab.
The timing pattern can distinguish broad recurring work from tab-linked work.
It does not establish frame or renderer ownership by itself.
The report uses each tab's median early CPU. Broad activity reaches at least
75% of target tabs at 0.5 ms or 20% of the peak, whichever is higher.
Concentrated activity puts at least 55% of the summed medians on two tabs,
with a peak at least three times the median across tabs. These labels require
samples for every target and at least 80% of the switches for that PID.

The official [WebKit Speedometer 3.1 source](https://github.com/WebKit/Speedometer/tree/1386415be8fef2f6b6bbdbe1828872471c5d802a)
is pinned to `1386415be8fef2f6b6bbdbe1828872471c5d802a`. The archive SHA-256 is
`cfefa818d7789ed2f2b9f3f1bf608cc35e4e8241489eef47049296ce92791313`.
Both the archive and generated video cache live under
`%LOCALAPPDATA%\bowser-bench-cache`. Their hashes are recorded in each manifest.
The harness uses a unique scratch directory there for each invocation so Windows
Temp cleanup during long waits cannot remove the served fixture or profiles.

For a separate renderer attribution run, use `probe-restore.py --exe <bowser> --out <existing-report-directory> --measurement-marker <marker>`. It primes a disposable profile, samples the owned job on one instrumented restore, and fires a loopback content-only CPU pulse after first paint. The probe's CSV and JSON are excluded from timing medians.

## Interpretation and output

The suite writes a manifest, one JSON file per attempt and a Markdown summary with
median, min/max, count, delta and winner. Values within 2% of the larger absolute
median are a photo finish. The summary also includes per-role memory, CPU, context
switch and GPU breakdowns. An incomplete arm has no winner. Invalid trials retain
their reason and any samples already captured. A typical full run takes multiple hours
because of the idle windows and repeated Speedometer workloads.

GDI samples what has reached the composed desktop. Polling can miss transient frames,
and the 300 ms stability result describes the center patch only. Cold means that the
benchmark job was empty before launch. Windows file caches remain warm. Public-site
content and the browser/runtime installations can change over time.

Activity is a time proxy without a watt or energy calibration. Concurrent GPU
engines can accumulate at the same time. DWM also services other applications.
GPU instances that disappear or reset between the endpoints can lose work, and the
raw window records flag those instances. Context switches count scheduler transitions
and include threads alive at the endpoints or born in the interval and still alive at
its end. Short-lived threads can escape this estimate.

CPU calibration samples `QueryThreadCycleTime` on a CPU-bound harness thread
against QPC with five short pulses at each endpoint. The estimate uses the
median of the three fastest usable pulses to reduce scheduling bias.
Raw data contains the
calibration samples, process cycles per second and cycle-derived core-ms/s.
The time estimate depends on the counter rate remaining representative across
cores and power states. The Windows API makes no universal cycle-to-time
guarantee. The harness flags browser PIDs that exit or cannot be sampled, and
retains complete tick-based job CPU and the unattributed tick residual.
Speedometer uses the same role and CPU accounting around the score workload.
