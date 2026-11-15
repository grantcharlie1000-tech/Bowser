Audit complete at `440575e0870fd35c257d4195ec990d3c02b837aa`. Local HEAD and `origin/main` match; the working tree remains clean. No edits, builds, tests, benchmarks, GUI sessions, dependency changes, or child agents were performed. I found no additional checkout/ancestor `AGENTS.md` or `CLAUDE.md`; the supplied instructions governed this audit.

**No performance gain is established.** The strongest candidates concern new-tab initialization, active-page memory targeting, suspension state, and UI-thread persistence/image work. These require parent-owned measurements. This audit provides neither council scores nor a stop-condition recommendation.

**Current architecture**

| Component | Source-backed behavior |
|---|---|
| Startup | `main.rs:42–78` establishes the data root, runs updater startup, starts eligible background updating, initializes logging/COM, then enters `App::run`. The `process_start` log occurs **after** updater startup, so it omits earlier launch work. `app.rs:5055–5145` loads settings and profile stores before window creation. |
| HTML chrome/content | `win32/window.rs:9470–9488` requests the shared tab environment, then toolbar/tab-strip/rail controllers. Popup creation follows toolbar activation; flyout/hover hosts are lazy. `chrome_host/host.rs:806–816` gives chrome a separate InPrivate profile within that environment. Preserve this isolation. |
| New-tab page | `win32/split_ui.rs:451–512` selects the start-page surface and lazily ensures its host. `start_ui.rs:159–216` builds/posts its model. Content controllers are separately lazy; typing can prewarm the active tab through `app.rs:6005–6041`. |
| Hidden tabs | Hiding sets Low memory target: `engine/webview2.rs:1696–1699`. Suspension is requested after 30 seconds, or 10 in efficiency mode: `efficiency.rs:20–25,95–107`. |
| “Sleeping” tabs | Default sleep is enabled after 300 seconds: `settings.rs:667–668,119`. Efficiency caps this at 120 seconds. This path **discards the controller** after safety checks; selection recreates it and navigates to the saved URL. Suspension and discard are distinct workloads. |
| Sidebar web apps | Separate WebApps user-data environment: `engine/web_app.rs:129–154`. Opened app controllers are retained, hidden, and eligible for suspension. `web_app_lifecycle.rs` and `engine/web_app_profile.rs` provide creation/release safeguards; do not bypass them to reclaim memory. |
| Persistence | Sessions use a worker; general small-file writes use a coalescing worker; history uses delayed writes and a shared lock. Workspace serialization/write still runs from the UI path. |
| Scheduling | Win32 dispatch handles lifecycle, persistence, downloads, animation, and page-host timers. Many already stop when inactive. Power fallback is 60 seconds; theme scheduling 30 seconds; optional tab/audio tick one second. Avoid replacing these with a broad permanent polling loop. |
| Caches | Thumbnail payloads: ≤64 × 36,000 bytes, excluding transient captures/decoder memory. Favicon caches: 64 decoded entries; byte cache ≤128 entries/1 MiB. History cache: up to 16 files, each retaining up to 5,000 entries. These bounds are **not measured resident-memory totals**. |

**Prioritized findings and minimal packages**

1. **P1 — New-tab initialization contains avoidable gating and invalidation candidates.**

   Call chain: `split_ui::wanted` → `start_ui::ready` → `ensure_host`; then `split_ui::sync` → `start_ui::due` → `service::page`.

   - [start_ui.rs:51](/C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/win32/start_ui.rs:51) imposes a **1,500 ms process-age threshold** before the first start-page host can be requested (`84–98`). This is a source constant, not a measured delay or saving.
   - `start_ui.rs:177–182` sets `dirty=true` whenever the tab ID changes. `198–210` consequently rebuilds the model even if settings, requested group, and file stamps are unchanged.
   - `start_page/service.rs:109–138` reloads bookmarks/config/widgets and, when enabled, history/privacy data; favicon lookup and thumbnail preparation add work. It runs on the UI path.
   - `win32/window.rs:138–142,1883–1914` closes hidden internal-page hosts after 180 seconds, making later new-tab/settings openings cold again.

   **Package:** Separate document-data invalidation from per-tab presentation reset; reuse the existing bounded model when dependency stamps and requested/default group match. Replace the fixed startup-age gate with an observed chrome-ready condition, preserving first-paint ordering and demand-driven creation.

   **Risks/tests:** Preserve group reopening, acknowledgments, file-change detection, private/guest separation, thumbnails, focus, and fallback behavior. Compare first start page, repeated Ctrl+T, blank-tab switching, and reopening after host retirement. Earlier initialization could increase startup CPU/peak memory or battery use; that would disqualify an apparent speed win under the no-tradeoff requirement.

