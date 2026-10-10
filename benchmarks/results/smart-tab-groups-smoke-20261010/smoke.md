# Bowser UI and policy smoke results

Captured UTC: 2026-10-10T05:50:58.0737213Z; overall result: **PASS**.

Event log: `C:\Users\charlie\AppData\Local\Temp\bowser-smoke-c9068f393f1f491a9ea2fd9b3ba88c8a\events.csv`; local HTTP request log: `C:\Users\charlie\AppData\Local\Temp\bowser-smoke-c9068f393f1f491a9ea2fd9b3ba88c8a\requests.txt`.
Temporary profile removed after shutdown: True; process-group shutdown confirmed: True.
External URI proof: scheme `bowser-smoke-dcbe73eb77c14ebf87544b5c5c324eeb`, callback event observed `True`, benign handler marker created `False`.
Loopback requests observed: /one, /favicon.ico, /two, /favicon.ico, /favicon.ico, /favicon.ico, /favicon.ico, /, /popup-user, /favicon.ico, /download, /slow, /.

## Checks

- **empty_launch_native_input: PASS** â€” Visible native window and EDIT control found.
- **lazy_engine_start: PASS** â€” No msedgewebview2.exe descendant before navigation.
- **address_submission_navigation: PASS** â€” Address field verified before Enter using Windows Forms SendKeys after virtual-key SendInput; loopback HTTP navigation reached BOWSER_SMOKE_ONE.
- **result_link_and_history: PASS** â€” Clicked the local result link; Alt+Left and Alt+Right title markers observed.
- **user_popup_new_tab: PASS** â€” User-initiated target=_blank produced popup_new_tab in the original HWND; /popup-user reached the loopback server and BOWSER_SMOKE_POPUP_USER became the active tab title; Ctrl+1 restored the original tab with BOWSER_SMOKE_ONE.
- **non_user_popup_blocked: PASS** â€” Delayed popup produced popup_blocked and left the current document usable.
- **permission_denied: PASS** â€” Local-page geolocation request produced permission_denied.
- **download_cancelled: PASS** â€” Local attachment request produced download_cancelled.
- **external_protocol_blocked: PASS** â€” scheme=bowser-smoke-dcbe73eb77c14ebf87544b5c5c324eeb; external_protocol_blocked observed; handler marker absent after 1 second.
- **close_during_loading: PASS** â€” The browser and all observed descendants exited after WM_CLOSE while /slow was pending.
- **frozen_binary_check: PASS** â€” Bowser release SHA-256 unchanged: d6f845239e2b44e13acb972cd9105a4f521308c443d13ce4e96309bd107eb5c6

The smoke uses a disposable WebView2 profile and a loopback-only HTTP server. It checks empty startup, native address input, lazy WebView2 startup, a local result link and history, a user-initiated popup opening a new tab, a non-user popup being blocked, geolocation denial, download cancellation, external protocol blocking, and close during a deliberately held local request. `-IncludeWebSearch` additionally uses temporary LOCALAPPDATA with no search-provider override, submits `Rust WebView2` to default Bing, locates and physically clicks a visible result via UI Automation, then requires the WebView source and native address EDIT to agree on the final non-Bing HTTP(S) URL and the Bowser window title to identify the destination page in the same HWND. A Bing redirect intermediary, stale address field, unusable title, or provider/network failure is recorded as a failure.
