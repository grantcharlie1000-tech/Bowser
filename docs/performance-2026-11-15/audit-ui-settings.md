Read-only audit complete for `440575e0870fd35c257d4195ec990d3c02b837aa`. HEAD matched the requested baseline; the checkout remained clean. No edits, builds, tests, benchmarks, GUI sessions, authenticated-site access, or child agents were performed.

**No measured speedup is established. This audit cannot pass a 50% target or provide council scores.**

1. **Highest-priority new-tab finding: changing blank-tab identity forces unnecessary start-page rebuilding.**

   Call chain: `src/app/tab_features.rs:127` → `App::new_tab` → `src/win32/window.rs:1844` → `src/win32/split_ui.rs:580` → `src/win32/start_ui.rs:159`.

   At `start_ui.rs:177–182`, a different tab sets `dirty = true`. At `198–210`, that forces `service::page`, even if settings and file stamps match the existing cached page.

   The synchronous rebuild includes bookmarks/config/widgets (`src/start_page/service.rs:109`), optional full history processing (`117`), privacy statistics (`122`), favicon lookup (`97,133`), and thumbnail reading/encoding (`150–154`). Thumbnail messages are then posted in a loop before the host is shown (`start_ui.rs:227`; `split_ui.rs:603–623`).

   **Minimal fix:** separate “different tab/reselect default group” from “underlying data changed.” Reuse the existing cached page when its actual inputs match, retaining the appearance-time freshness check. Consider moving remaining cold reads off the UI thread only after measuring their contribution.

   **Regression checks:** switching between blank tabs; reopened groups; external bookmark/widget/history changes; guest/private isolation; failed posts and retries; thumbnail freshness; start-page versus homepage/custom-URL/blank settings. Reusing the existing cache need not introduce a second cache.

   **Limits:** first-use controller startup remains. The first start page also has an explicit 1,500 ms process-age gate (`start_ui.rs:51–54`), while hidden internal hosts close after 180 seconds (`window.rs:140–142,1880–1914`). Removing either policy can increase startup contention, RAM or battery cost; neither is a free improvement.

2. **Highest-priority ongoing-content hypothesis: foreground memory trimming can persist after activity resumes.**

   A loaded foreground page requests `Low` after three seconds without system input (`src/app.rs:548–594`). Entering `Low` returns `IdleTimerAction::Kill`; returning to `Normal` requires another evaluation with recent input.

   The combined timer can remain killed when there are no background suspension/sleep/discard candidates (`app.rs:7486–7514,7598–7625`). Foreground restoration explicitly reevaluates the policy only when the window *gains* foreground (`src/win32/window.rs:2415–2419`). The inspected input paths do not supply an equivalent ordinary in-page activity notification.

   Thus, a single loaded tab can remain targeted `Low` during passive reply streaming and subsequent activity until another lifecycle event restores/rechecks it. **Whether this causes ChatGPT’s lag is unmeasured.** Actual application of the memory target must be checked separately from the requested state (`app.rs:7865–7870`).

   **Minimal investigation/fix:** trace requested/effective memory target through a foreground idle→typing/scrolling/streaming sequence. If confirmed material, introduce a narrowly scoped activity-restoration mechanism rather than indefinite polling or blanket disabling memory management.

   **Risks/tests:** RAM, CPU wakeups and battery may increase. Test sustained foreground activity, passive streaming, minimization, background windows, unsupported APIs, media/downloads and memory pressure. These resource effects must be reported separately and cannot be hidden behind speed gains.

