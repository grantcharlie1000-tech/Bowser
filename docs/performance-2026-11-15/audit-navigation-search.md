Baseline verified locally: `440575e0870fd35c257d4195ec990d3c02b837aa`. This is a source audit, **not a council verdict**. No builds, GUI sessions, benchmarks, edits, or children were run. A concurrent untracked `docs/performance-2026-11-15/` directory appeared during inspection; I left it untouched.

**Prioritized findings**

1. **Separate suspended-tab wake from discarded-tab reload.**  
   A retained controller is shown and receives `Resume()`; a discarded controller is closed, recreated, configured, and navigated to its last committed URL. These are materially different tasks. See [app.rs:7778–7820](C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/app.rs:7778) and [webview2.rs:4740–4757](C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/engine/webview2.rs:4740).

   Content-controller creation permits one outstanding request; subsequent requests queue. Selection does not generally reprioritize the navigation queue, while an eligible active prewarm is selected before that queue (`webview2.rs:1800–1831, 1857–1865`). A foreground discarded-tab wake can therefore inherit unrelated creation delay.

   **Candidate:** explicit foreground-demand priority, cancellation of obsolete queued work, and cheaper repeated configuration. Preserve private-profile assignment, pending POST bodies, latest navigation, close races, and first-request protections.  
   **Hypothetical ceiling:** removable queue wait plus redundant configuration on the critical path. Retained resume has no controller-creation saving; discarded reload still includes WebView2, network, server, and site work. Keeping discarded pages resident or speculative navigation would introduce resource/privacy tradeoffs.

2. **First start-page completion has a deliberate 1.5-second gate.**  
   [start_ui.rs:51–98](C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/win32/start_ui.rs:51) prevents the first start-page host from starting before that process age. `split_ui.rs:468–475` applies the gate. This affects launch-to-complete-start-page and very early new tabs, not ordinary later tabs.

   The start page reuses the split/private-landing host. Hiding it requests suspension; showing it resumes it (`split_ui.rs:506–549`). Snapshot rebuilds synchronously load bookmarks, widgets, optional history/privacy data, favicons, and thumbnails (`start_ui.rs:195–216`; `start_page/service.rs:96–158`).

   **Candidate:** replace elapsed-time gating with actual prerequisite readiness; prepare immutable page data away from the UI thread with generation-safe installation. Preserve complete widgets, images, theme, focus, and private landing behavior.  
   **Hypothetical ceiling:** the remaining portion of the 1,500-ms gate, plus blocking snapshot work. Removing the gate can also increase startup contention, so a blank surface or delayed toolbar cannot count as success.

3. **Persistent responsiveness: investigate a missing active-page memory restoration path first.**  
   [app.rs:572–594](C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/app.rs:572) requests `Low` memory after three seconds without input and kills the idle timer. The timer samples `GetLastInputInfo` (`window.rs:4490–4503`). Restoration exists on foreground regain, tab selection, and navigation; I found no equivalent ordinary typing/wheel-input restoration in an already-focused page. Other tab timers may incidentally revisit the decision.

   **Hypothesis:** a large active page remains at `Low` while typing, scrolling, or streaming, causing reclaim/paging stalls. This is a code-backed lifecycle concern, **not proof of the ChatGPT slowdown**.

   **Candidate:** an event-driven activity transition that restores the active controller promptly, including wake ordering. Avoid permanent polling or disabling background memory policy.  
   **Hypothetical ceiling:** stalls actually attributable to this policy. RAM, page faults, CPU, and battery must be measured separately; restoring working memory may increase active RAM.

4. **Navigation/configuration: repeated blocker preparation and UI-thread request interception are plausible costs.**  
   [webview2.rs:818–856](C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/engine/webview2.rs:818) registers each filter across eight resource contexts. `Blocker::request_filters()` rebuilds candidates and performs bounded subsumption work per request; importantly, the **256-candidate wildcard fallback happens before minimization** ([blocker.rs:936–943](C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/blocker.rs:936)). Shared/service-worker requests retain wildcard interception (`webview2.rs:1541–1561`), and matching runs synchronously in the host callback (`5876–5929`).

   **Candidates:** cache immutable filter plans by rule generation; investigate minimizing before applying the fallback threshold; measure callback queue time and matcher cost. Preserve case handling, wildcard/anchor semantics, worker coverage, redirects, exceptions, and private blocking. Never remove interception merely to improve timing.  
   **Hypothetical ceiling:** redundant preparation/registration and interception delays on the critical path. Server latency and site rendering remain outside this saving.

