**The strongest active-session lead is WebView2 process failure entering a synchronous modal error path before chrome recovery completes. A separate, concrete bitmap-allocation defect can produce either missing native chrome or a Bowser panic. Neither is established as the incident’s trigger.**

Audit remained read-only: no edits, builds, tests, launches, live-app/profile access, or children. HEAD is the requested `440575e…`; tracked files are unchanged. The stable manifest identifies `fc3f6f24…`, and its `src`, `ui`, and `Cargo.toml` have no differences from this baseline. Local `origin/main` has advanced, so findings remain pinned to `440575e…`.

The parent’s initial `Responding=true` does not establish working WebView2 content. Subsequent absence from the app inventory does not distinguish crash from closure. The screenshot shows desktop-visible regions, a thin window outline and caption-like marks; it cannot establish process identity, hit-test behavior or failure type. There is no fresh crash stack.

1. **Highest-priority active-session defect: process failures can block recovery inside a modal callback, and browser-process loss has no content recreation path.**

   At [webview2.rs:6042–6058](/C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/engine/webview2.rs:6042), only `RENDER_PROCESS_EXITED` receives special handling. Every other kind becomes generic `E_FAIL`, including browser loss, renderer unresponsiveness and auto-recoverable GPU/utility exits.

   That callback immediately reaches `app.rs:6628–6630` → [win32/mod.rs:111–120](/C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/win32/mod.rs:111), which calls an unowned `MessageBoxW` **inside the WebView2 event handler**. Repeated renderer crashes also reach this modal path through `app.rs:5612–5618`.

   Microsoft explicitly documents synchronous modal UI inside WebView2 callbacks as unsupported reentrancy; subsequent callbacks cannot proceed normally until the handler returns. [Threading model](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/threading-model#reentrancy)

   **Plausible combined failure chain:** shared browser process exits → content’s failure callback runs first → modal dialog retains that callback → chrome’s failure notification has not yet restored native controls → dead HTML surfaces remain designated as active. This can leave the native message pump responsive while browser surfaces are unusable. Callback order and whether a dialog existed remain unverified.

   Furthermore, generic failure handling does not invalidate/recreate the tab controllers or shared engine. Browser-process loss requires recreating associated WebViews; GPU/utility exits ordinarily recover automatically. [Process-failure recovery](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-related-events?tabs=win32cpp)

   **Narrow fix:** carry failure kind, affected tab/controller generation and available reason/exit details through `EngineEvents`; queue handling after the callback returns. Coalesce shared failures. Restore native chrome immediately, invalidate affected presentation state, and recreate controllers after browser loss. Preserve bounded renderer-reload behavior; avoid modal errors for auto-recoverable shared-process exits.

2. **Concrete native crash/blank-chrome path: failed DIB replacement destroys the usable surface and can reach unchecked pixel indexing.**

   [paint.rs:2980–3015](/C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/chrome/paint.rs:2980) releases the previous DIB **before** creating its replacement. `CreateCompatibleDC`, `CreateDIBSection` and selection failures do not propagate; a failed DIB leaves null pixels with nonzero dimensions.

   Main painting returns early for null pixels (`paint.rs:624–625`). However, `window.rs:12303–12312` then independently calls vertical-sidebar painting. That function checks its clip but not surface validity (`paint.rs:464–488`), and `fill_rect` indexes `pixels[i..i + 4]` (`951–960`). `pixels_mut()` returns an empty slice when allocation failed (`3044–3046`). With a nonempty sidebar region, this is a bounds-panic path. Release uses `panic = "abort"`.

   Without that extra drawing path, native chrome/captions can instead stop updating. Same-size `ChromeSurface::resize` returns without retrying the allocation (`317–325`).

   **Narrow fix:** make surface replacement transactional and fallible; retain the old valid allocation until replacement succeeds. Validate every independently callable painting entry point. Report allocation/presentation failure to the window and retry deliberately rather than relying on another resize. Establish an opaque, closable emergency surface if the layered surface cannot be presented.

   **Confidence:** source-confirmed defect under allocation failure; no evidence yet of resource exhaustion or failed allocation in this incident.

3. **Transparency is amplified by optimistic controller state and dependence on the layered overlay.**

   `navigate_tab` sets `controller_created` before successful asynchronous creation (`app.rs:6881`). Creation failures remove engine pending state and report an error (`webview2.rs:1921–1947,2014–2027`), without clearing that application flag. Process-failure handling likewise does not reliably clear it.

   [window.rs:12083–12088](/C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/win32/window.rs:12083) uses that flag to decide whether content exists; `12295–12296` suppresses the opaque empty-viewport surface accordingly.

   Meanwhile:

   - `WM_ERASEBKGND` fills only Solid mode: `window.rs:2760–2771`.
   - The main-window paint also skips its fill in translucent mode: `12384–12400`.
   - Native controls and captions are drawn into a separate layered overlay: `12303–12341`.
   - Both layered-upload attempts can fail without notifying the window: `paint.rs:918–932`. Initial overlay creation failure is also swallowed: `window.rs:2895–2909`.

   Thus “controller requested/previously created” can suppress the last opaque content fallback after the visible controller disappears. Glass painting alone does **not** explain absent captions: those have an independent native drawing path.

   **Narrow fix:** distinguish requested, live and failed controller state; clear failed state and invalidate the affected viewport. Make overlay creation/upload failure observable. Keep native caption interaction available during recovery. A blanket `WM_ERASEBKGND` fill or permanently disabling glass would mask part of the symptom without repairing lifecycle failures.

4. **Chrome takeover lacks a confirmed-state boundary and a bounded failure path.**

   [window.rs:9632–9709](/C:/Users/charlie/.t3/worktrees/Bowser/t3-8b9aed0b-1f9e0bbd7831ad1b/src/win32/window.rs:9632) activates HTML before `html_sync`, then hides the native edit regardless of successful state delivery. `ChromeHost::reveal` also records `revealed=true` before bounds/visibility success (`host.rs:1348–1368`). A successful `PostWebMessageAsJson` establishes neither application of state nor usable rendering.

   Failure recovery exists: fatal chrome `ProcessFailed` notifications are posted outside the callback (`window.rs:9589–9596`), and `html_failed` restores native painting/input (`9993–10018`). However:

   - Three **consecutive** COM failures are required; successes reset the counter (`host.rs:472–484`).
   - Navigation starting does not reset readiness (`host.rs:995–1007`).
   - No navigation-completion failure handler or ready/state-application timeout was found.
   - Pending HTML deliberately suppresses native toolbar/rail/vertical-sidebar painting (`paint.rs:478–481,660–678`). Horizontal tabs and captions remain native.
   - CSP refusal intentionally sends no `ready` (`ui/src/main.ts:515–518`).

   This is particularly relevant if a host reloads or loses its document during browsing; startup pending behavior is a secondary finding.

   **Narrow fix:** retain functional native chrome until the current document generation acknowledges applying required initial state. Reset readiness on navigation; use bounded create/ready/initial-state deadlines and reject late acknowledgements after fallback. Keep the existing deferred failure pattern.

5. **Window lifetime and reentrancy contain concrete hazards, but no fresh host-crash attribution.**

   Early ownership is present: `WM_NCCREATE` installs a boxed cloned `AppRef` and assigns `hwnd` (`window.rs:2618–2624`); `WM_NCDESTROY` frees it (`4814–4816`). **The evidence does not support “AppRef was never installed” as the leading explanation.**

   Remaining hazards:

   - `window_proc` and `edit_subclass` retain references into that box across dispatch instead of owning local `Rc` clones (`2627–2628`, `12849–12850`). Nested destruction can invalidate that storage.
   - `WM_CLOSE` closes pumping COM hosts **before** setting `app.closing=true` (`4754` versus `4761`).
   - `window_closing` returns false when app state is absent (`9577`); asynchronous chrome completions use HWND plus that check, without a creation-generation lease.
   - `App::with_engine` calls `current.close()` while holding `App`’s mutable borrow (`app.rs:1096–1098`); the navigation shutdown branch repeats this pattern (`6912–6917`). A synchronous callback touching `App` can panic.

   The sidebar host already demonstrates stronger creation-generation checks: `panel_webapp.rs:359–384`.

   **Narrow fix:** clone dispatch ownership before pumping; mark shutdown at its entry and make it idempotent; detach state before COM teardown; reject missing-owner or stale-generation completions. Drop `RefMut` before every relevant close call. Treat these as targeted lifetime repairs, not a broad rewrite.

**Minimal parent-owned reproduction and safety plan**

Use only the isolated profile and its verified process identities. Add bounded diagnostic records for failure kind/reason, browser/controller generation, chrome fallback transitions, paint allocation/upload results, `WM_CLOSE`/`WM_NCDESTROY`, and process exit. Avoid page content and URLs.

| Injection | What must be established |
|---|---|
| After normal browsing begins, terminate only the disposable WebView2 browser process | Does a modal callback precede chrome fallback? Does Bowser survive, regain visible native controls, recover content or show a usable error, and close normally? |
| Separately inject main-renderer exit, repeated exit, unresponsiveness, GPU exit and utility exit | Correct classification; bounded retry; no dialog storm; no renderer-dependent close route. Cover active, background and split tabs. |
| Fail the next main-surface DIB allocation during resize, first horizontally then with vertical tabs | Existing source’s blank/panic distinction; repaired build retains a valid surface or displays fallback, never indexes absent pixels, and recovers at unchanged dimensions. Also fail both layered-upload calls. |
| Fail navigation / suppress `ready` / fail initial state application / reload chrome without another `ready` | Native toolbar remains usable; timeout restores it; stale callbacks cannot reactivate failed HTML. |
| Force nested close during COM teardown and deliver controller completion after close | No `RefCell` panic, dangling dispatch state, resurrected controller or duplicate shutdown. |
| Repeat failure cases in glass/Solid mode, maximized/restored, vertical tabs and page fullscreen | Visible recovery plus actual close hit-testing; test caption close, Alt+F4 and Escape independently. Preserve successful-session geometry and input behavior. |

Existing tests cover pieces, not this acceptance condition: `app.rs:8895` checks renderer-crash bookkeeping; `host.rs:1500,1624` check failure classification/counting; `paint.rs:3490` checks upload-cache invalidation. They do not prove usable recovery after live process loss or allocation failure.

**Recommended order:** capture one isolated active-session failure first; prioritize deferred typed process recovery and the DIB failure test/fix. Then address confirmed takeover/lifetime failures. Performance acceptance remains blocked; this audit establishes no stability fix or performance gain.


