Audit complete for clean HEAD **`440575e0870fd35c257d4195ec990d3c02b837aa`**, matching the supplied `origin/main` baseline. No files changed, builds, benchmarks, GUI sessions, authenticated chats, or child agents used. Findings below establish source behavior; **all performance gains remain unmeasured**. This is not a council review or a substantiated stopping decision.

**Prioritized findings**

1. **P1 — Sleeping-site measurements must distinguish suspension from discard; discarded wake repeats new-page setup.**

   The selection chain is `App::select_tab` → engine selection/visibility → `restore_discarded_tab` at [app.rs:2023](C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/app.rs:2023). A retained controller follows `apply_visibility` → resize/show → `Resume` (`src/engine/webview2.rs:1664–1700,4740–4758`). A discarded tab instead recreates its controller and navigates to its committed URL; back/forward history is explicitly not restored (`src/app.rs:7778–7820`).

   New-controller setup calls `configure_controller` before pending navigation (`src/engine/webview2.rs:1992,2100`). Its blocker setup regenerates request filters and prepares renderer rules ([webview2.rs:5851](C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/engine/webview2.rs:5851)). `BlockerState::request_filters` recomputes candidates/minimization; renderer patterns are cached in part, but `for_page` still clones retained strings and the engine serializes them again (`src/blocker.rs:787–796,936–943,1574–1591`; `src/engine/webview2.rs:1065–1072`). Registration then makes one COM call per filter per blockable context (`:818–856`).

   **Minimal candidate:** cache immutable request-filter plans by rule generation, then consider bounded caching of serialized renderer policy by effective policy/domain. Keep per-controller installation and all security initialization. This targets new-site tabs and discarded wake; a simple suspended wake does not repeat controller configuration. Blank/start-page readiness needs its own measurement—the engine already keeps controller creation lazy (`:1662–1664`).

   **Risks/tests:** invalidation after list/level/private-policy changes; first-party and public-suffix exceptions; case-sensitive URL-filter differences; redirects; older WebView2 fallback; protection on the first request. Cache growth is a potential RAM regression. Do not propose keeping tabs alive or speculative network preloading as a free speed improvement.

2. **P1 correctness gate — Sleeping can destroy resumable downloads or stalled media state.**

   Before discard, `check_sleep_blocked` checks `owner_downloading` ([webview2.rs:4409](C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/engine/webview2.rs:4409)). That predicate accepts only `IN_PROGRESS` operations (`src/engine/download.rs:411–418`). The operation registry deliberately retains paused and resumable-failed operations (`:259–267`), but they therefore do not protect their tab from discard. Discard closes the controller and calls `interrupt_owner` (`src/engine/webview2.rs:4297–4303`), cancelling retained operations and reporting an unrecoverable interruption (`src/engine/download.rs:375–402`). Microsoft documents paused downloads as `INTERRUPTED`, rather than `IN_PROGRESS`. [Download-operation reference](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2downloadoperation?view=webview2-1.0.4314.35).

   Separately, the page sleep probe protects an unpaused media element only when `readyState > 2` (`src/engine/webview2.rs:4543–4548`). A silent/muted stream waiting for data at lower readiness can miss both this check and the audio check, making discard possible once the remaining conditions pass (`src/app.rs:7683–7703`).

   **Minimal candidates:** treat retained resumable operations as controller-lifetime dependencies; preserve playback intent during buffering, independent of currently available media data. Keep fresh page probes, stale-callback checks, private restrictions, and fail-closed handling.

   **Required tests:** pause download → hide → sleep deadline → resume → exact final hash; resumable network interruption; unknown operation state; muted buffering video; audio starvation; PiP; live `srcObject`; activation/navigation during the asynchronous probe. Keeping affected tabs alive may increase RAM/background energy relative to the defective behavior; report it explicitly.

   Suspension state is also only a request hint today: callbacks ignore success (`src/engine/webview2.rs:4589–4592`; `src/engine/web_app.rs:577–593`). The panel sweep gates on audio only (`src/engine/web_app.rs:407–419`). Record requested, confirmed, refused, resumed, and discarded states separately. WebView2 describes suspension as best-effort and automatically resumes visible views. [Lifecycle reference](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_3?view=webview2-1.0.4022.49).