2. **P1 — A loaded foreground page can remain at Low memory target after idle.**

   [app.rs:548](/C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/app.rs:548) sets Low after three seconds without input and returns `Kill` once stable (`572–590`). The timer handler obeys that result: `win32/window.rs:4490–4503`.

   Restoration exists on foreground gain (`window.rs:2415–2419`), tab selection (`app.rs:2023–2025`), navigation completion (`7118–7125`), and window restore. **I found no direct restoration/rearming path for ordinary typing or scrolling inside an already-focused page.**

   There is also a redundant transition during selection: `engine.select_tab` applies the engine’s previous target (`webview2.rs:1691–1693`) before the caller requests Normal (`app.rs:2025`). A previously Low engine can therefore apply Low to the newly shown page immediately before Normal.

   **Package:** Represent desired versus successfully applied memory state explicitly; restore the foreground interactive target through a coalesced native activity signal, without collecting input contents. Apply the intended target once during activation. Preserve idle/minimize policy instead of globally disabling trimming.

   **Risks/tests:** This is a **high-priority hypothesis for ChatGPT lag**, not proof that Low causes it. Test idle → typing/scrolling without changing focus; passive reply streaming; audio/video; split panes; minimize/restore; unsupported/failed target APIs. Measure private bytes, resident memory, faults, CPU and energy alongside responsiveness. Restoring Normal may consume more RAM, which must be disclosed.

3. **P1 — Suspension bookkeeping records requests as completed state.**

   Call chain: `App::idle_timer_fired` → `suspend_hidden_tabs` → `TabEngine::suspend` → `try_suspend`.

   - [webview2.rs:4588](/C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/engine/webview2.rs:4588) ignores both asynchronous completion values; `4451–4467` immediately inserts the tab into `suspended`.
   - `app.rs:7200–7222` independently marks eligible tabs suspended and logs `hidden_tab_suspended`.
   - `webview2.rs:4740–4757` shows the controller before requesting Resume and ignores Resume failure.
   - Sidebar apps repeat optimistic completion handling: `engine/web_app.rs:577–593`; `has_pending_suspend` then trusts the boolean (`427–430`).
   - `chrome_host/host.rs:654–678` does inspect completion, but treats refusal as host failure. A suspension refusal merits separate handling from a broken controller.

   **Package:** Use a bounded lifecycle state such as awake / suspend-pending / suspended / resume-failed, carrying controller/document generation. Update it from actual completion, reject stale callbacks, and define bounded refusal/retry behavior. Keep visibility, memory target, and discard eligibility distinct.

   **Safety constraint:** `webview2.rs:4420–4445` deliberately resumes and probes before discard; `app.rs:7676–7703` rechecks epoch, timeout, eligibility and audio. Preserve these checks. Do not automatically re-suspend a page whose probe found drafts/media.

   **Risks/tests:** Late suspend completion after selection, refused/unsupported suspension, synchronous failure, closing/replacing a controller during callbacks, draft/media/PiP/capture/private tabs. Existing tests at `app.rs:8778,8824` and `webview2.rs:7959,7975` provide relevant anchors, but were not run.

4. **P1 — Discard wake pays controller creation and navigation; controller requests have queueing risk.**

   `app.rs:7778–7820` closes the controller, clears usability/back-forward state, then restores only the committed URL. Back/forward history is explicitly not restored. This is existing baseline behavior, not an acceptable new optimization.

   `webview2.rs:1857–1865` serializes controller creation. `1800–1827` prioritizes an active prewarm request, otherwise selects queued requests FIFO; an already queued foreground navigation has no explicit priority over older background requests.

   **Package:** Keep the concurrency cap and introduce typed request intent: visible navigation/wake, other visible pane, background navigation, speculative prewarm. Reevaluate queued priority when selection changes; preserve pending POST ownership and stale-controller cancellation.

   **Risks/tests:** No starvation of explicitly requested background work; correct POST destination/body; rapid tab open/close/select; split/private isolation; failure progression. This helps only when queue delay is material. Retaining every controller or extending sleep delays would trade memory/battery for speed and is not a free fix.