3. **Settings opening and saving perform repeated synchronous disk work on the UI thread.**

   `settings_page::open` (`src/win32/settings_page.rs:137`) opens the internal tab; `sync` waits for bridge readiness, then prepares snapshots before showing the container (`444–543`).

   One dirty refresh reads settings independently through:

   - Main snapshot: `settings_page.rs:559`.
   - Address/search/bookmark snapshot: `911`, plus `BookmarkStore::load` at `916`.
   - Page-tools snapshot: `994`.
   - Protection snapshot: `html_dialog.rs:420` → `protection.rs:63`, plus blocker registry loading at `77`.

   Profile metadata, custom-search configuration and default-browser status are also gathered before show (`settings_page.rs:565–568,604`). String equality checks happen **after** these reads and serialization.

   Each ordinary settings control change serializes the full draft (`ui/src/settings-page.ts:349–359`). The queued handler synchronously reads, validates, saves and applies it (`settings_page.rs:693–706`). Saving rereads the existing file and performs `write_all`, `sync_all`, and rename (`src/settings.rs:1456–1479`).

   **Minimal fix:** gather one strict settings result per refresh and pass it to dependent snapshot builders; distinguish which sections changed before rebuilding them. A later worker-backed read/save package needs ordered commands, freshness/version checks, failure propagation and acknowledgement only after successful persistence.

   **Preserve:** unknown settings keys, unreadable-file refusal, atomic/durable saves, cross-window changes, pending drafts and security-setting application order. Do not replace strict disk state with stale defaults or acknowledge an unfinished save.

   Native fallback also reads before creating/showing controls (`settings_panel.rs:2127–2172,2249–2266`); `WM_CREATE` builds controls synchronously (`2326`). Measure it separately from HTML settings.

4. **Settings renderer work is eager; search repeatedly rewrites the full settings tree.**

   `ui/src/main.ts:540` dynamically loads the settings module. `startSettingsPage` constructs the controls before registering its receiver and sending `ready` (`settings-page.ts:251–2199`). Receiving the main snapshot then applies controls and starts navigation/entrance work (`2188–2195`).

   Every search input immediately calls `filter` (`2092`), which traverses rows, rewrites highlighting, evaluates disclosures/sections and calls `paintGroups` twice (`2031–2078`). Navigation also mixes DOM mutations with geometry reads (`1895–1914,1943–1948`).

   **Minimal fix:** first eliminate unchanged label/highlight writes, precompute stable lookup data, and consolidate equivalent group updates. Frame-coalesce search only if behavior and input latency improve. Defer section construction only if profiling justifies the larger accessibility/search/deep-link risk.

   **Tests:** existing `settings-page.mjs`, `settings-navigation.mjs`, `tab-options-dom.mjs`, plus real keyboard/IME, screen-reader order, Simple/All view, deep links, focus retention, zoom/DPI and reduced-motion verification. The existing navigation tests specifically protect late-layout anchoring.

5. **Tab/chrome refreshes retain repeated scans and serialization before their unchanged-state checks.**

   New-tab/select operations post a structural tab change (`src/app.rs:1756,2038,1622`). `WM_TABS_CHANGED` invokes `WindowChrome::refresh` (`window.rs:3273–3285`), which iterates tab IDs and repeatedly fetches individual fields (`918–987`).

   HTML synchronization repeats per-tab lookups before comparing inputs (`11268–11317`). Examples of linear per-ID searches include `App::tab_favicon_key` (`app.rs:4908`) and `tab_strip_extras` (`app/tab_features.rs:489–509`). This creates quadratic collection work at larger tab counts. Toolbar JSON and theme JSON are also regenerated before send-cache comparison (`window.rs:9793–9795,9839`).

   **Minimal fix:** produce one owned tab snapshot under one borrow, index it once, and reuse it across native/HTML consumers. Introduce narrowly scoped revisions for unchanged toolbar/theme inputs. Preserve per-host generation checks and successful-send caching.

   **Persistent-site relevance:** SPA source changes call the address callback and structural refresh (`engine/webview2.rs:5295–5306`; `app.rs:6236–6237`). Title changes post a nonstructural refresh (`6240–6263`). Conversation navigation or changing titles may therefore amplify host work; actual event frequency is unknown.

   **Already optimized:** tab-change messages coalesce (`app.rs:1626`); chrome sync coalesces separately (`window.rs:1814`); painting is separated from synchronization (`12071`); toolbar-only state does not repost full state to tabs/rail (`9836`). Further blanket batching or repaint suppression risks stale UI without necessarily saving meaningful time.

