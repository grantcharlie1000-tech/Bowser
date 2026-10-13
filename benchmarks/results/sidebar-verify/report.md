# Sidebar live verification

Date: 2026-10-12. Candidate: current dirty `main` worktree, built with `CARGO_TARGET_DIR=target-luna3`.

## Matrix

| # | Area | Result | Evidence / limitation |
|---|---|---|---|
| 1 | Rail panels and close paths | **FAIL** | Clicking Bookmarks visibly opened the panel, then the app faulted while toggling/continuing the panel exercise. See `10-bookmarks-open.png`, `11-bookmarks-toggle-close.png`, and `wer-crash-events.txt`. History, Customize, Home, close-button, Esc, and panel switching were not independently verified after the process became unresponsive. |
| 2 | Workspaces | **BLOCKED** | App faulted before workspace interaction. |
| 3 | Restart persistence | **BLOCKED** | App did not exit cleanly; no restart comparison. The profile was isolated, but no workspace/sidebar/bookmark persistence claim is made. |
| 4 | Profile separation | **BLOCKED** | No second profile or cookie/localStorage test was run. |
| 5 | Custom apps and isolation | **BLOCKED** | App faulted before custom-app setup. |
| 6 | Bookmarks and history workflows | **BLOCKED** | Bookmarks panel rendered its empty state, then the process faulted. Folder/search/edit/delete and History flows were not exercised. |
| 7 | Window sizes, DPI, vertical tabs | **BLOCKED** | Only the initial 1000x720 client at 96 DPI was captured; no resizing or layout-mode changes. |
| 8 | Themes and accent | **BLOCKED** | No theme/accent change was attempted. |
| 9 | Rapid interaction stress | **BLOCKED** | Not run after the app became unresponsive. |
| 10 | Performance and baseline comparison | **BLOCKED** | No 60-second CPU/memory run or archived-HEAD baseline was measured. At initial capture the process had 190 handles, 19 GDI objects, and 17 USER objects. After the Bookmarks panel appeared: 195/34/25. These are single snapshots during a failing run, not performance results. |

## Bug

**Major — the Bookmarks panel interaction sequence leads to an application fault.** Repro: launch the snapshotted executable with a fresh `--profile sidebar-test-luna2-20261012`; click the Bookmarks heart at screen point `(178, 787)` in the 1000x720 client window. The panel becomes visible. The harness waits, captures, then clicks the same icon to toggle it; during this sequence the app becomes unresponsive and Windows reports two faults for PID `0xBEF0`: `USER32.dll`, offset `0x1f18b`, first exception `0xc0000005` at 02:26:29 and then `0xc000041d` at 02:26:48. The screenshot at `10-bookmarks-open.png` shows the panel and the Windows crash dialog; `11-bookmarks-toggle-close.png` shows the panel gone with the crash dialog still present. WER cannot distinguish whether opening, painting, or the following toggle triggered the fault. Event details are in `wer-crash-events.txt`.

Expected: the panel opens, and a second click closes it without hanging or crashing. Actual: access violation/unhandled window-procedure exception, nonresponsive process. Suspected source path: `src/win32/side_panel.rs` (`open`, `proc`, `paint`) and `src/win32/panel_bookmarks.rs` (`Content::paint`); `src/win32/window.rs` (`rail_activate`, `apply_sidebar_bounds`) is the caller path. WER identifies USER32 and an offset only, so no source line is proven.

## Build and harness

- Build succeeded on the third attempt after concurrent edits settled; warnings were unused imports and unused rail helper functions.
- Executable snapshot: `test-bin/bowser.exe`, SHA-256 `4B32FC1EB9F2AFE09B72EAD642F2D4B8AB961A896FEF40CDD1001D5DD1C8AFA5`.
- Harness: `run-panels.ps1`; it captures a full-screen PNG for every scripted step and records window geometry, process handles, GDI objects, and USER objects in `panel-summary.json`.
- The test executable was stopped after it stopped responding. The installed Bowser executable was not touched.
- Cleanup of the two isolated profiles was attempted, but the environment's automatic review rejected the recursive `Remove-Item` operations. The remaining exact paths are `%LOCALAPPDATA%\Bowser\Profiles\sidebar-test-luna-20261012` and `%LOCALAPPDATA%\Bowser\Profiles\sidebar-test-luna2-20261012`.