5. **P2 — Background persistence can still block foreground UI work.**

   - [productivity.rs:902](/C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/productivity.rs:902): readers acquire `HISTORY_IO`.
   - `650–675` holds that same lock through history writing; `1063–1074` reaches `atomic_write_with`, whose `1154` calls `sync_all`.
   - A cached history read clones all entries (`585–589`); cache replacement clones them again (`564–576`).
   - `WM_SESSION_SAVE_TIMER` → `App::session_save_now` → `save_workspaces_now` (`window.rs:4571–4574`; `app.rs:5575–5581,1456–1481`) performs workspace serialization/read comparison/write on the UI thread (`workspaces.rs:840–858`).
   - `SessionWriter` uses an unbounded channel and writes every queued snapshot (`session.rs:519–544`). Slow storage can accumulate obsolete snapshots; shutdown joins behind them.
   - `background_write.rs:57–62,81–85` serializes all paths under one write lock, so an explicit synchronous operation can wait behind unrelated files.

   **Package:** Begin with removing redundant snapshot copying and bounding/coalescing pending session snapshots. For history, separate immutable reader snapshots from durable writer serialization using generation-aware publication; preserve queued visits, external edits, clear-history ordering and write failures. Move workspace/session persistence only as a coordinated ordered unit, with error delivery preserved.

   **Risks/tests:** Lost visits, deleted history reappearing, stale external-file overwrites, session/workspace mismatch, shutdown/crash recovery. Do not remove flushes or delay durability merely to improve timing.

6. **P2 — Thumbnail processing executes inside the UI-thread capture completion.**

   `tab_features.rs:609–645` schedules capture after 1.2 seconds, with a four-second minimum gap. [webview2.rs:4347](/C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/engine/webview2.rs:4347) reads the captured stream and synchronously calls `downscale_jpeg` from the completion (`4366–4369`). `thumbnail.rs:54–85` creates WIC objects, decodes, scales, and encodes there.

   **Package:** Keep WebView capture/UI ownership on its apartment; move owned-byte image processing to one bounded worker with its own COM initialization. Return generation-tagged results. Preserve image dimensions/quality and private-tab exclusion.

   **Risks/tests:** Transient memory grows if work queues are unbounded; stale navigation images; closed tabs; completion ordering; no visual downgrade. This is a plausible short interaction stall around tab/page activation, not evidence of continuous ChatGPT lag.

7. **P2/P3 — Narrow scheduling waste is preferable to a broad scheduler rewrite.**

   - `app.rs:7528–7558` schedules sleep rechecks for hidden HTTP(S) tabs without excluding private tabs, although `409–416` categorically excludes private tabs from discard. This can keep a 30-second recheck alive for work that cannot happen.
   - Sidebar reload polling reloads the preference file every 30 seconds (`panel_webapp.rs:101,138–150`), including when configured apps have no loaded view.
   - `win32/address_bar.rs:160–184` creates a sleeping thread per uncached suggestion request. Generation cancellation happens after the debounce; there is no post-network generation check before enqueuing the result.
   - `blocker.rs:1191–1201` supersedes a build receiver but does not cancel computation already running in the old worker.

   **Package:** Align timer eligibility with the real lifecycle; schedule the next meaningful deadline; use latest-request bounded workers where measured bursts justify them. Maintain suggestion privacy gates and blocker rule continuity. Lower wakeup counts alone do not satisfy any task-time target.

**Disjoint ownership for later implementation**

These are proposed reservations, not permission to start work:

| Owner/package | Exclusive files |
|---|---|
| Parent integration/lifecycle | `src/app.rs`, `src/engine/mod.rs`, `src/engine/webview2.rs`, `src/win32/window.rs`, `src/performance.rs` |
| Start-page model | `src/win32/start_ui.rs`, `src/win32/split_ui.rs`, `src/start_page/service.rs` and its tests |
| Persistence | `src/productivity.rs`, `src/session.rs`, `src/background_write.rs`, `src/workspaces.rs` |
| Thumbnail processing | `src/thumbnail.rs`, `src/app/tab_features.rs`, relevant tab-flow tests |
| Sidebar lifecycle | `src/engine/web_app.rs`, `src/web_app_lifecycle.rs`, `src/win32/panel_webapp.rs` |
| Suggestion scheduling | `src/win32/address_bar.rs` |
| Measurement | Parent-owned `benchmarks/*`; runtime execution exclusively with parent |

Packages requiring shared-file hooks provide interface proposals to the integration owner. Do not concurrently edit the shared files. Cosmetic/blocker changes belong with the corresponding audit owner after profiling.

