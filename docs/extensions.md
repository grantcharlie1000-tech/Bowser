# Browser extensions

## What WebView2 offers

Checked against the webview2-com 0.39.1 bindings in the cargo registry:

- `ICoreWebView2EnvironmentOptions6::SetAreBrowserExtensionsEnabled` turns extension support on
  when the environment is created.
- `ICoreWebView2Profile7::AddBrowserExtension(folder, handler)` loads an unpacked extension folder.
  The completion returns an `ICoreWebView2BrowserExtension`.
- `ICoreWebView2BrowserExtension` provides `Enable(bool, handler)` and `Remove(handler)`.
- Runtime on the development machine when the Stage 1 probe ran (2026-10-14): 155.0.4283.45, read
  by the engine's existing runtime probe (`GetAvailableCoreWebView2BrowserVersionString`). Bowser
  still queries each interface at runtime and degrades if one is missing.

## Limits, stated plainly

- WebView2 supports a subset of Chrome extension APIs. Each extension is only as compatible as the
  APIs it uses. Microsoft publishes no list of which ones.
- Only unpacked folders and `.crx` files chosen by the user are supported. Chrome Web Store install
  is not supported, and Bowser makes no Web Store compatibility claim.
- Bowser validates manifests itself (manifest version 2 or 3, name, version, permission lists). It
  is stricter than Chrome: non-text permission entries are refused. Declared permissions are shown
  to the user before install. Bowser does not enforce them; WebView2 does what it does.
- Enabling extensions is decided when the window's environment is created. A profile with no
  installed extension is created without the option. Installing the first extension in a running
  window reports "restart Bowser" when the option is absent.

### Runtime floor

Extensions are only enabled on a WebView2 runtime at or above **134.0.3124.44**. The engine reads the
version with the same runtime probe it already used, before the environment is created. Below the
floor, or when the version cannot be read, the option is not set, every enabled extension reports
why it did not load, and installing one in that window shows the same reason instead of the restart
advice.