6. **Panel searches reread and sort whole stores; popup construction has synchronous profile reads.**

   Flyout presentation calls `send_current` before reveal (`src/win32/html_panel.rs:382–407`). History snapshots call `search_history` (`467`), which reads and sorts the history before limiting results (`src/productivity.rs:1012–1027`). A history command first searches the old query (`html_panel.rs:573`), then can call `send_current` for another search (`592`).

   Bookmarks reload the store (`471`); Customize reloads sidebar configuration (`476`). Main-menu construction loads profile listings and metadata (`window.rs:7158,6615–6619,6647`).

   **Minimal fix:** retain a correctly invalidated read snapshot/index, avoid the old-query search for commands that do not need row resolution, and use generation-checked results for asynchronous searches. Preserve command-to-row identity and history deletion visibility.

   **Risks/tests:** stale results opening the wrong item, deleted history reappearing, cross-window changes, private/guest leakage, large histories, fast query replacement, close/reopen and focus restoration.

7. **Wake measurements currently lack trustworthy suspension completion; lifecycle changes need caution.**

   Sleeping-tab selection reaches `engine.select_tab` (`app.rs:2023`) → visibility transition (`engine/webview2.rs:1664`) → bounds/show (`1691–1693`) → `Resume` (`4753–4757`). A discarded tab instead navigates again (`app.rs:7804–7820`). The displayed sleeping predicate combines suspended and discarded states (`4900–4905`).

   Content `TrySuspend` ignores completion success (`engine/webview2.rs:4589–4592`), while state is marked suspended immediately (`4465–4466`). The existing `hidden_tab_suspended` event consequently does **not** prove a successful suspension.

   Separately, chrome-host suspension refusal calls the host failure callback (`chrome_host/host.rs:655–678`), including asynchronous completion. Rapid hide/show/resume deserves a lifecycle-race check before assuming controller recreation is unavoidable.

   **Minimal package:** explicit requested/confirmed suspension states and generation-aware completion handling. Preserve actual process-failure fallback. Measure whether any avoidable recreation or redundant operations occur before changing lifecycle policy.

   Existing bounds caches and show-before-hide ordering should remain (`webview2.rs:1681,4709`; `chrome_host/host.rs:693–713`). Hidden-window bounds invalidation fixes positioning correctness; simply removing it risks rendering and accessibility defects.

**Measurement handoff to the parent**

| Workload | Existing signal / missing completion |
|---|---|
| Settings first/warm/native | `production-baseline.ps1:348–359` times expanded host visibility, then sleeps before capture. `interaction_settings_ready` precedes snapshot application; `interaction_settings_show` precedes presentation. Add snapshot-generation application, correctly themed target controls, focus and a successful harmless interaction. |
| New tab | Separate blank/start page, configured URL, private, first host, warm host and post-idle-close. End at requested content visibly complete and the intended input accepting text; include promised thumbnails/widgets. |
| Sleeping-site wake | Separate confirmed suspension from discarded reload and ordinary hidden tabs. End at correct content presented **and** a successful interaction. Title/URL changes are insufficient. |
| Tab switch | `perf-probe.ps1:499–505` and `production-baseline.ps1:495–497` poll titles. They can finish before content presentation or responsiveness. |
| Popups/panels | Popup sizing is reported from rAF (`ui/src/menu-view.ts:196–205`), then native show occurs (`html_popup.rs:930`). Panels use double-rAF (`panel.ts:59`) but have a 300 ms reveal fallback (`flyout.rs:31`). Neither proves presented pixels or usable controls. |
| Layout/paint/input | Include final correct geometry, presented frame and successful input; cover native/HTML, split views, vertical tabs, DPI, focus transitions and accessibility. |

The production fixture’s load→double-rAF→title/HTTP acknowledgement (`benchmarks/scripts/production-fixture-server.mjs:22`) is useful semantic evidence, not display-presentation proof.