**ChatGPT responsiveness: attribution limits**

All five reported actions remain separate: **typing, scrolling, opening conversations, switching conversations, reply streaming**.

Bowser-side candidates include findings 2, 5 and 6, background controllers, and enabled extensions. Injection amplification needs a narrower hypothesis:

- `webview2.rs:5163–5204` installs cosmetic CSS at both ContentLoading and DOMContentLoaded. `895–900` assigns `textContent` even when unchanged. Avoiding identical reassignment is a small candidate, provided missing/modified styles are still repaired.
- Cookie CSS can include many generic selectors (`blocker/cookies.rs:21–30`). Style recalculation on a large changing DOM is an **engine-side cost influenced by Bowser CSS**, requiring a style trace.
- The mutation observer in `blocker/x_promoted.js:79–83` is X/Twitter-specific (`x_promoted.rs:18–21`); it is not source evidence for a ChatGPT observer problem.
- Forced-dark overrides are conditional (`webview2.rs:1031–1044`). Their rendering cost is unknown.
- Website JavaScript, DOM size, layout, allocation/GC, renderer/compositor work and server pacing remain independent possibilities. Browser private bytes do not establish JavaScript heap size or a GC cause.

Use a bounded synthetic conversation fixture with fixed seeded transcripts, a capped DOM, deterministic chunk delivery, editable input, scroll targets, and SPA conversation selection. Include small/long transcript sizes and streaming concurrent with typing/scrolling. Record exact node/message/chunk counts and content checksums. Separately validate the real site through a parent-controlled, privacy-preserving session; this audit accessed no authenticated chats and submitted no prompts.

**Existing harness assessment**

The best starting point is [production-baseline.ps1](/C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/benchmarks/production-baseline.ps1:421), but it does not cover the requested acceptance matrix.

Useful foundations:

- Alternating arm order, fresh/reused profiles, executable hashes and process-tree drain checks.
- `production-measurements.psm1:5–41` correlates fixture token, tab/navigation identity and successful navigation; rejects stale/superseded readiness.
- `measurements.psm1:272–404` records native working set/private usage, PID creation identity, roles and coverage errors.
- Negative fixtures and validation tests cover failed navigation, redirects and PID reuse.

Gaps:

- Defaults identify older commits (`production-baseline.ps1:12,39`); candidate mode uses `DiagnosticExe` as its baseline (`74`). Pin actual binaries/source independently to this baseline.
- Resource mode samples in the readiness loop and sleeps 250 ms by default (`35–38,472–476`). It cannot resolve short interaction improvements. Timing mode is separate, but polls at 10–20 ms.
- Fixture completion is load + two animation callbacks + HTTP acknowledgment (`production-fixture-server.mjs:22`), **not verified frame presentation or successful interaction**.
- Optional tab switching times title propagation (`production-baseline.ps1:493–497`), without proving suspension, resume or usable content.
- `run.ps1:629–630,720` explicitly excludes provider response from blank-submit. That is not search completion.
- `launch-to-usable.ps1:225–228` substitutes `NavigationCompleted` for real-site usability.
- Production harness metadata records hardware and acknowledges noise, but does not supply per-sample ambient machine-load accounting.
- Endpoint process sampling misses short-lived processes; hard faults/page-in bytes are unavailable. Working-set sums double-count shared pages.
- `production-analyze.py:65,91` contains an old 0.80 descriptive ratio and a ten-pair primary gate. Neither establishes this task’s 0.50 acceptance threshold.
- Diagnostic logging accumulates bytes in memory (`performance.rs:741–757,849–854`); prolonged detailed logging can itself distort RAM/latency.

**Future baseline contract — design only**

For every row below, freeze an explicit action and completion contract. Use external QPC timing, action/document identities, semantic correctness evidence, and separately validated presentation evidence. Title changes, API return, DOM acknowledgment and animation callbacks remain intermediate markers.