The floor is inferred, not documented by Microsoft. Microsoft documents 120.0.2210.55 as the first
runtime with the extension API. 134.0.3124.44 is where a missing MV3 service-worker registration was
fixed (WebView2Feedback #5113, a secondary source).

### Install prompt

The confirmation lists the name, the version and a capped summary, then says that Bowser does not
enforce the permissions and cannot narrow them after install: WebView2 grants whatever the manifest
declares, so the answer is all or nothing.

- A name or permission string with a Unicode format character is refused, never cleaned up: bidi
  overrides and isolates, zero-width characters, the soft hyphen, a BOM, tag characters, and the line
  and paragraph separators. Ordinary non-ASCII text is fine. Names that use a zero-width joiner (some
  emoji sequences) are refused too.
- The summary starts with the real counts of each list, then a flag when any permission, site-access
  pattern or content-script match covers every website (`<all_urls>`, or `*`, `http` or `https` with
  host `*`), then site access and content-script pages, then permissions. Each list line shows the
  first items that fit, each cut to 56 characters, and "and N more" for the rest. The whole prompt
  stays under 2,400 characters however a manifest is padded.
- A localized name (`__MSG_extName__`) is shown as written. Bowser does not read `_locales`.

### Folder rules

- Any entry at the root of the extension folder whose name starts with `_` is refused, except a
  folder named exactly `_locales`. WebView2 documents `E_ACCESSDENIED` for names that start with `_`,
  and `_metadata` is the usual case. Bowser refuses these before anything is copied or loaded and
  never deletes or edits files inside a folder the runtime has loaded: the folder is used in place.
  The check is for the root only; deeper names are left to the runtime.
- Unknown manifest keys, including `key` and `default_locale`, are ignored by Bowser's parser, so
  they pass.

## Profile state across restarts

- The WebView2 profile can keep extensions added with `AddBrowserExtension`. Bowser's records file
  is the source of truth, so at startup, once the profile is ready, the engine lists the profile's
  extensions (`GetBrowserExtensions`) and reconciles them:
  - an extension with no Bowser record is removed;
  - a recorded but disabled extension is disabled;
  - an enabled one is turned on, reusing the profile's copy instead of adding a second one.
- Bowser ids (`ext-...`) are not WebView2's ids. WebView2 derives its own id for each unpacked
  folder. Bowser keeps the mapping in `extension_ids.txt` beside `extensions.txt`, written when an
  extension is added and removed when it is unloaded.
- Removing an extension that is not loaded in this session looks up its WebView2 id in the profile
  and unloads it before its folder is deleted. If the lookup fails, the removal fails and the
  folder is kept.
- An extension is excluded from the startup load if it is removed before the profile is ready.
- A profile whose `extension_ids.txt` exists is reconciled even when no extension is enabled, so
  an extension removed while disabled does not linger.

Uncertainty, stated plainly:

- Persistence across a restart was observed once on a throwaway profile (see Verification status):
  a second process listed the extension, reused it with `Enable` (no second `Add`), and its worker
  answered with the same id and the `chrome.storage.local` value the first process's worker wrote.
  That is consistent with persistence; it does not show whether `onInstalled` ran again.
- If `extension_ids.txt` is lost or corrupt, a profile extension cannot be matched to its record.
  It is removed at startup and re-added as a fresh load, so its extension-held storage may reset.
  That path was not run live.
- Ids are compared as WebView2 reports them. Every id observed was 32 lowercase letters `a` to `p`,
  which Bowser's id parser accepts (ASCII alphanumeric). If a runtime changes that format, the
  mapping misses and the same fallback applies. The id comes from the folder path unless the
  manifest has a `key` (see Verification status), so Bowser's copy keeps its id only while it stays
  at the same path.
- A fresh WebView2 profile already lists two built-in extensions, "Microsoft Clipboard Extension"
  and "Microsoft Edge PDF Viewer". They have no Bowser record, so startup reconcile asks WebView2 to
  remove them, and `Remove` fails with `E_FAIL` (0x80004005). Nothing breaks, but the failure is
  silent outside the trace, and the log line for them shows the WebView2 id in its `bowser=` field.
  This was observed once and not changed.

## Diagnostics

Set `BOWSER_EXTENSION_TRACE` to any value and each extension call prints one line to stderr, in the
same opt-in style as `BOWSER_SUGGEST_TRACE`. The benchmark event log cannot carry these lines (it
takes only fixed lowercase names), and benchmark launches never load extensions. A debug build
prints to its console; a release build needs stderr redirected.

    extension call=add bowser=ext-4ed890b433d806cc result=ok hresult=0x00000000 id=akdelnaehijbgbkgdphgbhinfakfkjef name="Bowser Probe MV3"

- `call` is `add`, `enable`, `disable`, `remove`, `list` (the `GetBrowserExtensions` result) or
  `list_item` (one extension in that list).
- `result` is `ok`, `failed`, or `no_extension` (success reported without an extension).
- `id` and `name` are WebView2's. Names are cut to 64 characters, and control characters, format
  characters, quotes and backslashes become `?`, so a name cannot forge a line. File names and file
  contents are never logged.
- Two start-up lines: `call=runtime_floor` (the runtime version, the floor and whether extensions
  were enabled) and `call=profile7_cast` (`supported=true|false`). The environment option cannot
  report runtime support, because its options object is webview2-com's own; the `Profile7` cast is
  the real check.

## Verification status

- Unit tests cover manifest parsing (malicious and malformed input), size and depth caps, link and
  junction refusal, id and path containment, the records file and removal. For Stage 1 they also
  cover the probe fixtures (`tests/fixtures/extensions`: accepted or refused as intended), the root
  `_` rule, format-character refusal, the summary cap, the runtime floor and the log lines.
- **Live runs, 2026-10-14, runtime 155.0.4283.45, each on fresh temp directories, no window shown.**
  The ignored test `engine::extension_probe_live` is the only harness. One Stage 1 run (an earlier
  version of the test) proved that a local MV3 extension executes. Four runs of the current test
  followed: the first covered the scheme, restart, variants and `.crx`; the second added the hosted
  popup and options pages; the third fixed the probe's own report (it had mis-read a done flag) and
  added `chrome.webview` and `chrome.tabs` detail; the fourth is the last code, after a refactor.
  The runs agree on every result below. The one thing run 1 could not show, the popup, comes from
  runs 2 to 4. Samples come from the fourth run. Each run uses a new temp folder, so the baseline
  fixture gets a new id each time.
  - `AddBrowserExtension` on `probe-mv3`: success, `hresult=0x00000000`, name `Bowser Probe MV3`,
    id `kmhpblkcjnahkkljmkccepegpaieacpj` (32 letters `a` to `p`). `chrome.runtime.id` in the
    worker's reply was the same string. The `ICoreWebView2Profile7` cast is supported.
  - The content script ran (`<html data-bowser-probe="content-script">` read back through the
    engine; `content-script` beacon received). The service worker ran: the `worker-reply` beacon
    carried its answer, including the value it wrote to `chrome.storage.local` in `onInstalled`.
    The test loads the page after the add finishes, so it does not show whether a page that loads
    while the extension is still being added gets the content script.
  - **Extension URL scheme: `chrome-extension`.** `chrome.runtime.getURL('popup.html')` from the
    worker returned `chrome-extension://kmhpblkcjnahkkljmkccepegpaieacpj/popup.html`, and a hosted
    page's `location.origin` was `chrome-extension://<the same id>`. `extension:` and
    `ms-browser-extension:` were not seen anywhere; whether either works as an alias was not
    tested, and Bowser refuses both either way.
  - **Id after a real process restart: the same.** The test re-ran itself as a second process on
    the same profile and folder. The second process listed the extension (`list_item` with the same
    id), reused it with `enable` instead of `add`, and its worker's `chrome.runtime.id` matched. The
    worker also returned `{probe: "installed", variant: "probe-mv3"}` from `chrome.storage.local`.
    The id comes from the folder path: the baseline fixture got a different id in every run (a new
    temp folder each time), and the `key` fixture got the same id in every run.
  - `probe-mv3-locales` (`default_locale`, `__MSG_extName__`, `_locales/en/messages.json`): load
    `hresult=0x00000000`. WebView2 reported the resolved name `Bowser Probe MV3 Locales`, while
    Bowser's own record keeps the raw `__MSG_extName__`. Its worker answered.
  - `probe-mv3-key`: load `hresult=0x00000000`, id `ciocoifocbggchcecjgibbaeojegkpen`, which **equals**
    the id computed from the fixture's `key` (`tests/fixtures/extensions/README.md`) in every run.
    WebView2 honours a manifest `key`. Its worker answered.
  - `probe-mv3-metadata`: **refused by Bowser, not WebView2.** `extensions::install` returned
    `ReservedRootName` before anything was copied or loaded, so the runtime's own handling of a root
    `_metadata` folder was not observed.
  - A path ending `.crx` (a real 8-byte file in temp: the `Cr24` magic and a version word) passed to
    `AddBrowserExtension`: `hresult=0x80070002` (`ERROR_FILE_NOT_FOUND`, "The system cannot find
    the file specified") although the file exists. The API takes a folder. This says nothing about
    a real package.
  - Fresh profiles still list `Microsoft Clipboard Extension` and `Microsoft Edge PDF Viewer`, and
    removing them still fails with `E_FAIL` (0x80004005). Not changed.
  - The Stage 1 stop rule was not triggered. None of this shows that any particular real-world
    extension works.
- Hosted popup and options pages: see the section below for what was observed.
- Unit tests cover the page-path and page-URL rules, the exact-URL predicate, the one-shot grant,
  the frame rule and the error texts (`extensions::` and `engine::webview2::`).
- Not verified: the extensions-menu entry itself (no real window was driven; it compiles, and the
  engine path behind it ran in the probe with the window hidden); a hosted page in a shown window; the hosted page's frame, new-window, permission, download and external-protocol
  handlers (unit tests cover only the navigation and frame rules); what `chrome.tabs.query` returns
  when a private tab or chrome view exists; removal of an extension while its page is open;
  content scripts in private or chrome-view profiles; any `chrome.*` API beyond `runtime`,
  `storage.local`, message passing and `tabs.query`; real-world extensions.

To repeat the live check (PowerShell, from the repo root). It starts WebView2 processes but shows no
window, uses only temp directories, and never touches `%LOCALAPPDATA%\Bowser*`. Redirecting
`LOCALAPPDATA` as well keeps anything else the runtime writes out of the real profile:

    $env:LOCALAPPDATA = Join-Path $env:TEMP "bowser-probe-localappdata"
    New-Item -ItemType Directory -Force $env:LOCALAPPDATA | Out-Null
    $env:BOWSER_EXTENSION_TRACE = "1"
    cargo test --lib --no-run
    & (Get-ChildItem target\debug\deps\bowser-*.exe | Where-Object Extension -eq .exe |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1) `
        live_probe_mv3_extension_executes --ignored --nocapture --test-threads=1

(Run the built test binary directly: with `LOCALAPPDATA` redirected, `cargo` would use a different
cache.) The test re-runs itself once as `live_probe_restart_child` for the restart check; run that
test alone and it does nothing.

## Hosted popup and options pages

WebView2 draws no extension UI, so Bowser opens an extension's own pages itself. This is a minimal
host; the toolbar button and the management section are described under Managing extensions.

- **Source of the page.** Bowser's copy of `manifest.json` is read for `action.default_popup` (the
  popup) or `options_ui.page`, else `options_page` (the options page). Manifest V2's
  `browser_action` is not read. The path must be plain and relative: segments of ASCII letters,
  digits, `.`, `_` and `-` joined by `/`, at most 256 characters, with no empty, `.` or `..`
  segment, no scheme, drive, backslash, query, fragment or percent escape, and no segment ending in
  a dot. It must name a plain file reached through plain folders inside the copy (links are
  refused). Anything else is refused with a plain message and nothing opens.
- **The address** is exactly `chrome-extension://<WebView2 id>/<path>`, with the scheme observed in
  the live probe and the 32-letter id WebView2 reported for that extension in this window.
- **The window.** One top-level Bowser window holds one WebView2 controller in the tab environment,
  on the default (non-private) profile, so the page shares storage with the extension's worker. It
  has no web-message handler, `IsWebMessageEnabled` and host objects are turned off and read back
  (the page is not opened if either stays on), and permission requests, downloads, new windows and
  external-protocol launches are denied. Opening another page closes the open one; closing the
  engine closes it. The blocker and the privacy counters do not apply to it.
- **Navigation.** A one-shot grant covers that one URL (`HostedPagePolicy`). The first navigation to
  it uses the grant up; any other address, or the same one again (so a reload), is cancelled. Frames
  may be `about:blank`, `about:srcdoc` or a page of the same extension; nothing else. This is not a
  scheme unblock: tabs keep refusing every extension scheme (see Navigation policy).
- **Menu.** The Ctrl+Shift+E menu has "Open extension popup…", a submenu listing enabled extensions
  whose manifest names a popup (disabled when none does). An options page can be opened by the
  engine call but has no menu entry yet. A refusal is shown in a message box and is not recorded as
  a load failure.
- **Observed in the live probe (`probe-mv3`, hidden window).** Both the popup and the options page
  loaded at `chrome-extension://<id>/popup.html` and `/options.html`. In both, `chrome.runtime` was
  defined and its id matched, `chrome.storage.local.get` returned the value the worker wrote, and a
  click on the page's button made `chrome.runtime.sendMessage` return the worker's reply with no
  `lastError`. So hosted pages do have `chrome.*`; the Stage 2 go/no-go is **go**. A script that set
  `location.href` to a loopback page left the address unchanged.
  - `chrome.tabs.query({})` from the page returned two entries, both `active: true`,
    `incognito: false`, in different windows, with a string `url`. The first kept its id across the
    popup and options runs; the second changed each time a hosted page was opened, so it is
    inferred to be the hosted page's own WebView. No private tab or chrome view existed in that
    run, so nothing is known about whether a hosted page can see them.
  - `window.chrome.webview` exists on the hosted page, with `postMessage`, `hostObjects` and six
    other members, although Bowser turned off both settings and registers no message handler.
    Whether a `postMessage` from the page goes anywhere was not tested at first; it was then, see
    the next item.
  - **`window.chrome.webview` is inert on a hosted page (runtime 155.0.4283.45, temp profile, popup
    and options).** A `postMessage` with a distinct string returned without error and reached no
    host handler: a test-only `WebMessageReceived` observer on the hosted WebView saw nothing, and
    the same page script reached that observer when web messages were deliberately left on
    (positive control), so the observer can see one. `hostObjects` has no own members beyond
    `prototype`; a sync and an async call on eight guessed names failed with `Access is denied
    (0x80070005)`. Bowser registers no host object and no web-message or frame-message handler on
    this WebView, and a source test fails if production code adds one. This is what was observed,
    not a sandbox boundary: the object is there and its safety rests on nothing being registered,
    so a later change that adds a handler to this WebView would hand it to the extension's page
    script. It was observed only for the top-level page, not for a frame inside it.
  - Not a Chrome Web Store claim and no `chrome.*` shim: the page gets whatever the runtime gives it.

## Managing extensions

The settings page has an Extensions section (`bowser://settings/extensions`), reached from the
settings nav, from "Manage extensions" in the Ctrl+Shift+E menu, and from the toolbar button's list.
It is drawn by the settings page's own host; there is no `chrome://extensions` page and no `chrome.*`
shim.

- **List.** Each installed extension: name, version, on or off, a load failure from this session if
  any, a one-line summary of what it can reach, and on request what its manifest declares (site
  access, content-script matches, permissions, optional permissions and site access). The
  declarations are read from Bowser's copy of `manifest.json`, the one the user approved at install;
  each list sends at most 8 items of at most 64 characters with its full count. Bowser shows these
  and does not enforce or narrow them; the page says so.
- **Actions.** The switch turns an extension on or off (`set_extension_enabled` in
  `src/win32/window.rs`): the flag changes and the engine loads or unloads the existing copy, with no
  prompt and nothing read from the original folder, so turning one back on grants nothing new.
  Remove asks in the row first, then unloads it and deletes the copy (the page shows "Removing…"
  until the engine answers; one removal at a time). "Install unpacked…" opens Bowser's folder picker
  and the native confirmation that lists the declarations. "Install CRX file…" does the same for a
  `.crx` file on disk (see "CRX file" below). "Open popup" opens the hosted popup. Bowser installs
  unpacked folders and `.crx` files the user picks, and has no store install.
- **Wire** (`src/chrome_host/extensions_page.rs`). Rust -> page:
  `{v:1,t:"snap",p:"extensions",d:{state:"on"|"error",busy,ack,error,items:[{id,name,version,enabled,popup,failure,readable,all,sites,sitesN,scripts,scriptsN,perms,permsN,optional,optionalN}]}}`.
  Page -> Rust, in the settings page's `set` family: `extension` (`text` id, `on`), `extremove`
  (`text` id), `extpopup` (`text` id), bare `extinstall` and bare `extinstallcrx`. An id must be `ext-` and 16 lowercase hex
  digits and is looked up in `extensions.txt` again before anything acts on it. Every command is
  answered with a snapshot (`ack`: `none enabled disabled removed installed failed`), even when
  nothing changed.
- **States.** Loading (before the first snapshot), empty (with "Choose folder…" and "Install CRX
  file…" buttons of the same weight), an
  unreadable list (`state:"error"`, "Try again"), and a snapshot the page refuses ("could not be
  shown", "Try again").
- **Toolbar button.** Left of the download button's slot, shown only while an enabled extension
  whose manifest names a popup has not failed to load this session. Rust reserves
  `EXT_BUTTON_SLOT` (32 CSS px) beside the pill and sends `{v:1,t:"extbtn",on,n,name}`; a click
  sends `{v:1,t:"ext",a:"open",text:"x,y,w,h"}`. With one such extension the click opens its popup
  and the tooltip is its name; with several, a menu of their names hangs from the button with
  "Manage extensions" under it. The glyph is Bowser's puzzle; there is no badge, action icon or
  side panel, because WebView2 exposes none of them.
- **Updates.** Install, toggle, removal and every engine load result post `WM_EXTENSIONS_CHANGED`
  to every window, which refreshes its toolbar button and its settings page.

### CRX file

"Install CRX file…" (settings page, and the Ctrl+Shift+E menu) installs one `.crx` file that is
already on this computer. It makes no network request and is not a store install; Bowser does not
say or imply the file came from the Chrome Web Store.

1. Bowser's own file picker (`*.crx`) chooses the file; the page never names a path. A file over
   144 MiB (`crx_file::MAX_CRX_FILE_BYTES`) is refused before it is parsed.
2. `crx::verify_crx3_declared` reads the id the file declares and runs the full `verify_crx3` against
   it, so the id is accepted only if a verified proof was made by the key that derives it. A
   changed byte, a missing proof or an id its signer does not own fails, and nothing is installed.
   A passing signature says the file is intact and made by the key that owns its id, not who that is.
3. `unpack_stamped` unpacks into a new empty temporary folder and writes the developer key into
   `manifest.json`, so the id is the CRX's id wherever the copy lives.
4. `extensions::install_confirmed` copies that folder into the profile and shows the same permission
   prompt as an unpacked install. Declining deletes the copy and records nothing; accepting loads
   the copy as after a folder install. The temporary folder is deleted when the call ends.

Code: `src/crx_file.rs`; the picker and host action are in `src/win32/window.rs`
(`install_extension_from_crx_file`) and `src/win32/settings_page.rs`.

## Navigation policy

The `ms-browser-extension` scheme stays blocked for user-initiated navigations. Loading an
unpacked extension does not require opening that scheme from the address bar.

The live probe observed `chrome-extension` as the extension page scheme and Bowser still unblocks
nothing for tabs. In a tab, `chrome-extension:` and `extension:` are not on the block list: a
navigation to either goes down the external-protocol path, which only cancels the OS launch.
Frames, popups and the omnibox refuse both. A test records the tab policy for all three schemes, so
a later change is a visible decision. Hosted pages use their own exact-URL, one-shot policy
(`HostedPagePolicy`) instead of a scheme allowance, and it takes no part in tab navigation. Its
tests cover `chrome-extension` (the only scheme that can be granted, and only as one exact URL) and
`extension` and `ms-browser-extension` (refused, even with the same id and path).

## Where the code lives

- `src/extensions.rs`: validation, records file, install and remove, install prompt, load-result
  store, popup and options page paths and URLs.
- `src/engine/webview2.rs`: environment option, runtime floor, startup loading, runtime enable and
  remove, trace lines, the hosted-page navigation policy and the open-page entry point.
- `src/engine/extension_page.rs`: the Bowser-owned window and controller for a hosted page.
- `src/engine/extension_probe_live.rs`: the ignored live probe.
- `src/win32/window.rs`: Ctrl+Shift+E menu (including "Open extension popup…"), folder picker,
  install confirmation.
- `src/app.rs`: `App::open_extension_page`, the call from the menu to the engine.
- `src/chrome_host/extensions_page.rs`: the Extensions section's snapshot, its commands and the
  toolbar button message; `src/win32/settings_page.rs` runs the commands.
- `ui/src/extensions-section.ts`, `ui/src/extensions-logic.ts`: the section; `ui/src/view.ts`: the
  toolbar button. `ui/shots/extensions-render.mjs` renders both with a fake host.
- `tests/fixtures/extensions/`: the probe fixtures and their README.

## Store fetch status

`src/crx_fetch.rs` turns an extension id or a Chrome Web Store URL into verified CRX bytes and a stamped unpack folder, and is not wired to any UI. It follows the protocol Chromium's `ExtensionDownloader` uses: a GET to `https://clients2.google.com/service/update2/crx` (the only fixed host) returns an update manifest (`<gupdate>` XML), and the CRX is downloaded from the `codebase` URL inside it. The query is Chromium's: `os`, `arch`, `prod=chromiumcrx` (the unbranded value; `chromecrx` is Chrome's and is never sent), `prodchannel`, `prodversion` (the WebView2 runtime), `lang`, `acceptformat=crx3` and `x=id%3D<id>%26v%3D0.0.0.0%26uc`. `acceptformat` leaves out Chromium's `puff` because Bowser cannot apply it; a manifest that offers only a puff package is refused with its own error. `response=redirect` is not in Chromium's source and is no longer sent. The request carries the `X-Goog-Update-Updater: chromiumcrx-<prodversion>`, `X-Goog-Update-AppId` and `X-Goog-Update-Interactivity: fg` protocol headers and the user agent `Bowser-CrxFetch/<version>`, never a Chrome identity. The manifest is read the way `extensions/browser/updater/safe_manifest_parser.cc` reads it, for three things only: the `<app appid>` (it must be the requested id), the `<updatecheck status>` (`noupdate`, HTTP 204 or an empty body mean no package) and its `codebase` (HTTPS, a host, no credentials). The codebase download keeps the size cap, the HTTPS-only redirect rule and the five-hop limit; the bytes then go through `verify_crx3` for the requested id and `unpack_stamped` into the caller's empty folder, and a bad signature leaves that folder empty. Live result, 2026-10-14, one attempt: uBlock Origin (`cjpalhdlnbpafiamejdnhcphjbkeiagm`) with `prod=chromiumcrx`, `prodversion=155.0.4283.45` and no `response=redirect` got HTTP 200 and a 333-byte manifest for the requested app id with `<updatecheck status="noupdate"/>` and no `codebase`. So the earlier HTTP 204 answers were this same "no package" verdict, and the endpoint does speak this protocol to a non-Chrome client; it simply has no package to give for this id and runtime version. A second attempt the same day changed only `prodversion`, to Chrome stable `155.0.8059.80` (Windows, release note 2026-10-13), and got the same answer: HTTP 200, a 333-byte manifest for the requested app id with `<updatecheck status="noupdate"/>` and no `codebase`. That closes the unbranded store-download experiment. Nothing was downloaded, verified or unpacked in either attempt, and `chromecrx` was not tried. Store fetch is therefore implemented and covered by loopback tests (manifest accepted, `noupdate`, a different app id, an HTTP codebase and puff-only refused, the CRX download staying on `127.0.0.1`, the production manifest host being only `clients2.google.com`), but not yet shown to return a real CRX: that needs an id the store still offers for this runtime, and has not been observed live.
