# Isolated stable recovery reproduction, 2026-11-15

## Provenance and setup

- Reference executable was copied from the installed Bowser Stable 2 executable. SHA-256: `3d24b39f553bd1a381de99a7ca172682e8c530a6a2dc2f5ec7390573fe194fdd`, matching the v0.0.26 stable pin.
- Reviewed source: `440575e0870fd35c257d4195ec990d3c02b837aa`. Product source for the inspected entry points matches pinned release commit `fc3f6f24d3c5cc72c0c59f247a47d48d859966ed`.
- Isolated executable/data/launch records: `C:\Users\charlie\.t3\artifacts\bowser-performance-20261115\stability-stable-reference\`.
- Test data root: `valid-lifetime-probe`. Explicit `--data-dir`, isolated `LOCALAPPDATA`, ordinary diagnostic logging, and the repository's loopback demanding fixture. No authenticated sites or actual user profile were accessed.
- Root PID: 22340. Owned WebView2 browser PID: 27504; runtime 157.0.4341.46. The browser PID's parent, executable name, browser role, creation identity, and isolated-profile command line were verified before injection.
- Normal fixture request and successful navigation/fixture acknowledgement appeared in the flushed diagnostic log. Before failure, accessibility exposed content, address bar, toolbar, tabs and rail.

## Failure experiment

1. Launch the stable reference against the demanding loopback fixture and establish normal semantic completion.
2. Terminate only the verified browser process belonging to the disposable reference profile. Injection record: `valid-lifetime-probe/fault-injection.json`.
3. Observe a new Bowser modal dialog. Its exact text was `WebView2 error: Class not registered (0x80004005)`.
4. While this dialog remained, the main window's accessibility tree contained only its root, text and empty panes; the previously observed toolbar/content controls were absent.
5. Dismiss the error using Return. Native address input then reappeared. This confirms recovery/control availability was held behind modal dismissal in this test.
6. Close the isolated main window with Alt+F4. Root exited normally with code 0. The owned fixture server was then stopped. The actual installed browser and profile were not terminated or changed by this experiment.

## What this establishes

A forced loss of this isolated stable reference's shared WebView2 browser produces a blocking error path and removes normal main-window controls until dismissal. This supplies a baseline failing recovery case for the process-failure fix.

It does not establish why the user's browser failed naturally, that the user's screenshot had the same cause, or a natural host-process crash. The native-close check succeeded after dialog dismissal. The initial argument-validation trial using `about:blank` was invalid because `--open-url` accepts HTTP(S)/file initial addresses; that trial is excluded.

Both Windows capture interfaces timed out while capturing the reference window. Accessibility, process ownership/lifetime, and diagnostic records are evidence here; **visual recovery and pixel opacity remain unverified**. No performance gain is claimed.

## Source and primary documentation

The source audit is in `audit-stability.md`. Microsoft's [threading guidance](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/threading-model#reentrancy) disallows synchronous modal loops in WebView2 callbacks and recommends deferring their work. Its [process-failure guidance](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-related-events) requires associated WebViews to be recreated after browser-process loss and distinguishes auto-recoverable process exits.

## Additional observation

The reference log records foreground `idle_trim_low` followed by `memory_target_applied_low` after initial load. This is observed policy application, not proof that it caused responsiveness problems.

