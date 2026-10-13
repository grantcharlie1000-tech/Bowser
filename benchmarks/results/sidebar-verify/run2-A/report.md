# Bowser sidebar verifier A — run 2

## Result

The isolated `target-luna3` build completed and its executable launched with a throwaway profile. `PrintWindow` successfully captured the exact test window (`PID 67992`, title `Bowser - Search or enter address [V vA-4]`). The capture shows Customize content, but it is only a single rendered state and does not establish that a test interaction opened it.

The final build-log run produced a different executable hash than the file already copied and launched (`8E24…1E87` vs `F7B8…EF584`, with different file sizes). I preserved both. The GUI capture belongs to the earlier `F7B8…EF584` test executable; the latest `8E24…1E87` build was not launched because foreground activation was blocked. See `build-executable-comparison.json`.

The requested interaction matrix is **BLOCKED**. The active foreground window was Helium (`chrome.exe`, `Home / X - Helium`). Windows rejected activation of the test HWND even after a real Alt key tap and `SetForegroundWindow`; the harness stopped before sending clicks on the retry. The first CopyFromScreen run captured Helium instead of the test window, so its screenshots and interaction measurements are contaminated and are excluded from app results.

## Matrix

| # | Scenario | Status | Evidence / reason |
|---|---|---|---|
| 1 | Open/close/toggle/switch all panels; first-open Escape; 30 cycles; Switch profile menu + Escape; viewport restore | **BLOCKED** | Foreground activation failed. `test-window-printwindow.png` is a valid app-window capture but only one state. Initial CopyFromScreen captures are contaminated. |
| 2 | Workspace create/switch/restore/rename/icon/colour/reorder/delete and modal behavior | **BLOCKED** | Could not deliver verified real mouse input to the test window. |
| 3 | Graceful restart and workspace/tab/sidebar persistence | **BLOCKED** | No state-changing UI flow was completed. Captured startup `workspaces.txt` contains only the default workspace. |
| 4 | Profile isolation for bookmark, cookie and localStorage | **BLOCKED** | No two-profile content setup or local HttpListener page was exercised. |
| 5 | Narrow/maximized/snapped layouts and vertical tabs | **BLOCKED** | No resize, snap, maximize, or settings change was safely delivered to the test window. |
| 6 | Light/dark/accent visuals and ghosting | **BLOCKED** | No theme changes were made; no user personalization registry values were changed. |

## Crash evidence

No Application Error / WER event 1000/1001 matched the owned `target-luna3` executable after its launch, and `%LOCALAPPDATA%\Bowser\crash.log` was absent at capture time. The general one-hour Application log query contains older `target-opus\debug\bowser.exe` failures at 02:45–02:46 local time; these are not from this test build. See `owned-test-events.txt`, `application-events.txt`, and `crash-log-status.txt`.

## Profile files

Before cleanup, vA-4 had `settings.txt` (horizontal tabs, sidebar width 260, side panel width 340) and `workspaces.txt` (`active=0`, Default workspace). `sidebar.txt` and `bookmarks-v2.txt` were absent. vA-1 through vA-3 had only initial `privacy-stats.txt` and `running.lock` files. Exact file inventory is in `profile-files-before-cleanup.txt`; only the four test profiles were removed. `cleanup-summary.json` records that the test process exited and all four profile roots were removed.

## Suspected app bugs

None confirmed. This run establishes successful build, process startup, and a readable app-window render only. It does not verify the crash fix under repeated input or any of the requested behavior. The actionable blocker is external to Bowser: this harness session could not activate its test HWND while Helium owned the foreground.

## Artifacts

- `test-bin\bowser.exe` — executable launched for the captured run (SHA-256 `F7B84D161F215213E0CE6C505352501C18FC2801672C51E27FF0B338260EF584`).
- `built-after-test-bowser.exe` — latest successful build output, not live exercised (SHA-256 `8E24B3C203FBE9FB58B6734E786D51BAFFD652129004C09D553FF89F32481E87`).
- `build.log`, `build-executable-comparison.json` — latest build and both executable identities.
- `run-panels-A.ps1` — copied harness using `SendInput` for Escape; foreground retry adds activation ownership checks.
- `foreground-retry\` — aborted focus-retry harness output. The contaminated CopyFromScreen captures are the root-level `00-*`, `10-*`, and subsequent PNGs.
- `test-window-printwindow.png` and `test-window-printwindow.json` — verified capture/identity for the test window.
- `owned-test-events.txt`, `application-events.txt`, `crash-log-status.txt` — crash evidence.
- `profile-files-before-cleanup.txt`, `cleanup-summary.json` — profile inventory and cleanup record.
