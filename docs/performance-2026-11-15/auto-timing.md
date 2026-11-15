# Automated paired timing (benchmarks/auto)

Purpose: low-overhead, operator-free baseline-vs-candidate task timing. No CIM/process
inventory runs inside a timed interval; the only per-sample cost outside the app is
`SendInput` plus a fixture-log file read.

## Instrumentation (benchmarks/auto/instrumentation.patch)

Apply the same patch to the baseline and candidate source before building. It adds 8 single-line,
ungated `PerfLog` calls (written only under `--diagnostic-log`; buffered in memory, flushed when
the process exits, so the driver closes with WM_CLOSE). `git apply --check` passes on origin/main
4f4d677a6.

| event | file / site | role |
|---|---|---|
| `perf_input_new_tab` | window.rs `open_user_tab` entry | new tab start (native) |
| `perf_new_tab_returned` | window.rs `open_user_tab` exit | new tab handler end |
| `perf_tab_select_start` | app.rs `select_tab` entry | tab switch start (native) |
| `perf_omnibox_submit` | window.rs `ChromeCommand::Submit` | omnibox Enter start (native) |
| `perf_settings_open` / `perf_settings_ready` | settings_page.rs `open` / `Command::Ready` | settings start / end |
| `perf_internal_shown` | app.rs `show_internal_page` after `show_applied` | internal page applied |
| `perf_title_changed` | app.rs `title_changed` callback, only for titles starting `BOWSER_PERF_READY_` | content end (nonce) |

Reused existing events: `process_start`, `page_usable`, `submitted`, `navigation_started`,
`navigation_completed`, `enter_received`, `tab_discarded`, `tab_restored`.

The authoritative start of every interval is the driver's own QPC stamp taken immediately before
`SendInput` (same QPC clock as the app log); native start events are used to report
input-to-dispatch latency.

## Workloads and intervals

| workload | start | end |
|---|---|---|
| startup | driver stamp before process start | `perf_title_changed` nonce for `/light` fixture |
| new_tab (Ctrl+T) | SendInput stamp | `perf_new_tab_returned` (handler return) |
| tab_switch (Ctrl+1 to discarded tab, `--benchmark-memory-pressure on`) | SendInput stamp | `perf_title_changed` nonce after `tab_restored` |
| omnibox_enter | SendInput stamp for Enter | `perf_title_changed` nonce |
| settings_open (omnibox `bowser://settings`) | SendInput stamp for Enter | `perf_settings_ready` |
| conversation | idle 5 s, then 20 typed chars + wheel while 64 chunks stream | in-page frame gaps, Event Timing, long-animation-frame (posted to the fixture server) |

## Run

    powershell -File benchmarks\auto\run-pairs.ps1 -BaselineExe <b.exe> -CandidateExe <c.exe> -Pairs 10
    node benchmarks\auto\analyze.mjs <OutDir>          # markdown; add --json, --seed N, --resamples N

Safety: input is sent only while `GetForegroundWindow()` is the launched process's
`BowserNativeWindow`, re-checked (plus neutral modifier/button state) before every input call;
otherwise the sample is aborted and recorded as failed. Only processes the script launched are
closed (WM_CLOSE to the verified HWND, then `Process.Kill` on its own object after 20 s).

## Limitations

- "Presented" is not observable for the HTML chrome (no UI-to-native paint ack in the product).
  New-tab and settings ends are native-handler/ready events, not pixels. Content ends are title
  nonce events (set by the fixture after a double rAF).
- tab_switch relies on the discard timer firing within `-SleepWaitSeconds` (30 s default); samples
  without `tab_discarded` before the switch are rejected by the analyzer.
- Log flush happens only on process exit; if the app does not exit on WM_CLOSE the sample is
  killed and counted as failed (`log_not_flushed`).
- Conversation focus uses real clicks near the composer, verified by an in-page focus receipt.
- Requires 96 DPI, single foreground session, nothing else touching keyboard/mouse.