| Area/workload | Primary task timing | Required completion/work checks |
|---|---|---|
| Startup | Process creation → intended initial surface usable | Expected chrome/content rendered; focus and one deterministic interaction work. Separate empty, launch-URL and restored-session cases. |
| New tab | Ctrl+T/click → configured new-tab surface usable | Correct content/widgets/thumbnails, focus and functional interaction. Separate first host, retained host, retired host and URL/homepage configuration. |
| Suspended-site wake | Selection input → retained document usable | Confirmed suspension beforehand; successful resume; same document/state; correct rendered content and interaction. |
| Discarded-site wake | Selection input → reloaded document usable | Confirmed controller absence; creation/navigation success; correct page and available state. Never pool with suspended wake. |
| Page load | Navigation action → target content usable | Request reached server, successful final document, expected content/assets and action response. |
| Search | Search submission → useful result list usable | Exact query/provider request, successful results, correct query echo and usable result links. |
| Settings | Open action → requested section usable; separately apply → persisted/applied result | Correct controls, focus, value round-trip and reopened value; distinguish first/reused/retired host. |
| Media streaming | Play action → first presented audiovisual playback; seek separately | Correct resource/position; sustained playback, dropped frames, buffering and A/V continuity as guards. |
| Downloads | Download action → complete correct file available and UI completion | Server bytes, successful operation, final size/hash, no interruption; verify openability. |
| Chat typing | Fixed input burst → correct final text visibly presented | No dropped/reordered characters; per-key input-to-presentation distribution; fixed cadence. |
| Chat scrolling | Fixed scroll action → correct target viewport presented | Correct offset/anchor/content; frame pacing and missed-frame distribution. |
| Open conversation | Selection → chosen transcript rendered and interactive | Correct conversation identity/content/count; input works. |
| Switch conversation | A→B action → B rendered and interactive | No stale A content; draft/scroll-state behavior preserved; repeated A↔B cycles. |
| Reply streaming | Start action → complete correct reply rendered | Fixed chunk schedule/content, first-token and per-chunk presentation delays, final checksum; typing/scrolling remain responsive. Separate server delay from rendering backlog. |

Run **at least five valid alternating A/B pairs per workload and condition**, reporting all attempts, failures, median, min/max and paired ratios. Retain the existing ten-pair gate for primary claims where that analysis workflow is reused; gather more when variability prevents a decision. Require a separately supported ≥50% reduction for every agreed primary task metric; do not average away a failing area.

Controls and instrumentation:

- Freeze release configuration, UI mode/assets, runtime version, extensions, protection settings, profile seed, viewport/DPI, power mode and network fixture. Record actual executable/runtime hashes and launch path.
- Separate fresh profile, seeded profile restart with drained processes, retained environment, suspended document and discarded document. Fresh profile is not cold OS cache.
- Measure normal installed-update behavior separately from local/dev launches: `updater.rs:1992–1995` suppresses background checks for local/dev/benchmark launches.
- Record machine CPU/per-core utilization, memory pressure, disk/network activity, GPU load, power source and thermal/power state for every pair. Include harness/fixture-server load.
- Inventory Bowser plus all owned WebView2 browser, renderer, GPU, utility, extension and sidebar processes, tracking PID **and creation time**, including surviving helpers. Report Bowser and child totals separately.
- Report working set, private bytes, process counts, phase samples and sampled peaks with coverage labels. Add faults/page-in and energy evidence for diagnosing trim/wake costs. CPU seconds are not battery-energy measurements.
- Never sum hover-card tab-memory values: `webview2.rs:682–725` matches frame URLs and assigns each tab the **maximum** matching renderer working set. It is neither whole-tree accounting nor private memory.
- Add generation-tagged suspend request/completion/refusal, resume result, controller queue wait, target request/result, first interactive acknowledgment, persistence-lock wait, thumbnail processing, and task presentation markers. Keep detailed profiling separate from acceptance runs; measure observer overhead.

The feasibility limit remains explicit: if unchanged network, server, decode or fixed stream pacing already consumes more than half a task, optimizing Bowser-only work cannot halve that task without another viable change. Likewise, raising memory residency, suppressing protection, reducing visual work, omitting content or changing timing endpoints cannot qualify. No source inspection here establishes that every 50% target is attainable—or that further savings are exhausted.

**Skills used**

- `C:\Users\charlie\.agents\skills\benchmark-checklist\SKILL.md` — harness critique and paired-baseline design; execution/reporting steps deferred to parent.
- `C:\Users\charlie\.agents\skills\principle-explain-the-number\SKILL.md` — required measured limiter, actual work, spread and accounting before claims; no gains inferred.
- `C:\Users\charlie\.agents\skills\principle-model-the-domain\SKILL.md` — lifecycle/request/generation proposals instead of additional synchronized booleans; no refactor performed.
- `C:\Users\charlie\.agents\skills\principle-guard-the-context-window\SKILL.md` — scoped source reads and bounded handoff; no delegation.