For **ChatGPT-like content**, retain five separate workloads:

| Workload | End-to-end task and diagnostic evidence |
|---|---|
| Typing | Fixed text/IME sequence through correct final visible text and caret; record per-input presentation latency and missed/reordered input. |
| Scrolling | Fixed gesture through correct settled viewport; record input-to-present delay and frame stalls. |
| Opening conversations | Activate an unopened fixture conversation through correct visible content and responsive composer. |
| Switching conversations | Switch existing conversations through correct content, restored scroll/focus and responsive controls. |
| Reply streaming | Fixed synthetic chunk schedule through complete presented output and usable controls; measure chunk-arrival-to-presentation and stalls separately from server waiting. |

Bowser-side leads are findings 2 and 5, lifecycle transitions, and optional injected features. The default login script handles password focus/submission, not every chat keystroke (`webview2.rs:586`). Reader probing is deferred and guarded (`app.rs:4680–4741`); spatial navigation defaults off (`page_tools.rs:130`); the X mutation script is restricted to X/Twitter (`blocker.rs:1555`). These are **not evidence of a general ChatGPT mutation loop**.

If Bowser Shield is installed/enabled, its mutation observer scans added element subtrees synchronously (`extensions/bowser-shield/content/shield-content.js:248–282`). It skips text nodes; large conversation insertions are a more plausible stressor than text-only streaming. Installation and contribution remain unverified. Do not disable protection to claim a win.

Website JavaScript/layout, WebView2 renderer/compositor/GPU behavior and extension costs remain independent hypotheses. Use synthetic/public fixtures without reading authenticated conversations or submitting prompts.

**Proposed disjoint ownership**

| Package | Exclusive source ownership |
|---|---|
| Start-page cache | `src/win32/start_ui.rs`, `src/start_page/service.rs` |
| Settings data/save pipeline | `src/win32/settings_page.rs`, `settings_panel.rs`, `protection.rs`, `html_dialog.rs`, `src/settings.rs` |
| Settings renderer | `ui/src/settings-page.ts`, `settings-page-logic.ts`, associated settings tests |
| Shared app/chrome work | `src/app.rs`, `src/app/tab_features.rs`, `src/win32/window.rs` — keep memory-policy and tab-snapshot changes with one owner |
| Engine/host lifecycle | `src/engine/webview2.rs`, `src/chrome_host/host.rs` |
| Flyout data | `src/win32/html_panel.rs`, `src/productivity.rs`, associated panel tests |
| Measurement/bridge contract | Parent: `benchmarks/**`, shared Rust/TS protocol files; coordinate surface acknowledgements with their owners |

These packages are candidates, not authorization to implement. Start with cache reuse and shared snapshot collection; larger asynchronous save/lifecycle changes require stronger correctness coverage.

Acceptance remains **≤50% baseline task time in each separately declared area**, with correctness and visual/accessibility parity. Use matched production conditions, alternating repeated runs, explicit failures, and distributions; keep diagnostic instrumentation separate from acceptance runs. Report RAM, CPU, battery and background activity separately. Source evidence cannot establish that removable Bowser work occupies half of every task, especially for engine/site-bound workloads or presentation-limited interactions. It also cannot substantiate the council’s unanimous stop condition.

**Skills used**

- `C:\Users\charlie\.agents\skills\benchmark-checklist\SKILL.md` — distinguished proxies from completed tasks and bounded speed claims; runtime portions deferred to the parent.
- `C:\Users\charlie\.agents\skills\principle-type-system-discipline\SKILL.md` — informed suspension-state/generation proposals and boundary validation.
- `C:\Users\charlie\.agents\skills\typescript-best-practices\SKILL.md` — reviewed settings/bridge lifecycle and parsed payload handling; skipped broad style refactoring and execution.
- `C:\Users\charlie\.agents\skills\principle-guard-the-context-window\SKILL.md` — targeted source reads and bounded handoff; no child delegation.