5. **Injected scripts offer narrower explanations than a generic “mutation observer” diagnosis.**  
   The automatically scheduled reader probe calls `find()`, which scans paragraphs and repeatedly reads ancestor/subtree text. Its 250-ms check does **not** bound all subsequent text walks and candidate scoring: [reader.js:19–88](C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/reader/reader.js:19). It is already deferred and epoch-checked (`app.rs:4692–4745`), so adding another delay would only move the hitch.

   Translation collection also performs substantial synchronous traversal; its budget check occurs only after accepting a text item, followed by attribute scanning (`translate/translate.js:52–76`). Translation is conditional on user action or configured automatic translation, not an unconditional ChatGPT cost.

   The full-timeline mutation observer found is **X/Twitter-specific** (`blocker/x_promoted.js:47–84`; `blocker.rs:1554–1565`). Login detection uses focus/submit listeners, not a general DOM scan (`webview2.rs:586`). Spatial scanning is conditional on the enabled feature and its navigation action.

   Extension-profile capture is guarded against repetition (`webview2.rs:2300–2319`). Installed content scripts remain an unknown; their code was not inspected. Discard/reload causes fresh document and content-script startup, whereas retained resume does not intentionally reload the document.

   **Candidate:** bounded incremental reader/translation work with unchanged results and document-epoch cancellation.  
   **Hypothetical ceiling:** overlapping injected-script blocking time. This cannot yet explain steady-state typing, scrolling, or streaming after the probe has finished.

6. **Omnibox has removable host work, but remote suggestions and Enter are different paths.**  
   [address_bar.rs:160–198](C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/win32/address_bar.rs:160) creates a thread per uncached suggestion request, sleeps 160 ms, then fetches. HTTPS sessions/connections are recreated (`translate/net.rs:99–126`). Address suggestions are nickname/settings-gated and forbidden in private tabs (`address_bar.rs:234–239`).

   Enter synchronously records typed history **before navigation** (`app.rs:5773–5799`; [address_runtime.rs:122–130](C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/address_runtime.rs:122)). Typing-triggered controller prewarming already exists (`window.rs:11716–11737`); it is not a new optimization.

   **Candidates:** bounded cancellable suggestion worker, connection reuse, fewer per-edit copies/scans, and ordered asynchronous typed-history persistence. Preserve freshness, IME/selection behavior, private exclusions, deletion barriers, and crash durability. Shortening debounce needs request-volume/battery evidence.  
   **Hypothetical ceiling:** debounce plus avoidable setup/UI work for suggestions; synchronous persistence and controller/host overhead for Enter. Remote RTT and search-result rendering remain.

Startup also performs profile/settings/security/session initialization before creating the window (`app.rs:4957–5160`). History loading, custom blocker parsing, font realization, and command prewarming already have deferred paths. Measure remaining synchronous phases before redesigning them. Consolidating same-trust chrome views is a larger architectural option; it must retain content/chrome profile isolation, focus, accessibility, and visual parity.

**Measurement contract proposed to the parent**

Use the frozen baseline and candidate with identical release features, runtime, profile seed, extensions, blocker generations, DPI, viewport, power state, and workload. Run one lane at a time. Distinguish fresh profile, reused profile with all owned processes exited, and warm running browser; **fresh profile does not establish cold OS cache**.

| Workload | Start → required completion |
|---|---|
| Application startup | External process-launch timestamp → complete intended chrome/start page or target page visibly presented and responding to an input challenge. |
| New tab | Physical click/Ctrl+T → correct selected tab, fully rendered configured start page, working focus and action. Separate first-use, resumed-host, and configured web-page cases. |
| Suspended wake | Selection input → retained document visibly current and responding to a fresh nonce challenge. Require successful suspension beforehand. |
| Discarded wake | Selection input → recreated controller, successful current navigation, expected content and working input. |
| Navigation | Navigation action → expected destination/content presented and task-specific action succeeds. Include redirects, first/cached visits, back/forward. |
| Local/remote suggestions | Final keystroke → correct current-query rows visibly presented and selectable. Report local-first and remote-complete separately. |
| Enter-to-render | Physical Enter with verified input → correct URL/query/POST result visibly usable; separately retain the full new-tab/search task time. |
| ChatGPT typing | Each input event → correct visible character/caret; include after idle, wake, and during streaming. |
| ChatGPT scrolling | Wheel/gesture → corresponding presented movement; also time reaching a fixed target. |
| Opening conversations | Click → correct conversation identity/content and usable composer. |
| Switching conversations | Switch action → correct replacement conversation, preserved draft/state, usable input. |
| Reply streaming | Controlled chunk arrival → corresponding visible text; track chunk-to-presentation latency, stalls, and final-content completion separately from server delivery time. |

