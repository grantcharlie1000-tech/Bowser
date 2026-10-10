# Bowser 0.0.1

A small experimental x64 Windows browser: one native window, one native address/search input, and a WebView2 viewport. Its goal is useful browsing with few application components and measured resource use.

## Build and run

Requirements: Windows 10 or later (x64), Rust with the MSVC target, Visual Studio C++ Build Tools and Windows SDK, and the installed [Evergreen WebView2 Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/#download-section).

Build from this directory:

    cargo build --release --locked

Double-click Start-Bowser.cmd, or run target\release\bowser.exe. The window starts empty with the input focused. Enter a web address or search phrase. Alt+Left and Alt+Right navigate history. The engine starts when you begin typing, while no network request is sent until you press Enter.

The shared WebView2 runtime is required but is not bundled into Bowser. Runtime availability is checked and failures are shown as native error messages. Microsoft manages Evergreen security updates.

## Search and local data

The default search provider is Bing (`https://www.bing.com/search?q={query}`). To change it, create %LOCALAPPDATA%\Bowser\search-provider.txt containing a single HTTPS URL template with exactly one {query}, for example:

    https://www.google.com/search?q={query}

Missing or invalid configuration uses the default. Search text is percent encoded. Explicit HTTP(S) addresses and bare hostnames are accepted; unsupported schemes and embedded URL credentials are rejected.

Browser cookies/cache remain in %LOCALAPPDATA%\Bowser\WebView2, isolated from Edge's profile. The executable has no hosted backend or telemetry. Websites and the selected search provider receive normal network requests when used.

## Default website policy

Website permissions are denied, downloads are cancelled, and external application protocols are blocked. User-initiated HTTP(S) links requesting another window navigate in the existing viewport; script popups are blocked. Bowser retains the runtime's sandbox, site isolation, certificate validation and update behavior.

Tabs are in progress (see [Tabs and keyboard shortcuts](#tabs-and-keyboard-shortcuts)). Bookmarks, account management, extensions, a settings UI, and a download manager are not included. It is an experimental browser and the benchmark report describes its observed behavior and limits.

## Interface

The window has two rows over a translucent surface:

- **Tab islands:** each tab is its own rounded island with a favicon, title and close button. The active tab is lighter and has a border. A `+` button adds a tab, and the minimise, maximise and close buttons sit at the far right.
- **Omnibox:** one address and search field spanning nearly the full width below the tabs. There is no toolbar, logo or bookmarks bar.

An empty tab shows the desktop through the window. Real pages render opaque, as before. The tab islands and per-tab engine wiring are still in progress, so this describes the intended layout.

## Glass material and fallback

On Windows 11 (build 22621 or later), the window uses the system transient backdrop. On Windows 10, it uses a tinted acrylic accent; set `BOWSER_GLASS_VARIANT=blur` to use plain blur-behind instead. The development machine runs Windows 10, so the Windows 11 path has not been checked on Windows 11.

Bowser uses an opaque themed fill with no transparency when any of these apply:

- transparency effects are off (`EnableTransparency` is `0` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`);
- the session is remote;
- high-contrast mode is on;
- the system backdrop call fails.

Set `BOWSER_FORCE_SOLID=1` to force the solid fallback regardless of these settings. Light and dark follow the Windows app mode (`AppsUseLightTheme`) and update when the system colour setting changes.

## Tabs and keyboard shortcuts

Shortcuts match modifiers exactly, so Ctrl+Alt combinations (AltGr on some keyboard layouts) never trigger a command. Tab commands are wired to the shortcut handler, but the tab islands are still in progress.

| Action | Shortcut |
| --- | --- |
| New tab (focuses the address field) | Ctrl+T |
| Close tab | Ctrl+W, Ctrl+F4 |
| Next tab | Ctrl+Tab, Ctrl+Page Down |
| Previous tab | Ctrl+Shift+Tab, Ctrl+Page Up |
| Select tab 1 to 8 | Ctrl+1 to Ctrl+8 |
| Select last tab | Ctrl+9 |
| Focus address field | Ctrl+L, Alt+D, F6 |
| Reload | F5, Ctrl+R, browser refresh key |
| Hard reload | Ctrl+F5, Ctrl+Shift+R |
| Stop loading | Esc, browser stop key |
| Back | Alt+Left, browser back key |
| Forward | Alt+Right, browser forward key |

## Verification and benchmarks

    cargo test --locked
    cargo clippy --locked -- -D warnings
    powershell -NoProfile -ExecutionPolicy Bypass -File benchmarks\smoke.ps1
    powershell -NoProfile -ExecutionPolicy Bypass -File benchmarks\run.ps1 -Runs 5 -IdleSeconds 30

The benchmark controls foreground browser windows and sends input. Allow it to finish without using the keyboard/mouse. It creates disposable Bowser, Edge and Chrome profiles, uses identical local fixtures for page workloads, and records the whole process group's working set, private bytes and CPU time. Existing browser profiles are not used.

Fresh-profile and reused-profile launches both start a new process group. Windows filesystem/standby caches are left untouched, so these are not OS-reboot cold measurements. The first-page endpoint is a fixture's DOM completion plus two animation frames, detected through its title marker; it is a controlled usability proxy. Remote search behavior is checked separately because network/provider latency varies.

Raw results, software/hardware metadata and the measurement report are in benchmarks/results. Bowser's executable footprint is reported separately from the shared WebView2 installation and browser profile data. Summed working sets can count shared pages more than once; private bytes are also recorded. Direct battery energy is unmeasured unless an appropriate source is available.

The Phase 0 measurements, known limits, largest measured bottleneck, and next performance experiments are summarized in [docs/phase-0-results.md](docs/phase-0-results.md).

The shell uses direct windows and webview2-com bindings; url supplies URL parsing and encoding. Cargo.lock pins the complete dependency set. Benchmark code and optional event logging run only during explicit measurement launches.
