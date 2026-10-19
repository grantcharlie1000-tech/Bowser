# Bowser UI and policy smoke results

Captured UTC: 2026-10-08T20:47:59.6972581Z; overall result: **PASS**.

Event log: `C:\Users\charlie\AppData\Local\Temp\bowser-smoke-60500a0d654841f8a2ee0471ce4a82af\events.csv`; local HTTP request log: `C:\Users\charlie\AppData\Local\Temp\bowser-smoke-60500a0d654841f8a2ee0471ce4a82af\requests.txt`.
Temporary profile removed after shutdown: True; process-group shutdown confirmed: True.
External URI proof: scheme `bowser-smoke-b520e3c4bcd34575bb743f1420804521`, callback event observed `True`, benign handler marker created `False`.
Loopback requests observed: /one, /favicon.ico, /two, /favicon.ico, /favicon.ico, /favicon.ico, /favicon.ico, /, /popup-user, /favicon.ico, /favicon.ico, /download, /slow, /.

## Checks

- **empty_launch_native_input: PASS** — Visible native window and EDIT control found.
- **lazy_engine_start: PASS** — No msedgewebview2.exe descendant before navigation.
- **address_submission_navigation: PASS** — Address field verified before Enter using SendInput; loopback HTTP navigation reached BOWSER_SMOKE_ONE.
- **result_link_and_history: PASS** — Clicked the local result link; Alt+Left and Alt+Right title markers observed.
- **user_popup_same_view: PASS** — User-initiated target=_blank loaded in the original main HWND; popup_same_view event observed.
- **non_user_popup_blocked: PASS** — Delayed popup produced popup_blocked and left the current document usable.
- **permission_denied: PASS** — Local-page geolocation request produced permission_denied.
- **download_cancelled: PASS** — Local attachment request produced download_cancelled.
- **external_protocol_blocked: PASS** — scheme=bowser-smoke-b520e3c4bcd34575bb743f1420804521; external_protocol_blocked observed; handler marker absent after 1 second.
- **real_web_search_result: PASS** — query=Rust WebView2; provider=https://html.duckduckgo.com/html/?q=Rust%20WebView2; title=Rust WebView2 at DuckDuckGo; result="webview2 - Rust - Docs.rs" -> https://docs.rs/webview2/latest/webview2/; native address=https://docs.rs/webview2/latest/webview2/; destination title=webview2 - Rust; same HWND
- **close_during_loading: PASS** — The browser and all observed descendants exited after WM_CLOSE while /slow was pending.
- **frozen_binary_check: PASS** — Bowser release SHA-256 unchanged: c1a8b654574a4df64890a39c2a5898bf06fceda7ab8ff25735cc352bc5b1a0b6

The smoke uses a disposable WebView2 profile and a loopback-only HTTP server. It checks empty startup, native address input, lazy WebView2 startup, a local result link and history, user and non-user popup policy events, geolocation denial, download cancellation, external protocol blocking, and close during a deliberately held local request. `-IncludeWebSearch` additionally submits `Rust WebView2` to configured DuckDuckGo HTML, locates and physically clicks a visible result via UI Automation, then requires the WebView source and native address EDIT to agree on the final non-DuckDuckGo HTTP(S) URL and the Bowser window title to identify the destination page in the same HWND. A DuckDuckGo redirect intermediary, stale address field, unusable title, or provider/network failure is recorded as a failure.
