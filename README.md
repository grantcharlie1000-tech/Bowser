# Bowser 0.0.1

A small experimental x64 Windows browser: one native window, one native address/search input, and a WebView2 viewport. Its goal is useful browsing with few application components and measured resource use.

## Build and run

Requirements: Windows 10 or later (x64), Rust with the MSVC target, Visual Studio C++ Build Tools and Windows SDK, and the installed [Evergreen WebView2 Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/#download-section).

Build from this directory:

    cargo build --release --locked

Run `Update-Bowser.cmd` once from the project to build and install Bowser for the current Windows user. It installs to `%LOCALAPPDATA%\Programs\Bowser` and creates a Start Menu shortcut that launches the installed executable directly; there is no build during normal startup. Browser profiles and settings remain in `%LOCALAPPDATA%\Bowser`. The updater validates the locked release build before closing Bowser or replacing installed files, and preserves browser data. Use Windows Settings > Apps to uninstall Bowser; uninstall removes the application and shortcuts but keeps browser data. Run `Update-Bowser.cmd` again after changing the source to deploy a new build.

The window starts empty with the input focused. Enter a web address or search phrase. Alt+Left and Alt+Right navigate history. The engine starts when you begin typing, while no network request is sent until you press Enter.

The shared WebView2 runtime is required but is not bundled into Bowser. Runtime availability is checked and failures are shown as native error messages. Microsoft manages Evergreen security updates.

## Search and local data

Address bar searches use one of two engines: **Google** (`https://www.google.com/search?q={query}`) or **DuckDuckGo** (`https://duckduckgo.com/?q={query}`). To choose, open the main menu, choose Settings, pick the engine under Search engine, and press Save. The choice applies to the next search with no restart, and is stored as `search_engine=google` or `search_engine=duckduckgo` in the profile's `settings.txt`.

Nothing changes on upgrade. A profile with no saved choice keeps its previous behaviour: a valid `search-provider.txt` is used if present, and otherwise the search goes to Bing (`https://www.bing.com/search?q={query}`). A saved choice takes precedence over `search-provider.txt`; to use the file again, choose Reset to defaults in Settings or delete the `search_engine` line. To send searches to another site, create %LOCALAPPDATA%\Bowser\search-provider.txt containing a single HTTPS URL template with exactly one {query}, for example:

    https://www.google.com/search?q={query}

Missing or invalid configuration uses the default. Search text is percent encoded as one query value, so spaces, `&`, `#`, `+`, `%` and non-ASCII text are sent exactly as typed. Explicit HTTP(S) addresses and bare hostnames are accepted and never searched; unsupported schemes and embedded URL credentials are rejected.

Browser cookies/cache remain in %LOCALAPPDATA%\Bowser\WebView2, isolated from Edge's profile. Bowser has no hosted backend or first-party telemetry. WebView2's [Microsoft Defender SmartScreen reputation checking](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/data-privacy) is enabled where allowed by Windows settings; reputation checks may send visited URLs to Microsoft's service. Websites and the selected search provider receive normal network requests when used.

## Password migration status

`Ctrl+,` opens the themed Passwords & Passkeys panel. It can search by site or username, add and edit entries, delete after confirmation, reveal a password, copy it, generate a password, and preview/commit a user-selected password CSV. Exact duplicates and conflicts are classified before import; new entries are written only after confirmation. The encrypted vault and per-site “Never” choices are stored in the active profile's `Passwords.dat`. Guest profiles do not expose vault access or save prompts.

The vault uses Windows DPAPI for current-user at-rest protection. DPAPI does not prompt for Windows user presence when the vault opens, and reveal/copy/fill have no additional re-authentication in this version. DPAPI protection does not use per-profile entropy; profile separation currently relies on each profile's distinct vault file and directory. Revealed text hides after 15 seconds. Copied passwords are excluded from clipboard history and cloud upload where Windows honors the registered formats, and are cleared after about 30 seconds only if the clipboard still contains the value Bowser copied. Bowser does not export vault credentials. CSV exports contain plaintext passwords; Bowser reads the selected file locally and does not delete it. Keep the export protected and remove it yourself after verifying the imported sign-ins.

`Ctrl+Shift+L` opens a credential picker for the active HTTPS origin (HTTP is accepted only for `localhost`). Bowser fills only the top-level page after an explicit selection; it does not fill on page load or fill cross-origin frames. A submitted login on a site without a matching username, or with a changed password, can prompt to Save, Not now, or Never for this site. A login that matches the saved username and password (unchanged) produces no prompt. Choosing Not now suppresses further prompts for that origin for the rest of the session. Form detection and save prompts are best-effort for ordinary password forms. WebView2's built-in password autosave remains disabled.

Passkey creation and sign-in depend on the Windows WebAuthn platform and the installed WebView2 runtime. Bowser has not verified WebAuthn create/get in its embedded runtime. Passkeys are not part of CSV migration, and Bowser cannot import or export their private keys; use Windows Settings or the authenticator/provider that created them. See [the passkeys investigation](docs/passkeys-investigation.md) for the supported API surface and current evidence.

## Browser data migration

Choose **Import from another browser…** from the main menu to import bookmarks, or open **Passwords & Passkeys** to import a password CSV. Bowser detects local Chrome, Edge, Brave and Firefox profiles, but bookmark import reads only Chromium `Bookmarks` JSON files and user-selected Netscape bookmark HTML exports. Firefox bookmarks can be exported from Firefox as HTML first. The source file is opened read-only; Bowser never modifies or deletes browser data. Imported bookmarks are saved per Bowser profile in `bookmarks.txt` as `title<TAB>url<TAB>folder`, one UTF-8 entry per line. Tabs and newlines are stripped from fields, only HTTP(S) URLs are accepted, and normalized duplicate URLs are skipped.

Passwords are never read from browser databases. Export passwords using the source browser's own CSV export, then use Bowser's existing preview-and-consent vault import. Chrome and Edge password stores can use app-bound encryption; Bowser does not and cannot bypass that protection. History databases are not imported because Chrome, Edge and Firefox history is stored in SQLite and Bowser has no SQLite reader. Settings from other browsers are skipped because Bowser only supports its own `google` and `duckduckgo` search engine preference and does not currently map external settings safely. Bowser's `history.txt` format is reserved for future imports: `unix_ts<TAB>url<TAB>title`.

## Default website policy

Website permissions are denied unless you allow them for a site. Microphone, camera, location and notification requests can each be allowed or blocked per site from **Site information**, the connection icon at the left of the address pill. Bowser never prompts: a site with no stored choice is blocked, as are all other permission kinds. Choices are saved in `permissions.txt` in the profile folder and apply to the next request. WebView2 keeps no decisions of its own, so access a page already holds (a live microphone, camera or location) continues until the page reloads. Downloads are cancelled, and external application protocols are blocked. User-initiated HTTP(S) requests for another window open in a new tab; script popups are blocked. Web messages are enabled only for the password form detector's fixed message shapes, validated against the message source and current top-level URL; the native side never sends vault credentials or a credential API to page JavaScript. Host objects remain disabled. Bowser retains the runtime's sandbox, site isolation, certificate validation and update behavior.

Open **Privacy dashboard** from the main menu or press **Ctrl+Shift+Delete**. It reports only requests that Bowser's blocker actually stopped. The block list makes one combined decision and does not identify whether a match is an ad or a tracker, so Bowser reports one blocked-requests total. All-time, current-session, current-site, top tracked sites with blocked requests, and daily totals are shown. Daily buckets are bounded to 90 days and per-site history stores exact counts for up to 128 host names; a host outside that bounded set has no saved per-site count. The profile's human-editable `privacy-stats.txt` stores counters and host names only, never full URLs. **Clear statistics** removes those saved counters and the current-session count. The dashboard also shows the current site's connection and blocker state, the profile's reported tracking-prevention level, SmartScreen's configured state, sleeping-tab settings, and stored per-site permission decisions. Permission choices can be changed there; as with Site information, changes apply to the next request. Guest-profile statistics are ephemeral and disappear when the guest profile is closed.

The dashboard can clear cookies and site data, cache, or WebView2 browsing history for the active profile, using the selected time range. Bowser asks for confirmation and reports when WebView2 completes or returns an error. This does not clear saved passwords or passkeys. Clearing browser history affects WebView2's history; Bowser does not maintain a separate history file.

Bookmarks, hosted account management, and a download manager are not included. The main menu provides profiles, extensions, Passwords & Passkeys, the site blocker, the Privacy dashboard, and Settings. It is an experimental browser and the benchmark report describes its observed behavior and limits.

## Interface

The window has two rows over a translucent surface:

- **Tabs:** rounded tabs have individual favicons and titles, with a close button on the left that appears on hover. Matching sites and Ctrl-click opener links group together in a shared pill. A hover card offers group markers and related tabs. Hovering a site group's coloured bar opens its tab island menu: name it (Enter saves, Escape reverts), pick a colour, switch to any of its tabs, or close the whole island. Clicking the bar collapses the island to one tab, and clicking it again expands it. Right-click a tab to pin/unpin it. Its *Add tab to group* menu sets its site's grouping to *Always group this site*, *Group this site automatically*, or *Never group this site*, with a check on the current choice. An Always rule lasts only while the site has an open unpinned tab: when the last one closes or navigates elsewhere, or at startup when none is open, the rule returns to automatic grouping, and the site's name, colour and collapse state are kept. Pinned tabs stay at the left and do not join groups. Group preferences are stored in `%LOCALAPPDATA%\Bowser\tab-groups.txt`. The tab overflow picker appears only when tabs cannot all fit.

Tab organisation rules:

- **Automatic grouping** is on by default. Set `automatic_tab_grouping=false` in `settings.txt` (or untick *Group tabs from the same site automatically* in Settings, then restart) to stop new automatic groups. Groups you made by hand and Always groups still work. Automatic groups are a live projection of your tabs, so with the setting off they are not formed at the next launch.
- **Names:** an unnamed island suggests a name from its site, such as ChatGPT for chatgpt.com, or the capitalised first label of the domain otherwise. The suggestion appears as a hint in the island menu and is only saved if you type it yourself, so a name you chose is never replaced.
- **Placement:** drag a tab between two members of a site's group to join it. Drag a grouped tab elsewhere to take it out, or use *Remove from group* in the tab menu. A tab you place this way stays where you put it, and automatic grouping will not undo it. Placement is kept for the current session only, because tab identities do not survive a restart. Site modes, names, colours and collapse state are still saved across restarts.
- **Undo:** *Undo tab organisation* in the tab menu, or Ctrl+Shift+Z, reverts the last group, name, colour, mode or placement change. Up to 32 steps are kept. Collapse and expand are not undo steps. While the address field has focus, Ctrl+Shift+Z is left to the field's own redo; use *Undo tab organisation* in the tab menu to undo grouping there.
- **Stability:** pinned tabs are never grouped automatically. While a page is loading, its tab keeps the group it had before the navigation. The new site applies when loading finishes, so redirects do not make a tab jump between groups.

- **Toolbar:** navigation controls sit beside a centred address pill, with the main menu, new-tab button, and window controls at the right. The pill's left icon shows the connection state (padlock for HTTPS, open padlock for HTTP, warning for a certificate problem, info for local or unknown pages); clicking it opens Site information with the connection, per-site permissions, cookies and blocker state.
- **Picture in picture:** right-click a tab with a visible, playing video and choose **Picture in picture**, or press **Ctrl+Shift+Y** for the active tab. Bowser requests Chromium's native WebView2 video PiP; the command appears only when a compatible video is found. PiP availability and window behavior depend on the installed WebView2 Runtime and the website's video. Pausing or ending the video, Bowser's Stop command, page navigation and tab closure request PiP exit.
- **Settings:** open the main menu and choose Settings to configure sleeping tabs, the blocker, and automatic tab grouping. Save writes to the active profile's human-editable `settings.txt`, preserving comments and unknown keys. The search engine applies to the next search; other changes apply on the next launch. The window also provides Open settings file and Reset to defaults.

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

Shortcuts match modifiers exactly, so Ctrl+Alt combinations (AltGr on some keyboard layouts) never trigger a command.

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
| Passwords & Passkeys | Ctrl+, |
| Fill saved password | Ctrl+Shift+L |
| Undo tab organisation (group, ungroup, move and rename steps) | Ctrl+Shift+Z |
| Toggle split view | Ctrl+Shift+\ |
| Focus left / right split pane | Ctrl+Shift+Left / Ctrl+Shift+Right |
| Close focused split pane | Ctrl+Shift+W |

Drag a tab to the left or right half of the page to open it beside the selected tab. Drag the divider to resize panes; double-click it to restore equal widths. Split focus, pane selection, and divider ratio are included in crash recovery, which restores only the active page and the two visible split pages.

Under system memory pressure, a tab that has been hidden for 30 seconds or more may be unloaded to free memory. It stays in the tab strip with its title, favicon and group, and it reloads its last address when you select it again. Its back and forward history is lost when this happens. Pinned tabs, tabs playing audio and tabs that are still loading are never unloaded.

## Verification and benchmarks

    cargo test --locked
    cargo clippy --locked -- -D warnings
    powershell -NoProfile -ExecutionPolicy Bypass -File benchmarks\smoke.ps1
    powershell -NoProfile -ExecutionPolicy Bypass -File benchmarks\run.ps1 -Runs 5 -IdleSeconds 30

The benchmark controls foreground browser windows and sends input. Allow it to finish without using the keyboard/mouse. It creates disposable Bowser, Edge and Chrome profiles, uses identical local fixtures for page workloads, and records the whole process group's working set, private bytes and CPU time. Existing browser profiles are not used.

Fresh-profile and reused-profile launches both start a new process group. Windows filesystem/standby caches are left untouched, so these are not OS-reboot cold measurements. The first-page endpoint is a fixture's DOM completion plus two animation frames, detected through its title marker; it is a controlled usability proxy. Remote search behavior is checked separately because network/provider latency varies.

Raw results, software/hardware metadata and the measurement report are in benchmarks/results. Bowser's executable footprint is reported separately from the shared WebView2 installation and browser profile data. Summed working sets can count shared pages more than once; private bytes are also recorded. Direct battery energy is unmeasured unless an appropriate source is available.

The Phase 0 measurements, known limits, largest measured bottleneck, and next performance experiments are summarized in [docs/phase-0-results.md](docs/phase-0-results.md).

The shell uses direct windows and webview2-com bindings; url supplies URL parsing and encoding; psl provides the local Public Suffix List used for tab grouping; zeroize clears selected temporary credential buffers. Cargo.lock pins the complete dependency set. Benchmark code and optional event logging run only during explicit measurement launches.