3. **P1 — Download initiation and completion reporting perform synchronous filesystem work on the UI thread.**

   The complete chain is:

   - `DownloadStarting` → `handle_starting` → synchronous plan (`src/engine/download.rs:93–110`).
   - Plan → known-folder resolution, directory creation/stat, collision checks ([app.rs:4203](C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/app.rs:4203); `src/downloads.rs:446–475`). Collision selection can probe the initial name plus 9,999 alternatives.
   - `begin` emits `Started` before returning (`src/engine/download.rs:238`).
   - `App::on_download_event` → manager event → synchronous `save` for state changes (`src/app.rs:4227–4241`; `src/downloads.rs:802–825`).
   - Save rereads/merges history, serializes, and calls `atomic_write` (`src/downloads.rs:1285–1324`), which writes, `sync_all`s, and performs a write-through replacement ([productivity.rs:1148](C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/productivity.rs:1148)).

   This can delay initiation, completion notification, and unrelated UI work. It is **not evidence that Bowser limits bulk network throughput**.

   **Minimal packages:** first reduce duplicate history reads/parsing within a correctly serialized transaction. Then evaluate a deferral-based asynchronous destination planner and ordered persistence worker, leaving COM calls on their owning thread. Preserve existing durability guarantees; merely postponing writes would introduce a correctness tradeoff. Microsoft provides a download-start deferral and warns that an existing destination can be overwritten. [Download-start reference](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2downloadstartingeventargs?view=webview2-1.0.4022.49).

   **Risks/tests:** multiwindow history merging, ID collisions, destination races, simultaneous Save dialogs, disk-full/read-only destinations, crash/restart durability, shutdown during pending work, private cancellation, guest nonpersistence, Store-placeholder handling. The existing writer uses a fixed `.tmp` name; concurrency must be designed rather than added around it.

   Progress already has a 250 ms engine throttle and another UI throttle; state changes bypass progress throttling (`src/engine/download.rs:202–225,334–339`; `src/win32/downloads_ui.rs:43–44`). A faster progress animation would not prove faster downloading.

   A secondary UI cost is completed-file checking: bubble/page snapshots call `downloads_check_files`, which stats every completed entry, at most once every two seconds (`src/app.rs:4296–4323`; `src/downloads.rs:1130–1149`; `src/download_shell.rs:47–48`). Move snapshot refresh work off-thread only with revision-safe results and fresh validation before Open/Show.

4. **P1 for video startup — YouTube document/player rewriting introduces a whole-response dependency.**

   The production patterns include the `/watch?...` **document**, plus player JSON ([youtube.rs:44](C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/blocker/youtube.rs:44)). They pause at response stage (`src/engine/webview2.rs:937–950`), then request the response body, decode it, strip ad fields, base64-encode a replacement, and fulfill or continue (`:1193–1235,1272–1323`).

   Therefore the watch-page path can postpone parser access while the body is obtained and rewritten. The 16 MiB limit is checked **after** decoding/materializing the response (`:1193–1202`). Parsing/copying runs in the completion callback. Settings metadata is also checked on relevant navigation/player responses (`src/blocker/youtube.rs:116–137`). Media segment bodies are not handled by this rewrite path; its exact endpoints are restricted (`:75–81`).

   **Minimal candidates:** reduce parsing/copying and repeated policy lookup; move substantial pure transformations off the UI thread through a bounded, generation-safe completion mechanism. This may improve concurrent responsiveness without materially reducing media startup. Removing watch-page rewriting or ad protection is not an acceptable optimization.

   **Risks/tests:** unchanged non-ad player data, byte-correct media URLs, duplicate response headers and cookies, UTF-8/base64, compressed/chunked bodies, malformed/oversized responses, cancellation, redirects, rapid navigation, live-player endpoints, and every error path releasing its paused request. A streaming replacement architecture is a larger project, not a justified minimal fix yet.

