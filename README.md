# Bowser 0.0.1

A small experimental x64 Windows browser: one native window, one native address/search input, and a WebView2 viewport. Its goal is useful browsing with few application components and measured resource use.

## Stable download

The clickable application is [Bowser.exe](https://github.com/grantcharlie1000-tech/Bowser/releases/download/v0.0.5/Bowser.exe) from the [v0.0.5 release](https://github.com/grantcharlie1000-tech/Bowser/releases/tag/v0.0.5). It is a 64-bit Windows 10 or 11 program. Windows needs the installed [Evergreen WebView2 Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/#download-section); current Microsoft Edge already includes it.

Opening that file starts this exact build. The copy in this repository is `Open-Bowser.cmd`. It does not compile the source tree. It checks `%LOCALAPPDATA%\Programs\Bowser\Bowser.exe` against `stable/release.json`, downloads the release above when the hash does not match, and creates Desktop and Start Menu shortcuts that open the installed file directly. If that file is still in use, the pinned release is written to the next free folder under `%LOCALAPPDATA%\Programs` (`Bowser Stable`, then `Bowser Stable 2`, and so on) and the shortcuts point there. The open window is left running. Close it, then use the Bowser shortcut. Browser profiles stay in `%LOCALAPPDATA%\Bowser`. Replacing the executable keeps that data.

This build is not Authenticode-signed. Windows SmartScreen may ask you to choose More info, then Run anyway.

`Open-Bowser.cmd` checks that pin on GitHub before it installs. The installed Bowser also checks it after it has stayed open for about a minute, and then at most once an hour. It downloads a different build only when the SHA-256 matches the pin, and switches to it the next time Bowser starts. A build that fails to stay open is rolled back. Browser data is kept. The main menu's footer shows the state, and it offers **Restart to update** once a build is downloaded (the open tabs come back after the restart). **Check for updates** and **Update automatically** are in Settings, Updates. Turning automatic updates off stops the background check and drops a build that was waiting to install; a check you run yourself still downloads and installs. The switch covers every profile and is stored in `%LOCALAPPDATA%\Bowser\auto-update.txt`. A copy downloaded before this updater existed needs one reinstall from the release above, or one run of `Open-Bowser.cmd`. After that, Bowser keeps itself on the pinned build.

## Dev Bowser

`Dev-Bowser.cmd` is the working copy. It builds the source in this folder, opens a separate window titled Dev, and keeps that window's profile in `%LOCALAPPDATA%\Bowser-Dev`. It does not read or replace the installed Bowser. Leave the command window open: when the source changes, it rebuilds and reopens the dev window, so a change made by an agent shows up there. Close the command window to stop rebuilding. The dev window stays until you close it.

`Start-Bowser.cmd` builds once and opens that same dev profile. It does not keep watching.

## Build and run

Requirements: Windows 10 or later (x64), Rust with the MSVC target, Visual Studio C++ Build Tools and Windows SDK, and the installed [Evergreen WebView2 Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/#download-section).

Build from this directory:

    cargo build --release --locked

Use `Open-Bowser.cmd` to install or refresh the stable app. Use `Dev-Bowser.cmd` while changing this tree. Building with `cargo build --release --locked` produces `target\release\bowser.exe` and does not replace the installed Bowser or its profile.

The window starts empty with the input focused. Enter a web address or search phrase. Alt+Left and Alt+Right navigate history. The engine starts when you begin typing, while no network request is sent until you press Enter.

The shared WebView2 runtime is required but is not bundled into Bowser. Runtime availability is checked and failures are shown as native error messages. Microsoft manages Evergreen security updates.

## Search and local data

Address bar searches use one of two engines: **Google** (`https://www.google.com/search?q={query}`) or **DuckDuckGo** (`https://duckduckgo.com/?q={query}`). To choose, open Settings (main menu, or type `bowser://settings/search`) and pick the engine under Search engine. The choice is saved at once and applies to the next search with no restart, and is stored as `search_engine=google` or `search_engine=duckduckgo` in the profile's `settings.txt`.

Nothing changes on upgrade. A profile with no saved choice keeps its previous behaviour: a valid `search-provider.txt` is used if present, and otherwise the search goes to Bing (`https://www.bing.com/search?q={query}`). A saved choice takes precedence over `search-provider.txt`; to use the file again, choose Reset settings in Settings > About Bowser or delete the `search_engine` line. To send searches to another site, create %LOCALAPPDATA%\Bowser\search-provider.txt containing a single HTTPS URL template with exactly one {query}, for example:

    https://www.google.com/search?q={query}

Missing or invalid configuration uses the default. Search text is percent encoded as one query value, so spaces, `&`, `#`, `+`, `%` and non-ASCII text are sent exactly as typed. Explicit HTTP(S) addresses and bare hostnames are accepted and never searched; unsupported schemes and embedded URL credentials are rejected.

Browser cookies/cache remain in %LOCALAPPDATA%\Bowser\WebView2, isolated from Edge's profile. Bowser has no hosted backend or first-party telemetry. WebView2's [Microsoft Defender SmartScreen reputation checking](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/data-privacy) is enabled where allowed by Windows settings; reputation checks may send visited URLs to Microsoft's service. Websites and the selected search provider receive normal network requests when used.

### Isolated data directory (`--data-dir`)

All persistent state (history, bookmarks, workspaces, tab groups, settings, session and `running.lock`, permissions, site preferences (zoom), privacy stats, password vault, profiles, `last-profile.txt`, Guest folders, updater files, `crash.log`, WebView2 data) lives under one app-data root. By default that is `%LOCALAPPDATA%\Bowser`. `--data-dir <absolute path>` (or the `BOWSER_DATA_DIR` environment variable; the flag wins) replaces that root for the whole process, and windows it spawns inherit it. Because the session lock and updater mutex are keyed by the root, an instance with its own data directory never hands off to, refuses because of, or overwrites a running Bowser that uses the real one.

Benchmark launches (`--benchmark-log` and friends) can never write to the real profile: with no `--data-dir` they use a fresh throwaway directory under `%TEMP%` (`bowser-benchmark-data-<pid>-<nonce>`, deleted on exit), and an explicit `--data-dir` that is or sits inside `%LOCALAPPDATA%\Bowser` is refused.

## Agent interface

`--agent` is an optional loopback API for an agent that needs to read a page as text and run page scripts. A launch without `--agent` does not listen and opens the same window as before. There is no new toolbar, setting, or menu item.

Start a hidden host. It does not open the human browser. `--data-dir` is required and must not be `%LOCALAPPDATA%\Bowser` or `%LOCALAPPDATA%\Bowser-Dev`, so the agent never uses your profile. The process prints one JSON line, `{"event":"listening","addr":"127.0.0.1:47321"}`, and stays up.

    cargo build --locked
    target\debug\bowser.exe --agent --data-dir C:\temp\bowser-agent

The listener binds to `127.0.0.1` port `47321` (override with `--agent-port` and `--agent-bind`). `--agent-bind` accepts only a loopback IP literal (`127.0.0.1` or `::1`). `0.0.0.0`, LAN addresses, and hostnames are refused. A connection whose peer is not loopback is refused. Do not expose this port past the machine. There is no authentication token: any process on this machine can connect. Anything that can connect can run arbitrary JavaScript in the page as the local user, with that user's cookies and files the page can reach. The release binary is a Windows GUI app, so redirect stdout to read the listening line. A debug build writes it to the console.

Refs live on `window.__bowserAgent` in the page. The page can replace that object, so a ref is only as trustworthy as the page you just snapshotted. `click` and `press` dispatch DOM events. They are not operating-system input, and a site can ignore them when it requires a real user gesture. One command runs at a time. `script` returns when the function returns and does not wait for a Promise. Cookies and `localStorage` stay in the session folder until that `--data-dir` is deleted. The host refuses to start while `WEBVIEW2_USER_DATA_FOLDER` or the other WebView2 override variables are set. A second host cannot open a session folder that is already open.

From another terminal, each command prints one JSON object:

    target\debug\bowser.exe --agent-call navigate --session alpha --url http://127.0.0.1:8765/page.html
    target\debug\bowser.exe --agent-call snapshot --session alpha
    target\debug\bowser.exe --agent-call fill --session alpha --ref e1 --text Ada
    target\debug\bowser.exe --agent-call click --session alpha --ref e2
    target\debug\bowser.exe --agent-call snapshot --session alpha

A snapshot looks like `{"ok":true,"url":"http://127.0.0.1:8765/page.html","title":"Agent fixture","tree":"http://127.0.0.1:8765/page.html\nAgent fixture\ntextbox \"Name\" e1\nbutton \"Save\" e2"}`. The tree is the URL, the title, and short rows (`role "name" eN`). On the fixture page the name field is `e1` and Save is `e2`, because refs follow document order. Refs are assigned when you snapshot and last until the next snapshot or navigation. Use the refs from the snapshot you just took. `text` returns the main text; add `--format markdown` for a heading plus that text. Other commands: `press --key Enter`, `script --expression document.title`, `back`, and `stop` (shuts the host down). `click` also accepts `--selector` instead of `--ref`.

The same `--session` name keeps cookies, `localStorage`, and back/forward history, because that name is its own WebView2 folder under `<data-dir>\agent-sessions\<name>`. A different name does not see that state. `navigate` runs `location.assign` and `back` runs `history.back()`. `click`, `fill`, `press`, and `script` run in the page. They are not simulated keystrokes.

`script` returns the value once: a string comes back as `"result":"hello"`, a number as `"result":42`. A thrown script is an error when the installed runtime supports `ExecuteScriptWithResult`. On the older `ExecuteScript` fallback, a throw and a real `null` both come back as `"result":null`. `click`, `fill`, and `press` succeed only when the page script returns `{"ok":true}`. A snapshot that comes back null is reported as a failure. Failures are one JSON object on stderr, `{"ok":false,"error":"..."}`, and a non-zero exit. The human browser still uses its normal error dialog.

The same calls can be one JSON object per TCP connection to `127.0.0.1:47321`, answered by one JSON line: `{"op":"snapshot","session":"alpha"}`, `{"op":"fill","session":"alpha","ref":"e2","text":"Ada"}`.

## Password migration status

`Ctrl+,` opens the themed Passwords & Passkeys panel. It can search by site or username, add and edit entries, delete after confirmation, reveal a password, copy it, generate a password, and preview/commit a user-selected password CSV. Exact duplicates and conflicts are classified before import; new entries are written only after confirmation. The encrypted vault and per-site “Never” choices are stored in the active profile's `Passwords.dat`. Guest profiles do not expose vault access or save prompts.

The vault uses Windows DPAPI for current-user at-rest protection. DPAPI does not prompt for Windows user presence when the vault opens, and reveal/copy/fill have no additional re-authentication in this version. DPAPI protection does not use per-profile entropy; profile separation currently relies on each profile's distinct vault file and directory. Revealed text hides after 15 seconds. Copied passwords are excluded from clipboard history and cloud upload where Windows honors the registered formats, and are cleared after about 30 seconds only if the clipboard still contains the value Bowser copied. Bowser does not export vault credentials. CSV exports contain plaintext passwords; Bowser reads the selected file locally and does not delete it. Keep the export protected and remove it yourself after verifying the imported sign-ins.

`Ctrl+Shift+L` opens a credential picker for the active HTTPS origin (HTTP is accepted only for `localhost`). Bowser fills only the top-level page after an explicit selection; it does not fill on page load or fill cross-origin frames. A submitted login on a site without a matching username, or with a changed password, can prompt to Save, Not now, or Never for this site. A login that matches the saved username and password (unchanged) produces no prompt. Choosing Not now suppresses further prompts for that origin for the rest of the session. Form detection and save prompts are best-effort for ordinary password forms. WebView2's built-in password autosave remains disabled.

Passkey creation and sign-in depend on the Windows WebAuthn platform and the installed WebView2 runtime. Bowser has not verified WebAuthn create/get in its embedded runtime. Passkeys are not part of CSV migration, and Bowser cannot import or export their private keys; use Windows Settings or the authenticator/provider that created them. See [the passkeys investigation](docs/passkeys-investigation.md) for the supported API surface and current evidence.

## Browser data migration

Choose **Import from another browser** in Settings (You and Bowser) to import bookmarks, or open **Passwords & Passkeys** to import a password CSV. Bowser detects local Chrome, Edge, Brave and Firefox profiles. Bookmark sources include Chromium `Bookmarks` JSON, Firefox's newest automatic `bookmarkbackups/*.jsonlz4` backup, and user-selected Netscape bookmark HTML exports. The source file is opened read-only; Bowser never modifies or deletes browser data. Imported bookmarks are saved per Bowser profile in `bookmarks.txt` as `title<TAB>url<TAB>folder`, one UTF-8 entry per line. Tabs and newlines are stripped from fields, only HTTP(S) URLs are accepted, and normalized duplicate URLs are skipped.

Passwords are never read from browser databases. Export passwords using the source browser's own CSV export, then use Bowser's existing preview-and-consent vault import. Chrome and Edge password stores can use app-bound encryption; Bowser does not and cannot bypass that protection. **History import is not supported (browser history databases are SQLite).** Chromium's `Preferences` JSON can map only Google or DuckDuckGo as Bowser's search engine; supported values are previewed and applied only after confirmation. Other external settings are skipped and identified in the summary. The first-run import offer appears after the first chrome paint and once the first tab reports usable content, for each non-guest profile; any choice is remembered. Bowser's `history.txt` format remains `unix_ts<TAB>url<TAB>title` for Bowser's own history only.

## Default website policy

Website permissions are denied unless you allow them for a site. Microphone, camera, location, notification and popup permissions can each be allowed or blocked per site from **Site information**, the connection icon at the left of the address pill, or from the **Privacy dashboard**. Bowser never prompts: a site with no stored choice is blocked, as are all other permission kinds, and a stored Allow counts only for that exact origin (scheme, host and port). Choices are saved in `permissions.txt` in the profile folder and apply to the next request. WebView2 keeps no decisions of its own, so access a page already holds (a live microphone, camera or location) continues until the page reloads. Downloads from normal tabs and web app panels are saved (see the Downloads bullet below); private tabs and Bowser's own pages cancel them. External application protocols are blocked. User-initiated HTTP(S) requests for another window (a link click) always open in a new tab. Windows a page opens from script are blocked unless you chose **Allow** for popups on that site; even then only HTTP(S) addresses open, and always as a tab, never a separate window. Web messages are enabled only for the password form detector's fixed message shapes, validated against the message source and current top-level URL; the native side never sends vault credentials or a credential API to page JavaScript. Host objects remain disabled. Bowser retains the runtime's sandbox, site isolation, certificate validation and update behavior.

Open the **Privacy dashboard** from Settings (Privacy and security) or press **Ctrl+Shift+Delete**. It reports only requests that Bowser's blocker actually stopped. The block list makes one combined decision and does not identify whether a match is an ad or a tracker, so Bowser reports one blocked-requests total. All-time, current-session, current-site, top tracked sites with blocked requests, and daily totals are shown. Daily buckets are bounded to 90 days and per-site history stores exact counts for up to 128 host names; a host outside that bounded set has no saved per-site count. The profile's human-editable `privacy-stats.txt` stores counters and host names only, never full URLs. **Clear statistics** removes those saved counters and the current-session count. The dashboard also shows the current site's connection and blocker state, the profile's reported tracking-prevention level, SmartScreen's configured state, sleeping-tab settings, and the permissions dashboard. The dashboard lists every site with a stored choice, the current site, and any site that was recently refused, each with **Allow**, **Block** and **Default** for camera, microphone, location, notifications and popups (Default stores nothing, so the site is blocked). A search box filters the sites, **Reset site** clears one site's choices, and **Reset all** clears every site after an in-page confirmation. A **Recently blocked requests** list shows the last 50 refused permission requests and blocked script popups as site, kind and time. It is kept in memory only, never written to disk, never holds page content, and is gone when Bowser closes. As with Site information, changes apply to the next request. Guest-profile statistics are ephemeral and disappear when the guest profile is closed.

The dashboard can clear cookies and site data, cache, or WebView2 browsing history for the active profile, using the selected time range. Bowser asks for confirmation and reports when WebView2 completes or returns an error. This does not clear saved passwords or passkeys. Clearing browser history affects WebView2's history; Bowser does not maintain a separate history file.

Hosted account management is not included. The main menu is short: new tabs, History, Downloads, bookmarking, workspaces, the site blocker, reader view, a More tools submenu (notes, command palette, saved sessions, extensions, Passwords & Passkeys) and Settings; profiles, import, the Privacy dashboard and updates are in Settings. It is an experimental browser and the benchmark report describes its observed behavior and limits.

## Private tabs

Choose **New private tab** from the main menu, the command palette, or press **Ctrl+Shift+P** (this chord used to open the profile menu, which moved to **Ctrl+Shift+M**). A private tab belongs to the current window and workspace like any other tab, shows a private glyph on the tab and in the address pill, and is created InPrivate (`ICoreWebView2ControllerOptions.IsInPrivateModeEnabled`) in one shared, in-memory WebView2 profile named `BowserPrivate`. All private tabs of a window share that profile, so a site you sign in to in one private tab is signed in in the others; it is separate from the browsing profile and from Bowser's own chrome profile. Pages opened from a private tab (link and script popups that Bowser allows) are private too.

What a private tab never does:

- **History, suggestions and favicons:** nothing is added to `history.txt`, to the address-bar suggestions, or to any favicon store.
- **Cookies, storage and cache:** held in memory by WebView2 and discarded when the last private tab of the window closes (or the window exits or crashes). Nothing is written to the profile's WebView2 folder.
- **Sessions:** private tabs are left out of the crash-recovery session, named sessions and the tab lists saved with workspaces, so they are not restored after a restart.
- **Tab groups:** a private tab is never in a group, so a private site never reaches `tab-groups.txt` (site modes, names, colours, collapse). Grouping choices are not offered on its tab menu, and it cannot be duplicated or moved to another window (a new window is a new process that would not be private).
- **Permissions:** every permission request is denied. Stored site choices do not apply and none can be saved from a private tab; Site information lists them as blocked. Downloads are cancelled and popups follow the usual policy.
- **Blocker and statistics:** the blocker is always on in a private tab and ignores both the global switch and per-site exceptions (the main-menu item is shown checked and disabled), and blocked requests are not counted in the Privacy dashboard or `privacy-stats.txt`.
- **Tracking prevention:** forced to Strict for the private profile, and a private tab is not created if Strict cannot be applied and read back.
- **Sign-ins:** no save prompts are offered; its pages get no message channel for the sign-in detector. Filling a saved sign-in with Ctrl+Shift+L remains your explicit choice.
- **Sleeping tabs:** a private tab is never discarded (closing its controller could end the in-memory profile); it can still be suspended.

A bookmark is the one thing a private tab may write, and only when you add it yourself (Ctrl+D). Extensions are installed into the browsing profile only and do not run in private tabs. Private tabs are not a defence against the network, the sites you visit, your employer or Microsoft Defender SmartScreen reputation checks (which may still see visited addresses); they only keep Bowser from keeping a local record. Guest windows (`--guest`) remain separate: a new process with its own temporary profile folder that is deleted on exit. Private tabs are the lighter choice when you want one private page in your normal window; Guest is the choice for handing the whole browser to someone else.

## Interface

The window has two rows over a translucent surface:

- **Tabs:** rounded tabs have individual favicons and titles, with a close button on the left that appears on hover. Matching sites and Ctrl-click opener links group together. Expanded groups sit inside a shared shell; their tabs are 5% shorter, and a coloured ball at the left opens the group menu or collapses the group. A collapsed group keeps the selected tab and fans the hidden members out as overlapping balls. When selection moves outside the group, it settles to a compact capsule. Click the ball to reopen the group, or choose a hidden member from the menu. The menu lets you rename the group (Enter saves, Escape reverts), pick a colour, switch to any member, or close the whole group. Right-click a tab to pin/unpin it or choose Auto, Always, or Never for its site's grouping. Pinned tabs stay at the left and do not join groups. Group preferences are stored in `%LOCALAPPDATA%\Bowser\tab-groups.txt`. The tab overflow picker appears only when tabs cannot all fit.

Tab organisation rules:

- **Automatic grouping** is on by default. Set `automatic_tab_grouping=false` in `settings.txt` (or turn off *Group tabs by site* in Settings > Appearance, then restart) to stop new automatic groups. Groups you made by hand and Always groups still work. Automatic groups are a live projection of your tabs, so with the setting off they are not formed at the next launch.
- **Names:** an unnamed island suggests a name from its site, such as ChatGPT for chatgpt.com, or the capitalised first label of the domain otherwise. The suggestion appears as a hint in the island menu and is only saved if you type it yourself, so a name you chose is never replaced.
- **Placement:** drag a tab between two members of a site's group to join it. Drag a grouped tab elsewhere to take it out, or use *Remove from group* in the tab menu. A tab you place this way stays where you put it, and automatic grouping will not undo it. Placement is kept for the current session only, because tab identities do not survive a restart. Site modes, names, colours and collapse state are still saved across restarts.
- **Undo:** *Undo tab organisation* in the tab menu, or Ctrl+Shift+Z, reverts the last group, name, colour, mode or placement change. Up to 32 steps are kept. Collapse and expand are not undo steps. While the address field has focus, Ctrl+Shift+Z is left to the field's own redo; use *Undo tab organisation* in the tab menu to undo grouping there.
- **Stability:** pinned tabs are never grouped automatically. While a page is loading, its tab keeps the group it had before the navigation. The new site applies when loading finishes, so redirects do not make a tab jump between groups.

- **Toolbar:** navigation controls sit beside a centred address pill, with the main menu and window controls at the right; in the default layout the new-tab button sits at the far right end of the tab row. The pill's left icon shows the connection state (padlock for HTTPS, open padlock for HTTP, warning for a certificate problem, info for local or unknown pages); clicking it opens Site information with the connection, per-site permissions, cookies and blocker state.
- **Picture in picture:** right-click a tab with a visible, playing video and choose **Picture in picture**, or press **Ctrl+Shift+Y** for the active tab. Bowser requests Chromium's native WebView2 video PiP; the command appears only when a compatible video is found. PiP availability and window behavior depend on the installed WebView2 Runtime and the website's video. Pausing or ending the video, Bowser's Stop command, page navigation and tab closure request PiP exit.
- **Reader view:** press **Ctrl+Alt+R**, click the reader button (a capital A beside text lines) at the right of the address pill, or choose **Reader view** in the main menu or the command palette. The page is restyled in place: the article is found with a small scoring heuristic (article, main and role=main blocks, then the paragraph-dense block with the lowest link density), everything else (navigation, sidebars, footers, forms, frames, ads, cookie banners, sticky bars, overlays) is hidden, and the text is set in a 68-character column at 19 px with a 1.6 line height. Images, figures, tables and code inside the article stay. A small bar at the bottom of the page changes the text size (A-, A+) and the theme (Auto follows Bowser's theme; Light, Sepia and Dark are fixed); the choice is remembered until Bowser closes. Leave with Ctrl+Alt+R again, **Esc**, or the pressed toolbar button; any navigation also leaves it. Nothing is navigated or fetched, so the address and back/forward history do not change, and leaving restores the page exactly. The toolbar button appears only when the page looks like an article (at least 250 words of text in one block with no more than 35% link text). The only automatic work is one read-only check after a page finishes loading in the active tab; nothing is injected when a document is created, and reader view never runs on `bowser-ui:`, `about:`, `file:`, `data:` or other non-web pages. **Hide distractions** (main menu or command palette) is the lighter mode: it hides fixed and sticky overlays, cookie banners and dialogs and releases a page's scroll lock without restyling the text. If a page has no article-like text, reader view says so and changes nothing. See [docs/reader.md](docs/reader.md).
- **Settings** is a page in a tab, `bowser://settings`: choose Settings in the main menu or the command palette, or type the address (sections: `bowser://settings/profile`, `/appearance`, `/privacy`, `/performance`, `/search`, `/downloads`, `/media`, `/updates`, `/about`). An open settings tab is reused rather than duplicated. The page has a section list, a search box that filters the settings as you type (Ctrl+F or `/` jumps to it), and sections for the profile (manage profiles, import), appearance (the four browser layouts, sidebar width, automatic tab grouping), privacy and security (blocker, sites it skips, tracking prevention, history on or off, the Privacy dashboard), performance (efficiency mode, sleeping tabs), the search engine, downloads (the save folder, and whether to ask where to save each file), media support, updates (Update automatically, Check for updates, Restart to update) and About (version, Open settings file, Reset settings). Every change is saved at once to the active profile's human-editable `settings.txt` (comments and unknown keys are kept) and a small "Saved" confirms it. Layout, sidebar width, efficiency, sleeping tabs, search, history and updates apply immediately; rows marked *Applies after restart* (blocker, its sites, tracking prevention, automatic grouping) take effect on the next launch. The **Media** section checks, the first time you open it, which video formats this browser can play (H.264, VP9, AV1, HEVC), whether decoding is reported as hardware or software, and whether Widevine and PlayReady are available, with text-only advice such as installing the Windows HEVC or AV1 video extension for hardware decode; it uses the browser's own capability APIs and makes no network request. The page is drawn by Bowser itself, not loaded from the web: it is never saved in history or offered as a site in the address bar, a restored session brings a settings tab back as the page, and web pages cannot open or embed it. When the window runs the native (non-HTML) chrome, Settings opens the older native Settings window instead.
- **History** is a page in a tab, `bowser://history`: press **Ctrl+H**, click History on the rail, choose History in the main menu, or type the address. An open history tab is reused rather than duplicated. Visits are grouped by day (Today, Yesterday, then dated days) with each page's icon, title, address and time; the search box filters as you type (Ctrl+F or `/` jumps to it, Esc clears it), and the list refreshes by itself when a visit is recorded while the page is open. Click a row to open it in the tab, Ctrl+click or middle-click (or the row's button) to open it in a new tab, and press **Delete** (or the row's bin button) to remove it; **Undo** (or Ctrl+Z) puts it back. Up/Down move through the rows and Enter opens one. **Clear browsing data** opens the Privacy dashboard. When history is switched off for the profile the page says so and links to Settings > Privacy and security; a guest window shows that guest sessions never save history. Like Settings, the page is drawn by Bowser itself: it is never recorded in history or suggestions, never opens in a private tab, comes back with a restored session, and web pages cannot reach it. Without the HTML chrome, History opens the side panel as before.
- **Downloads** work like Chrome's. A download from a normal tab or web app panel saves at once to your Downloads folder (a name clash becomes `name (1).ext`), or to the folder chosen in Settings > Downloads; with **Ask where to save each file** on, a Save dialog comes first and cancelling it cancels the download. A button appears at the right of the address pill with a progress ring while anything downloads (a check mark and a dot when one has finished), and a bubble under it lists the five latest downloads with live progress, speed and time left, and Pause, Resume, Cancel, Show in folder, Copy link, Retry and Remove. The bubble opens by itself when a download starts (without taking the keyboard from the page), closes by itself after a few seconds unless you are using it, and is toggled by the button. **Ctrl+J**, the main menu's Downloads row, the bubble's footer or the address `bowser://downloads` open the full page in a tab: downloads grouped by day with search, the source site, status and the same actions, **Open downloads folder**, **Clear all** (confirmed in the page) and **Undo** for a removal. The list is kept in `downloads.txt` in the profile folder (newest 500; a download still running when Bowser closes comes back as `Failed, Interrupted`); a guest window keeps it in memory only, and private tabs never download. Opening or showing a file uses only the path Bowser recorded; a file you deleted reads `File moved or deleted`. Without the HTML chrome downloads still save and Ctrl+J opens the download folder in Explorer.

- **Efficiency mode** (`efficiency_mode=auto|on|off`, default `auto`; Settings > Performance) trims background work on laptops. Auto is active while the PC runs on battery or Windows Battery Saver is on; it is re-read on power-status broadcasts, with a slow 60 second fallback timer, and applies without a restart. While active, hidden tabs are asked to suspend after 10 s instead of 30 s, the tab sleep delay is capped at 120 s (a shorter configured delay is kept, and Sleep hidden tabs off stays off), background tabs are held at the low memory target, and Bowser's own process runs at EcoQoS while the window is minimised. Audible, picture-in-picture, capturing, loading, pinned and active tabs are never slept.
- **Tracking prevention** (`tracking_prevention=balanced|strict`, default `balanced`; Settings > Privacy and security) is applied when the engine starts, so a change takes effect after a restart. The Privacy dashboard shows the level the browser reports now and notes a pending change.

The browser layout is chosen in Settings > Appearance (`bowser://settings/appearance`): `tab_layout=horizontal` (toolbar above tabs, the default), `tabs-top` (tabs above toolbar), `single-row` or `vertical` (tabs in a sidebar). `tab_layout` and `sidebar_width=180..360` are stored per profile; the vertical viewport accounts for the sidebar at the current display DPI. In vertical mode, drag the sidebar edge to resize, use the mouse wheel to scroll, and use the workspace name at the top to switch, create, rename, or delete workspaces. The main menu also contains a Workspaces submenu. Tabs belong to one workspace and horizontal mode shows only the active workspace's tabs. Other workspaces use the existing hidden-tab sleep lifecycle; restored tabs load when selected. Guest workspaces stay in memory for that guest session.

An empty tab shows the desktop through the window. Real pages render opaque, as before. Horizontal tab islands keep their existing visuals; vertical tabs render as a sidebar list with group headers.

## Glass material and fallback

On Windows 11 (build 22621 or later), the window uses the system transient backdrop. On Windows 10, it uses a tinted acrylic accent; set `BOWSER_GLASS_VARIANT=blur` to use plain blur-behind instead. The development machine runs Windows 10, so the Windows 11 path has not been checked on Windows 11.

Bowser uses an opaque themed fill with no transparency when any of these apply:

- transparency effects are off (`EnableTransparency` is `0` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`);
- the session is remote;
- high-contrast mode is on;
- the system backdrop call fails.

Set `BOWSER_FORCE_SOLID=1` to force the solid fallback regardless of these settings. Light and dark follow the Windows app mode (`AppsUseLightTheme`) and update when the system colour setting changes.

## Tabs and keyboard shortcuts

Shortcuts match modifiers exactly, so Ctrl+Alt combinations (AltGr on some keyboard layouts) never trigger a command. The one exception is Ctrl+Alt+R (reader view), which means AltGr+R also toggles reader view on layouts where AltGr reports as Ctrl+Alt.

| Action | Shortcut |
| --- | --- |
| New tab (focuses the address field) | Ctrl+T |
| New private tab | Ctrl+Shift+P |
| Profile menu (switch, create, delete) | Ctrl+Shift+M |
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
| Command palette | Ctrl+Shift+K |
| Notes & Tasks | Ctrl+Shift+N |
| Add bookmark | Ctrl+D |
| History page (`bowser://history`) | Ctrl+H |
| Downloads page (`bowser://downloads`) | Ctrl+J |
| Privacy dashboard | Ctrl+Shift+Delete |
| Extensions | Ctrl+Shift+E |
| Zoom in / out | Ctrl+Plus (Ctrl+=, Ctrl+Numpad Plus) / Ctrl+Minus (Ctrl+Numpad Minus), or Ctrl+mouse wheel |
| Reset zoom to 100% | Ctrl+0 (Ctrl+Numpad 0) |
| Reader view (leave with the same chord, Esc, or the toolbar button) | Ctrl+Alt+R |

**Page zoom** follows Chrome's levels (25, 33, 50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300, 400, 500 percent) and is remembered per site: the zoom you set on an origin (scheme, host and port) is saved in `site-prefs.txt` in the profile folder and applied again when you navigate to that origin or restart Bowser. A zoomed page shows a chip with the percentage inside the address pill (click it to return to 100%), and **Site information** shows the same value with a Reset button. Guest sessions remember zoom for the session only and write nothing to disk. The file holds `<origin> zoom <percent>` lines; unknown or malformed lines are ignored, and it is capped at 256 KiB and 2048 sites.

Drag a tab to the left or right half of the page to open it beside the selected tab. Drag the divider to resize panes; double-click it to restore equal widths. Split focus, pane selection, and divider ratio are included in crash recovery, which restores only the active page and the two visible split pages.
Notes, tasks, bookmarks, history, and named tab sessions are stored locally in each profile. Guest profiles keep notes and tasks in memory for the lifetime of the window; they do not write productivity data to disk. The command palette searches open tabs, bookmarks, history, and browser commands. History can be disabled per profile in `history-enabled.txt`.

The command palette is a live overlay: results for tabs, bookmarks, history, and browser commands update as you type. Use Up/Down and Enter, Escape, or click a result. Ctrl+Shift+N opens the Notes & Tasks sidebar docked to the right of the page. Notes have a title and body editor; tasks can be added, edited, checked off, and deleted. Drag the sidebar's left edge to resize it. Its `notes_panel_width` setting is stored in that profile's `settings.txt`; Guest keeps notes, tasks, and width only in memory.

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