Start with a deterministic large conversation fixture: long transcript, code blocks, editable composer, route changes, fixed-rate streamed chunks, and preserved drafts. It can exercise all five ChatGPT actions without authenticated chats or prompt submission. Real-site validation remains necessary later.

Existing evidence needs tightening:

- `process_start` is logged **after** argument parsing, data-root setup, and updater startup (`main.rs:24–69`); external timing must include these.
- `TrySuspend` completion is ignored (`webview2.rs:4589–4592`), while bookkeeping marks tabs suspended. Existing suspension labels cannot establish the wake precondition.
- [production-measurements.psm1:20–41](C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/benchmarks/production-measurements.psm1:20) has useful token/tab/navigation success checks. Extend these with controller/document identity and fresh wake/action nonces.
- `launch-to-usable.ps1:225–228` falls back to navigation completion; the production fixture uses double `requestAnimationFrame`. Neither proves display presentation or interaction readiness.
- The current analyzer’s `0.80` comparison is explicitly non-acceptance (`production-analyze.py:65`); it does not implement this task’s `≤0.50` requirement.

Reject wrong foreground/executable, stale tokens/titles, hidden controllers, failed/error/auth pages, fallback chrome, missing widgets, lost text, altered protection decisions, and timeouts as successes. Keep failures in the report. Use presentation evidence alongside semantic completion; callback receipt and RAF counts are insufficient.

Use at least ten valid alternating matched pairs per condition, expanding when variability requires it. Report absolute times, paired ratios, uncertainty, and tails per workload—no aggregate that masks a failed area. CPU, RAM, GPU load, network volume, and battery/resource effects are separate gates. If removable critical-path work is fraction `p`, even eliminating it saves at most `p` of total time; **50% remains unproven, not ruled out**.

**Implementation packages and disjoint ownership**

- **Measurement owner:** `src/performance.rs`, `benchmarks/*`. Add identity, successful-suspension, interaction, presentation correlation, and negative fixtures.
- **Lifecycle owner:** sole owner of `src/app.rs`, `src/engine/webview2.rs`, `src/win32/window.rs`. Handle active-memory restoration, controller scheduling, startup phases, and integration hooks.
- **Start-page owner:** `src/win32/start_ui.rs`, `src/win32/split_ui.rs`, `src/start_page/*`, `ui/src/start-page*`. Replace age gating and remove blocking snapshot work.
- **Blocker owner:** `src/blocker.rs`, `src/blocker/*`; lifecycle owner integrates engine changes. Cache/minimize with exact decision parity.
- **Omnibox owner:** address runtime/suggestion modules, `win32/address_bar.rs`, `win32/html_suggest.rs`, relevant UI modules. Keep networking changes scoped to suggestions.
- **Script owner:** `src/reader/*`, `src/translate/translate.js`, `src/app/page_translate.rs`, corresponding UI tests. Preserve probe/translation output and cancellation.

Minimum meaningful tests: suspend refusal and late completion; select/close/navigation races; draft/media/download/private sleep protection; foreground input after idle; queued foreground creation and POST supersession; start-page first-use/resume/settings parity; blocker coverage equivalence including workers and cap boundaries; stale suggestion replies and history-clear races; large dynamic/CJK/hostile reader and translation fixtures. Parent-run native end-to-end tests must then verify the actual timing and resource gates.

**Skills used**

- [benchmark-checklist](C:/Users/charlie/.agents/skills/benchmark-checklist/SKILL.md): completion, error, limiter, repeatability, and resource checks; benchmark execution skipped.
- [principle-fix-root-causes](C:/Users/charlie/.agents/skills/principle-fix-root-causes/SKILL.md): traced lifecycle and scheduling decisions; runtime reproduction and fixes deferred.
- [principle-guard-the-context-window](C:/Users/charlie/.agents/skills/principle-guard-the-context-window/SKILL.md): targeted source reads and bounded handoff; no delegation.
