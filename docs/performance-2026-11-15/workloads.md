# Frozen workload and evidence plan

This plan makes the user's per-area target explicit. A successful build, source audit, callback, accessibility tree, title change, or animation-frame marker alone is not presentation or input-latency proof. Synthetic conversation behavior must also be checked on ChatGPT.com later; it cannot establish real-site gains by itself.

## Conditions shared by each matched pair

Record source and binary hashes, build flags, WebView2 version, fixture hashes, profile seed, extension and protection settings, DPI, viewport, power state, cache condition and controller/document identities. Run ordinary release Bowser with isolated app data. The hidden agent window and benchmark behavior flags do not qualify as normal browsing.

Run one workload lane at a time with no builds, tests, profiling or other owned browser sessions competing for CPU/RAM. Alternate baseline/candidate order. Separate fresh profile, reused profile with all owned processes exited, and warm browser. A fresh profile does not establish cold OS cache.

Keep all attempts, failures, cancellation and timeout reasons. Count success only for the correct foreground executable and window, current nonce/document, correct content, completed work and usable action. Wrong/error/authentication pages, fallback-only chrome, lost input and missing widgets fail. Report task time and paired ratio with uncertainty and tails per workload. RAM, CPU, GPU, page faults, network and resource/battery effects are separate evidence.

## Required tasks

| Area | Start | Required completion and distinct conditions |
| --- | --- | --- |
| Startup | External process launch | Complete intended chrome and start/target page visibly presented; fresh input challenge works. Include all work before internal `process_start`. |
| New tab | Physical Ctrl+T or button | Correct selected tab, configured start content/widgets/images complete, focus and navigation work. Separate first use, reused host and configured web-page cases. |
| Suspended wake | Selection input | A successfully suspended retained controller resumes the same document, presents current content and responds to a fresh challenge. A requested suspension alone is insufficient. |
| Discarded wake | Selection input | New controller/document at the current expected URL with correct content and working input. Preserve drafts/media/download/private protections. |
| Page load | Navigation action | Expected content presented and task-specific action succeeds. Include first/cached visits, redirects and history navigation. |
| Search | Final input / physical Enter | Correct current-query local and remote suggestions selectable; separately time Enter through usable results. Keep full new-tab-to-result task time. |
| Settings | Open action | Requested section and all controls presented, focus/input work. Separately exercise filtering and a reversible setting with persistence verification. |
| Conversation typing | Input events | Every expected character/caret presented without loss. Cover after idle, after wake, and while chunks arrive. |
| Conversation scrolling | Wheel/gesture | Corresponding presented movement; separately reach a fixed transcript target. Preserve scroll anchoring during stream. |
| Open conversation | Select from home | Correct identity/transcript presented and composer usable. |
| Switch conversation | Select another conversation | Correct replacement identity/transcript, independently preserved drafts and usable composer. |
| Reply streaming | Controlled chunk arrival | Corresponding text presented; retain chunk-to-presentation latency, stalls, final text and completion separately from delivery cadence. |
| Video/audio | Play action | First correct decoded media visibly/audibly presented. Record rebuffering, dropped frames, seek and steady-state stalls separately. |
| Download | Actual download action | Correct file initiated and complete bytes durably present; verify hash/size, redirect, collision and cancellation/resume behavior. Separate initiation from transfer throughput and disk completion. |

## Conversation fixture requirements

Use synthetic data only: eight conversations with deterministic long transcripts, code blocks, editable drafts, home/open/switch actions and fixed ordered streamed chunks. Every run uses a fresh token. Reject stale generations and preserve the old conversation's draft when switching during a stream. Use bounded event records; diagnostic logging must not become per-character synchronous I/O. No authenticated conversations or automatic real-site prompt submission are required to prepare this fixture.

Server records establish delivered work and page acknowledgements establish semantic completion only. Animation-frame timestamps are DOM/render scheduling proxies. Pair them with independently observed presentation and input evidence before making user-facing speed claims. Capture failure is an evidence gap, not a passing sample.

The ordinary loopback fixture server now serves `/conversation?token=UNIQUE_TOKEN`. It provides eight 120-message transcripts, code blocks, independent drafts, home/open/switch actions and 64 ordered reply chunks scheduled at 40-ms intervals. Actual delivery timestamps are retained in the server's stream receipt; scheduled cadence is not assumed to equal delivery cadence. The visible status includes action and nonce for semantic checks. `Check draft` compares the visible composer with its retained state; it does not measure typing latency.

`node --test --test-concurrency=1 benchmarks/scripts/conversation-fixture.test.mjs` currently passes nine transport/state/availability regressions. Each feature group was observed failing before implementation. These small Node correctness checks ran while the Cargo lane was occupied; no timing result from them is performance evidence. The page/client still need real DOM and ordinary Bowser GUI verification before use in accepted samples. The full application suite has not yet run against the combined changes.

## Stability gate

Repeat isolated browser-process loss on baseline and candidate, then separately cover renderer exit/unresponsiveness, GPU/utility recovery, allocation failure, layered upload failure, chrome navigation/readiness failure, and close/late-completion races. Establish visible native controls and actual close operation after failure. A process with `Responding=true` does not establish usable surfaces. Do not inject faults into the user's browser/profile.