5. **P2 — Non-YouTube pages enable Fetch with empty patterns; workers retain UI-thread request interception.**

   Non-YouTube `response_patterns` returns empty (`src/blocker.rs:1604–1610`), yet each changed filter signature calls `Fetch.enable` unconditionally (`src/engine/webview2.rs:1479–1484`). A bounded candidate is explicit Fetch-state management: avoid enabling it when unnecessary and safely disable after leaving its applicable workload. Retain network blocking and outstanding-request completion.

   This is an **engine-overhead hypothesis**, not evidence of paused ChatGPT response chunks. The inspected upstream Chromium implementation creates an interceptor and updates loader factories on enable, even with an explicit empty pattern list; the installed WebView2 implementation and cost remain unverified. [Chromium Fetch implementation](https://chromium.googlesource.com/chromium/src/%2B/8b96632ef93385dbf3039bb44b9447240102dbd9/content/browser/devtools/protocol/fetch_handler.cc).

   Service/shared-worker requests intentionally use wildcard interception ([webview2.rs:1541](C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/engine/webview2.rs:1541)); contexts include media, fetch/XHR, and EventSource (`:1565–1573`). Each matching request enters the host matcher (`:5876–5929`). Large custom filter sets can also trigger wildcard fallback (`src/blocker.rs:936–943`).

   **Limits:** this is per-request handling, not a body-copy loop per SSE token. Long-lived streams and many short requests must be measured separately. Removing worker interception would lose protection; narrowing it needs proof that every applicable rule remains covered.

6. **P2, conditional — Shield’s DOM fallback can add work during streamed conversation rendering.**

   The packaged extension injects into HTTP(S) frames (`extensions/bowser-shield/manifest.json:40–45`). When operating in DOM mode, it installs an observer; network mode stops that scrubber (`content/shield-content.js:388–402`). Added element subtrees are synchronously searched and matched ([shield-content.js:248](C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/extensions/bowser-shield/content/shield-content.js:248)). Notification batching at `:205–221` does **not** batch that scanning.

   The observer ignores character-data changes and text nodes, and watches only selected URL attributes plus child additions (`:249,277–282`). Consequently plain text-node token updates are a weaker suspect than Markdown subtree replacement, images, links, or conversation-tree mounting.

   **Minimal candidate:** deduplicate overlapping added subtrees within the same callback and avoid provably redundant URL decisions, with invalidation for attributes, base URI, policy, and site context. Do not delay enforcement or remove cosmetic rules.

   Installation and DOM-fallback use are **unverified**. No claim that this extension caused the reported ChatGPT lag is justified. Built-in X mutation scanning is domain-gated (`src/blocker.rs:1555–1557`); the native login script uses focus/submit events, not a token observer (`src/engine/webview2.rs:586`). Same-policy conversation URL changes return early from blocker preparation (`:1461–1463,5295–5306`).

**GPU, lifecycle, and trusted-boundary constraints**

The audited environment constructors use default WebView2 options (`src/engine/webview2.rs:3291`; `src/engine/web_app.rs:151`). No source override for GPU disabling or autoplay policy was found. Actual GPU acceleration, codec/DRM support, runtime version, driver behavior, policy/environment overrides, decode saturation, site buffering, and CDN bandwidth require parent-owned runtime evidence.

Hidden views receive a low-memory target (`src/engine/webview2.rs:1696–1699`; `src/engine/web_app.rs:564–570`). Efficiency mode changes suspend/sleep timing (`src/efficiency.rs:21–25,95–106`); host EcoQoS applies according to minimized/efficiency state (`src/app.rs:7463–7477`). Relaxing these may improve wake latency while worsening memory or battery consumption and cannot silently count as passing “no tradeoffs.”

Preserve the actual boundary checks:

- SmartScreen enable/readback: `src/engine/webview2.rs:6207–6228`.
- Exact-origin permission decisions, private denial, and disabled engine permission persistence: `:5369–5398`.
- Host-object/message restrictions and validated login messages: `:4881–4958`.
- Private download cancellation: `src/engine/download.rs:97–99`.
- Download UI commands resolved from trusted IDs/state, not page-supplied paths: `src/downloads.rs:1155–1184`.

**Proposed disjoint ownership for later implementation**

These are file reservations for planning, not spawned assignments.

| Owner/package | Exclusive files | Responsibility |
|---|---|---|
| Core integration/lifecycle | `src/app.rs`, `src/engine/webview2.rs`, `src/engine/web_app.rs`, `src/engine/mod.rs` | Wake state, lifecycle safety, Fetch lifecycle, all adapter integration |
| Pure blocker preparation | `src/blocker.rs`, `src/blocker/registration.rs`, `src/blocker/youtube.rs`, `src/blocker/json.rs` | Cached policy plans, rewrite transforms, rule-parity tests |
| Downloads | `src/engine/download.rs`, `src/downloads.rs`, `src/downloads/tests.rs`, `src/download_shell.rs`, `src/win32/downloads_ui.rs`, `src/win32/downloads_page.rs` | Operation dependencies, destination planning, persistence, snapshot I/O |
| Optional Shield optimization | `extensions/bowser-shield/content/shield-content.js`, extension tests | Equivalent DOM processing |
| Parent measurement | Dedicated benchmark fixtures/harness | All serial runtime, GUI, resource, and task-completion evidence |

Cross-package changes to `app.rs` or `webview2.rs` go through the single integration owner. Avoid parallel edits to those files or an opportunistic rewrite of the shared atomic-write helper.

**Parent-owned measurement design**

Use production-equivalent configuration in isolated profiles. The existing production protocol explicitly distinguishes diagnostic launches from benchmark paths and acknowledges that double-RAF is not displayed-frame proof ([PRODUCTION-PERFORMANCE.md:14](C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/benchmarks/PRODUCTION-PERFORMANCE.md:14)). Existing silent-WAV lifecycle fixtures do not validate video presentation or audible playback.

Predeclare and report each workload separately:

| Workload | Reproducible fixture and successful completion |
|---|---|
| Suspended-site wake | Confirm retained controller and suspension outcome; select tab → correct content visibly presented and a deterministic interaction succeeds. |
| Discarded-site wake | Confirm controller removal; select → correct restored content and successful interaction. Record document/network reload separately. |
| New tab | Separate blank/start-page interaction from new URL navigation; validate focus, usable controls, correct page and completed action. |
| Video/audio start | Loopback HTTP Range server with fixed media; controlled delay/bandwidth and matched codec, resolution, frame rate, audio track. Click → actual first video presentation/audio output; then fixed-duration uninterrupted playback validation. |
| Streaming stalls | Deterministic segment starvation/recovery. Measure each stall and recovery to sustained presentation; check dropped frames, timeline continuity, audio continuity, quality, and completion. |
| Download initiation | Deterministic known-byte attachment; click → intended operation and first written bytes. Separate automatic destination from fixed Save-dialog interaction. |
| Download completion | Click → terminal success and complete readable file; validate exact length/hash and resume correctness. Also record terminal event → visible completion UI. |
| Download throughput | Known payloads with controlled transport limits; report bytes/time independently of UI progress. Include concurrent downloads and paused/resumed transfer. |

For **ChatGPT-like responsiveness**, use synthetic conversations, never authenticated user chats or submitted prompts:

| Distinct task | Fixture and endpoint |
|---|---|
| Typing | Long conversation, idle and streaming; ordinary text, paste, and IME → exact expected composer content visibly updated, caret preserved. |
| Scrolling | Fixed input sequence → specified message/position visibly reached; record input-to-presentation latency and frame pacing. |
| Opening conversations | Click fixture conversation → correct transcript visibly loaded and composer usable; validate message count/content checksum. |
| Switching conversations | Alternate short/long fixture conversations → correct destination, preserved draft/scroll semantics, no stale stream content. |
| Reply streaming | Fixed SSE/chunked-fetch schedule, with direct and service-worker variants; measure action-to-first rendered content, arrival-to-presentation lag, and final correct transcript. Exercise text updates and Markdown subtree replacement separately. |

For ChatGPT’s five tasks, distinguish **server emission delay**, **network delivery**, **renderer processing/layout**, **extension work**, and **host/UI contention**. A fast first token cannot substitute for responsive typing or switching. Count errors, missing/duplicated tokens, rejected inputs, incomplete work, timeouts, and failed transfers.

Use alternating baseline/candidate trials with identical production features, runtime, content, cache state, extension configuration, window geometry, and power conditions. Run profiling separately from scored trials. Report task-time distributions and uncertainty, plus process-tree CPU/GPU, RAM, disk/network work, and energy separately. Secondary live checks should use public media/download content and user-authorized representative sites; authenticated ChatGPT checks remain outside this audit.

**Feasibility bounds**

For baseline task time \(T=H+E\), where \(H\) is reducible Bowser-attributable critical-path time and \(E\) is the remainder, improving \(H\) by factor \(k\) gives:

\[
T'/T = 1-f+f/k,\qquad f=H/T.
\]

Halving total time requires \(f(1-1/k)\ge 0.5\). Thus even eliminating host overhead cannot halve a task unless it accounts for at least half of its critical path. Profiling must account for overlap and contention rather than summing unrelated component durations.

Bandwidth-bound downloads, server-paced tokens, codec/DRM initialization, and already-smooth playback may leave insufficient reducible time. Fixed-duration viewing cannot legitimately be halved by changing playback rate; zero baseline stalls also cannot show a percentage reduction. These are measurement-contract/feasibility issues for the parent and independent council—not grounds for this audit to declare targets achieved or further savings exhausted.

**Skills used**

- [benchmark-checklist/SKILL.md](C:/Users/charlie/.agents/skills/benchmark-checklist/SKILL.md): completion, correctness, limiter, repeatability, and end-to-end evidence design. Execution steps skipped under audit-only restrictions.
- [principle-boundary-discipline/SKILL.md](C:/Users/charlie/.agents/skills/principle-boundary-discipline/SKILL.md): preserved security/network/filesystem boundaries and separated pure transformations from adapters. No validation was removed.
- [principle-guard-the-context-window/SKILL.md](C:/Users/charlie/.agents/skills/principle-guard-the-context-window/SKILL.md): targeted source reads and bounded handoff; no delegation used.
