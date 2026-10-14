# WebView2 loads extensions; Bowser must host them

Researched 2026-10-14. Research only: no extension, CRX or binary was downloaded or run.

Versions considered:

- WebView2 Win32 reference pages for Release SDK 1.0.4258.31 (Runtime 154, 2026-09-28) and Prerelease SDK 1.0.4312-prerelease (Runtime 156, 2026-10-12). The notes diffed the Profile7 page across the two and found only `ms.date` differs ([N]); the other extension pages were reported as the same text.
- The extension API has been stable since Release SDK 1.0.2210.55, which "requires WebView2 Runtime 120.0.2210.55 (Dec. 11, 2023) or later" ([SDK 1.0.2210.55 notes](https://learn.microsoft.com/en-us/microsoft-edge/webview2/release-notes/sdk/1-0-2210-55), [V]).
- Bowser pins `webview2-com =0.39.1`. Its bindings declare `CORE_WEBVIEW_TARGET_PRODUCT_VERSION` = `145.0.3800.47` (`webview2-com-sys` `src/bindings.rs`; the bundled loader's file version is 1.0.3800.47, so SDK 1.0.3800.47 is the inferred source). They contain exactly the stable extension interfaces and nothing else ([capability audit](extensions-capability-audit.md)).
- Installed runtime on the dev machine: 155.0.4283.45 per the audit (read from the registry on 2026-10-14). `docs/extensions.md` still says 154.0.4258.62; the audit is newer.

How to read the evidence tags:

- **[V]** means this report re-fetched the page on 2026-10-14 and read the quoted text verbatim.
- **[N]** means the claim comes from the research notes, whose fetch tool summarised the page. Anything marked [N] and not in quotation marks is a paraphrase ("summary, not verbatim").
- Status words in the API table are: Supported per Microsoft, Unsupported per Microsoft, Secondary report, Not stated in official docs.
- "Secondary" means a GitHub issue, Microsoft Q&A answer, vendor blog or third-party report. Microsoft staff comments in issue threads are stronger than random users but are not documentation.
- Three load levels are kept apart throughout. **L1 loaded**: `AddBrowserExtension` returned success. **L2 core runs**: a content script or service worker was seen executing. **L3 feature works**: a given user-facing feature of the extension works. Bowser's host can observe L1 only. No claim in this report goes past what its evidence level supports, and none of it is a Chrome Web Store compatibility claim.

## Executive summary

**WebView2 can run real Chromium extensions, but it is an engine with no browser around it, so Bowser has to build everything the user touches.** Microsoft documents that `AddBrowserExtension` takes an unpacked folder and that the extension "is running right after installation". It also documents that "icon, badge, pop up" entry points will be missing, and nowhere lists which `chrome.*` APIs work ([Profile7](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2profile7), [V]). There is no CRX or store entry point and no host API to open a popup or options page ([notes](https://github.com/MicrosoftDocs/webview2-win32-reference/tree/main/1-0-4312-prerelease), [N]). Microsoft staff call store installation "by design" unsupported ([#3694](https://github.com/MicrosoftEdge/WebView2Feedback/issues/3694), [V]). The realistic ways a Windows browser runs genuine Chrome extensions are: (1) WebView2 plus a host-built pipeline, (2) CEF with Chrome-style browsers, (3) Electron plus GPL or paid third-party libraries, (4) an own Chromium build. Only (1) keeps Bowser's native Win32 window, HTML chrome, tab model, privacy blocker and small footprint, and it is the only one where extension execution is already wired into this codebase. Bowser has never loaded an extension in a live window, so Stage 1 of the plan is a local-fixture proof. If it fails, this recommendation falls and the engine question reopens with real evidence.

**Blocker for one-click Web Store install, in one sentence:** WebView2 gives the host no CRX or store install entry point (`AddBrowserExtension` accepts only an unpacked folder), so the store page's own "Add" button cannot install into Bowser.

**Closest feasible path:** the user supplies an extension id or a Web Store URL, and Bowser fetches the CRX3, verifies it, unpacks it to a private folder, asks consent, and calls `AddBrowserExtension`. Bowser's own chrome can offer an "Install" button when the active tab is a store listing, parsing the id from the URL in Rust. Remaining work for true store-page install is listed in the recommendation.

**Recommendation (one architecture):** stay on WebView2 and make Bowser own acquisition (a CRX pipeline) and UI (consent, toolbar entry, Bowser-hosted popup and options pages). No in-page shim and no engine swap.

## 1. AddBrowserExtension takes an unpacked folder, never a CRX

**Answer.** The official docs describe an unpacked directory only. A `.crx` is never mentioned in the Microsoft WebView2 pages the notes searched (release notes, concept pages, the 1.0.4312-prerelease reference, the design spec) ([N]). What happens on a CRX path is **not stated in official docs**.

The whole host surface is four interfaces plus completion handlers:

- **Opt-in.** `ICoreWebView2EnvironmentOptions6::put_AreBrowserExtensionsEnabled` (Rust: `SetAreBrowserExtensionsEnabled`). It defaults to FALSE. With FALSE, new extensions cannot be installed and already-installed ones are unavailable. Connecting to an already-running environment with a different value fails with `HRESULT_FROM_WIN32(ERROR_INVALID_STATE)` (summary, not verbatim) ([EnvironmentOptions6](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2environmentoptions6), [N]).
- **Profile.** `ICoreWebView2Profile7::AddBrowserExtension(LPCWSTR extensionFolderPath, handler)` and `GetBrowserExtensions(handler)`. The latter returns a snapshot that does not update after later installs or removals.
- **Extension object.** `ICoreWebView2BrowserExtension` exposes `Id`, `Name`, `IsEnabled`, `Enable(BOOL, handler)` and `Remove(handler)`. `ICoreWebView2BrowserExtensionList` exposes `Count` and `GetValueAtIndex`.

Verbatim from the Profile7 page ([Profile7](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2profile7), [V], page `ms.date` 2026-09-21):

> Adds the browser extension using the extension path for unpacked extensions from the local device.

> The extension folder path is the topmost folder of an unpacked browser extension and contains the browser extension manifest file. If the `extensionFolderPath` is an invalid path or doesn't contain the extension manifest.json file, this function will return `ERROR_FILE_NOT_FOUND` to callers.

| HRESULT (verbatim) | Documented meaning (verbatim) |
| --- | --- |
| `HRESULT_FROM_WIN32(ERROR_NOT_SUPPORTED)` | Extensions are disabled. |
| `HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND)` | Cannot find `manifest.json` file or it is not a valid extension manifest. |
| `E_ACCESSDENIED` | Cannot load extension with file or directory name starting with "_", reserved for use by the system. |
| `E_FAIL` | Extension failed to install with other unknown reasons. |

**What a CRX path does.** Not documented. The documented rule ("invalid path or doesn't contain the extension manifest.json") implies a `.crx` file path returns `ERROR_FILE_NOT_FOUND`. That is an inference and untested. `ERROR_NOT_SUPPORTED` is the option-off error, not a wrong-format error: a Microsoft engineer asked a user who got "The request is not supported" whether the option was set ([#3748](https://github.com/MicrosoftEdge/WebView2Feedback/issues/3748), [N], Secondary). A user who passed a `.zip` reported it did not work and gave no HRESULT ([#3766](https://github.com/MicrosoftEdge/WebView2Feedback/issues/3766), [N], Secondary). A Microsoft collaborator wrote: "You would need an unpacked extension, and you will call this API on it." ([#3694](https://github.com/MicrosoftEdge/WebView2Feedback/issues/3694), 2024-02-01, [V]).

**Persistence and in-place reading.** Verbatim from the same page [V]:

> During installation, the content of the extension is not copied to the user data folder. Once the extension is installed, changing the content of the extension will cause the extension to be removed from the installed profile. When an extension is added the extension is persisted in the corresponding profile. The extension will still be installed the next time you use this profile.

The same page adds that adding the same extension from the same folder path reinstalls it, and that when two extensions have the same Id "only the later installed extension will be kept". `ICoreWebView2BrowserExtension` says `isEnabled` "is persisted per profile" and "The removal is persisted and future uses of this profile will not have this extension installed." ([BrowserExtension](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2browserextension), [V]). The opt-in is per environment, so each launch must set it again for previously installed extensions to run.

**Versions.** Stable in Release SDK 1.0.2210.55. That notes page lists "Support for browser extensions in WebView2" under "Promotions to Phase 3 (Stable in Release)" with the text "Supports browser extensions in WebView2." ([SDK 1.0.2210.55 notes](https://learn.microsoft.com/en-us/microsoft-edge/webview2/release-notes/sdk/1-0-2210-55), [V]). Before that: experimental in 1.0.1988-prerelease (Runtime 117), stable-in-prerelease in 1.0.2194-prerelease ([N]). **Evergreen versus Fixed Version:** not stated in official docs. No extension page distinguishes them. A Fixed Version runtime of 120.0.2210.55 or later should expose the same interfaces; that is UNCERTAIN and untested.

**Where Microsoft's own pages conflict.** The "Differences between Microsoft Edge and WebView2" table has the row `| Browser Extensions | Off | No | This feature is turned off. |`, and its legend says "Configurable indicates that you can turn on or off the feature using WebView2 APIs or command-line switches." ([browser-features](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/browser-features), [V], `ms.date` 2025-11-14). "Off" matches the API default. "Configurable: No" contradicts `AreBrowserExtensionsEnabled`. This report relies on the API reference and the stable release note. They name the opt-in, the error codes and the semantics, and Microsoft's own sample uses them. Both pages were edited recently, so staleness cannot be inferred from dates. The most plausible reading is that the row describes Edge's own extension experience (toolbar, store, management page), which is indeed absent. UNCERTAIN.

**Where Bowser stands today** ([extensions.md](extensions.md), [audit](extensions-capability-audit.md)):

- It uses exactly this surface for user-chosen unpacked folders.
- It copies the folder into its own `Extensions\<id>` folder, inspects the copy, and loads the copy.
- It reconciles with `GetBrowserExtensions` at startup.
- One code fact matters: `SetAreBrowserExtensionsEnabled` is called on webview2-com's own Rust options object, so its failure branch can never fire. Real support detection is the later `Profile7` cast.

*Cite (fetched 2026-10-14):* [Profile7](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2profile7), [BrowserExtension](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2browserextension), [SDK 1.0.2210.55 notes](https://learn.microsoft.com/en-us/microsoft-edge/webview2/release-notes/sdk/1-0-2210-55), [browser-features](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/browser-features), [#3694](https://github.com/MicrosoftEdge/WebView2Feedback/issues/3694).

## 2. Microsoft publishes no chrome.* support list for WebView2

**Answer.** **Not stated in official docs.** The notes found no Learn page, API remark, spec or release note that lists supported or unsupported `chrome.*` APIs for WebView2 ([N]). The Overview of WebView2 APIs only says an app can embed a WebView2 control that uses a Microsoft Edge extension, and lists the host classes (summary, not verbatim; [overview](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/overview-features-apis), [N]). Microsoft's Edge page says Edge extensions use a subset of the Chromium JavaScript methods and has a per-API table (summary, not verbatim). That page covers the Edge browser, not WebView2, and must not be read as a WebView2 list ([Edge API support](https://learn.microsoft.com/en-us/microsoft-edge/extensions/developer-guide/api-support), [N]).

The only WebView2-specific official statements are listed below. Everything else in the table is a GitHub issue or third-party report.

- The UI entry points are missing: "Extensions that are designed to include any UI interactions (e.g. icon, badge, pop up, etc.) can be loaded and used but will have missing UI entry points due to the lack of browser UI elements to host these entry points in WebView2." [V]
- `Id` is "the same browser extension ID returned by the browser extension API `chrome.runtime.id`" [V].
- `Name` resolves a localized manifest name [V].
- Release-note items cited in the table.

| API or feature | Status | Evidence and notes |
| --- | --- | --- |
| action / browserAction (icon, badge, popup) | Unsupported per Microsoft (UI entry points only) | Verbatim sentence above [V]. Behaviour of the API calls themselves is not stated. The host cannot read icon or popup path; request #3856 is open ([#3856](https://github.com/MicrosoftEdge/WebView2Feedback/issues/3856), [N]). |
| storage (local, sync, session) | Not stated in official docs | No WebView2 source. `storage.sync` has no sync backend if "Profile and Identity \| Off \| No" is read literally ([browser-features](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/browser-features), [V]); inference. |
| scripting | Not stated in official docs | No evidence either way. |
| Content scripts (manifest-declared) | Secondary report | A Microsoft engineer's comment after a 2023 loading fix says other tested extensions worked (paraphrase, [#3728](https://github.com/MicrosoftEdge/WebView2Feedback/issues/3728), [N]). Third-party tests report Dark Reader and uBlock Origin Lite working; these are author-reported and unverified ([Flutter issue #10](https://github.com/Lukas-Bohez/ConvertTheSpireFlutter/issues/10), [N]). |
| MV3 service workers | Secondary report | A service-worker extension was missing from `edge://serviceworker-internals` on WebView2 135 canary. It was fixed in 134.0.3124.44 and 135.0.3163.0 ([#5113](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5113), [N]). The Runtime 134 note says extensions are no longer disabled by `AddBrowserExtensionAsync` whether or not developer mode is on (summary, not verbatim; [Runtime 134](https://learn.microsoft.com/en-us/microsoft-edge/webview2/release-notes/runtime/134), [N]). Keepalive and wake behaviour: not stated. |
| declarativeNetRequest | Not stated in official docs | Wanted as crucial by a browser developer ([#3258](https://github.com/MicrosoftEdge/WebView2Feedback/issues/3258), [N]). One weak third-party signal that a blocker extension blocked a test pixel (author-reported). |
| webRequest / webRequestBlocking | Not stated in official docs | In Chrome MV3, blocking is policy-install only ([Chrome webRequest](https://developer.chrome.com/docs/extensions/reference/api/webRequest), [N]). |
| tabs (query, get, events) | Secondary report | Every WebView2 in the profile appears as a tab. Ids are not mapped to host WebView2s, and "active" flags are wrong ([#4607](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4607), [#3854](https://github.com/MicrosoftEdge/WebView2Feedback/issues/3854), [#3855](https://github.com/MicrosoftEdge/WebView2Feedback/issues/3855), [N]). |
| tabs.create / window opened by an extension | Supported per Microsoft (narrow: event routing only) | Runtime 123.0.2420.47: fixed `NewWindowRequested` not firing when opened by a browser extension (issue #3841) ([Runtime 123](https://learn.microsoft.com/en-us/microsoft-edge/webview2/release-notes/runtime/123), [N], summary). What `chrome.tabs.create` returns to the extension: not stated. |
| windows | Not stated in official docs | No evidence. |
| contextMenus | Supported per Microsoft (surfacing only) | `ICoreWebView2ContextMenuItem::get_Name` says extension menu items are named "extension" ([ContextMenuItem](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2contextmenuitem), [N], summary). API fidelity not stated. |
| sidePanel | Not stated in official docs | Needs browser UI. WebView2 turns off the Edge Sidebar shortcut (Ctrl+Shift+E) [V]. Secondary: a side-panel page opened in a separate WebView2 reportedly never worked (paraphrase; [#5019](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5019), [N]). |
| alarms | Not stated in official docs | No evidence. |
| notifications | Not stated in official docs | The web-platform table row for Push Notifications reads "This feature isn't implemented in WebView2." [V]. That is the web feature, not `chrome.notifications`. |
| cookies | Not stated in official docs | No evidence. |
| downloads | Not stated in official docs | The host `DownloadStarting` API [V] is not `chrome.downloads`. |
| identity | Not stated in official docs | WebView2 has "Profile and Identity \| Off \| No" [V] and "Google has disabled Google Authentication in embedded webviews, which includes WebView2" [V]. Edge-only list marks `getAuthToken` and `getAccounts` unsupported. |
| management | Not stated in official docs | `edge://extensions` is a blocked URL [V]. |
| runtime.id | Supported per Microsoft | Verbatim `Id` sentence above [V]. |
| runtime.sendMessage / connect | Not stated in official docs | No WebView2 report of success or failure. |
| runtime.openOptionsPage | Not stated in official docs | Microsoft treats opening or managing options as a feature request ([#4608](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4608), [N], paraphrase). |
| externally_connectable | Not stated in official docs | No evidence. |
| permissions.request | Not stated in official docs | No prompt UI exists; see item 3. |
| native messaging | Secondary report | A Microsoft Q&A moderator says it is unsupported in Teams' WebView2 control; this is Teams-specific ([Q&A](https://learn.microsoft.com/en-us/answers/questions/1691245/support-of-browser-extensions-with-native-messagin), [N]). |
| i18n (localized manifest name) | Supported per Microsoft (name only) | `Name` returns the localized name [V]. `chrome.i18n` behaviour: not stated. |
| DevTools panels | Secondary report | Microsoft's sample extension is a DevTools-panel extension ([sample](https://github.com/MicrosoftEdge/WebView2Samples/tree/main/SampleApps/WebView2APISample/assets/extensions/example-devtools-extension), [N]). A user reports React DevTools tabs missing ([#3728](https://github.com/MicrosoftEdge/WebView2Feedback/issues/3728), [N]). |
| InPrivate profiles | Secondary report (broken) | "Enabled but not working" ([#5081](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5081), [#3694](https://github.com/MicrosoftEdge/WebView2Feedback/issues/3694), [N]). Official docs: not stated. |

**Manifest versions.** Not stated for WebView2. A Microsoft contributor said WebView2 will follow Edge's changes to extension version support (paraphrase; [#3789](https://github.com/MicrosoftEdge/WebView2Feedback/issues/3789), [N], Secondary). Chrome removed all remaining MV2 extensions from the Web Store on 2026-08-31 ([timeline](https://developer.chrome.com/docs/extensions/develop/migrate/mv2-deprecation-timeline), [N]), so store extensions are MV3. Treat MV3 as the target.

**Runtime floor.** Official: SDK 1.0.2210.55 and Runtime 120.0.2210.55 or later [V]. Practical floor for MV3 service-worker stability: 134.0.3124.44, inferred from #5113. Bowser's 155 runtime is well above both.

**Source conflict resolved.** A third-party issue claims the extension APIs first shipped in SDK 1.0.4191.47. Microsoft's pages say 1.0.2210.55 [V]. Microsoft governs.

*Cite (fetched 2026-10-14):* [Profile7](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2profile7), [BrowserExtension](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2browserextension), [browser-features](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/browser-features), [Overview](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/overview-features-apis), [Edge API support](https://learn.microsoft.com/en-us/microsoft-edge/extensions/developer-guide/api-support).

## 3. WebView2 draws no extension UI and gives the host no popup or options API

**Answer.** No host API opens an action popup or options page. The notes found none in the Release 1.0.4258.31 or Prerelease 1.0.4312 references, which contain six `*BrowserExtension*` pages and no toolbar, action, popup, options, store, CRX or extension-permission interface ([reference repo](https://github.com/MicrosoftDocs/webview2-win32-reference/tree/main/1-0-4312-prerelease), [N]). The local bindings agree: seven extension interfaces and nothing else ([audit](extensions-capability-audit.md)). Opening an options page is treated as a feature request, not a bug ([#4608](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4608), [N]).

| UI surface | Status in WebView2 | Evidence |
| --- | --- | --- |
| Action icon and badge | Absent | Verbatim "missing UI entry points" sentence [V]. |
| Action popup window | Absent | Same sentence [V]. The host cannot learn the popup path from the API: only `Id`, `Name`, `IsEnabled` exist [V]. |
| Options page | No host UI or API | Not stated in official docs. Requests for management and options APIs are open or closed without action ([#3856](https://github.com/MicrosoftEdge/WebView2Feedback/issues/3856), [#5084](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5084), [#4627](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4627), [N], Secondary). |
| Permission prompts | Not stated in official docs | The host API has no permission member. Whether an extension's own permission requests reach `PermissionRequested` is untested. |
| Chrome Web Store "Add" button | Not supported by design | Staff, verbatim: "It's by design to not allow extension to be installed throw the store." ([#3694](https://github.com/MicrosoftEdge/WebView2Feedback/issues/3694), 2023-08-09, [V]). A collaborator: "WebView2 does not plan to support installing extensions in WebView2 through Edge/Edge Store. Edge and WebView2 are different entities and an extension being in Edge Store does not mean it is licensed to be used in WebView2. What WebView2 does support is a BYO extension mechanism where developers source their own extensions and are compliant with the extension's license." (2023-08-14, [V]). The store info banner was disabled in runtime 114/115 ([Runtime 115](https://learn.microsoft.com/en-us/microsoft-edge/webview2/release-notes/runtime/115), [N]). Whether the page's button itself is active stayed ambiguous in [#3312](https://github.com/MicrosoftEdge/WebView2Feedback/issues/3312) ([N], Secondary). |
| Extensions management page | Blocked | `edge://extensions` is in the "Blocked internal URLs" list [V]. |
| Developer-mode toggle | No API | Open request, backlog ([#5088](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5088), [N], paraphrase). |
| Extension context-menu items | Reach the host | Named "extension" in `ContextMenuRequested` ([ContextMenuItem](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2contextmenuitem), [N], summary). |

**Attribution caveat.** GitHub reports the association of `dianaqu` as NONE ([#3694](https://github.com/MicrosoftEdge/WebView2Feedback/issues/3694), [V]). The notes found her profile lists Microsoft Edge as her employer and that she authored the extensions spec PR ([N]); that is unverified here. Her comments are engineer statements in an issue thread, not documentation. The "BYO" statement above comes from `nishitha-burman`, whose association is COLLABORATOR [V], and it says the same thing.

**What a host can do instead (undocumented reliance, flagged).** Read `manifest.json` from the folder Bowser registered (`action.default_popup`, `options_ui.page` or `options_page`, `icons`). Then open `chrome-extension://<id>/<page>` in a Bowser-owned WebView2 in the same environment and profile.

- A Microsoft engineer said extension URLs are navigable in WebView2 content and that navigating to an options page worked (paraphrase, [#4390](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4390), [N], Secondary).
- DuckDuckGo's engineers described hosting popups this way ([#3855](https://github.com/MicrosoftEdge/WebView2Feedback/issues/3855), [#3258](https://github.com/MicrosoftEdge/WebView2Feedback/issues/3258), [N], Secondary).
- Known cost: a hosted popup's `chrome.tabs.query` sees the hosting WebView2 as the "active tab" (#3855).
- Not documented: whether such a page has `chrome.*` privileges. UNCERTAIN until tested.

**The extension URL scheme: the sources disagree.**

- `chrome-extension://`: a Microsoft engineer called it the real URL (paraphrase; [#4390](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4390), [N], paraphrase). The task brief uses it.
- `extension://`: the same engineer said both forms work after a Navigation fix, but the reporter could not make `extension://` work and the thread did not resolve ([#4390](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4390), [N]).
- `ms-browser-extension://`: this is what Bowser blocks today. No Microsoft source names it for WebView2. The one hit the notes found was a third-party Adblock Plus code-review comment, probably about legacy EdgeHTML (Secondary).
- Official Microsoft docs: **not stated**.

Bowser's navigation policy blocks `ms-browser-extension` but not `chrome-extension` or `extension`. Those fall into the external-protocol path, which `NavigationStarting` does not cancel ([audit](extensions-capability-audit.md)). Find the real scheme before hosting popups.

*Cite (fetched 2026-10-14):* [Profile7](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2profile7), [#3694](https://github.com/MicrosoftEdge/WebView2Feedback/issues/3694), [browser-features](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/browser-features), [reference repo](https://github.com/MicrosoftDocs/webview2-win32-reference/tree/main/1-0-4312-prerelease).

## 4. Chrome Web Store install mechanics, and why Chromium's policy gates do not apply here

Framing: this is a user's own browser fetching one extension the user asked for. It is not mirroring or bulk fetching.

**CRX3 format and what is signed.** This report re-read the Chromium source on 2026-10-14 ([crx_verifier.cc](https://raw.githubusercontent.com/chromium/chromium/main/components/crx_file/crx_verifier.cc), [crx3.proto](https://raw.githubusercontent.com/chromium/chromium/main/components/crx_file/crx3.proto), [V]).

- A CRX3 is `Cr24`, a little-endian version that must be 3, a little-endian header length N, an N-byte `CrxFileHeader` protobuf, then the ZIP. Any other version is rejected as `ERROR_HEADER_INVALID`.
- Header fields: `sha256_with_rsa` (2), `sha256_with_ecdsa` (3), `verified_contents` (4), `signed_header_data` (10000). `SignedData.crx_id` is 16 bytes.
- Every proof signs `"CRX3 SignedData\x00"`, then a 4-byte little-endian length of `signed_header_data`, then `signed_header_data`, then the raw ZIP bytes. The code feeds the verifier the context, the size of `signed_header_data_str`, the data, then streams the archive.
- The proto comment calls the RSA proofs PSS. The verifier uses `RSA_PKCS1_SHA256` (PKCS#1 v1.5), and ECDSA uses `ECDSA_SHA256`. The code governs what Chrome accepts.
- The verifier rejects a header containing the ZIP tokens `PK\x05\x06` or `PK\x06\x07`, and, behind a killswitch feature scheduled for removal in October 2026, `PK\x06\x06`. It has no header-size cap beyond a sentinel check; the proto tells clients to reject an N "too large". Bowser must pick its own cap.

**Extension ids.** The id is the first 16 bytes of SHA-256 over the DER SubjectPublicKeyInfo, hex-encoded with `0-9a-f` mapped to `a-p`, giving 32 characters ([id_util.cc](https://raw.githubusercontent.com/chromium/chromium/main/components/crx_file/id_util.cc), [N]). For an unpacked folder, Chromium uses the manifest `key` if present and otherwise hashes the folder path ([extension.cc](https://raw.githubusercontent.com/chromium/chromium/main/extensions/common/extension.cc), [N]). WebView2 says its `Id` equals `chrome.runtime.id` and points to MDN for how it is generated [V]. **Whether WebView2 derives it from `key` or from the path: not stated in official docs.** Web Store zips normally have no `key` (unchecked here), so an unpacked load would get a path-derived id, not the store id.

**Update endpoint and parameters.**

- The URL is `https://clients2.google.com/service/update2/crx` ([extension_urls.cc](https://raw.githubusercontent.com/chromium/chromium/main/extensions/common/extension_urls.cc), [N]). Chrome's docs and Microsoft's Edge FAQ list the same URL ([Chrome docs](https://developer.chrome.com/docs/extensions/how-to/distribute/install-extensions), [Edge FAQ](https://learn.microsoft.com/en-us/deployedge/microsoft-edge-manage-extensions-faq), [N]).
- Chromium builds the query from `os`, `arch`, `prod` and `acceptformat=crx3,puff`, plus an `x=` parameter holding URL-encoded `id=<ID>&v=<version>&uc` ([update_query_params.cc](https://raw.githubusercontent.com/chromium/chromium/main/components/update_client/update_query_params.cc), [manifest_fetch_data.cc](https://raw.githubusercontent.com/chromium/chromium/main/extensions/browser/updater/manifest_fetch_data.cc), [N]).
- The direct-download form with `response=redirect` and `prodversion` appears only in community scripts ([gist](https://gist.github.com/slhck/547773a8b1bd104e621e2a36b4960ef7), [HttpWatch](https://blog.httpwatch.com/?p=2397), [N], Secondary).
- Chrome's downloader sends `X-Goog-Update-Interactivity`, `X-Goog-Update-AppId` and `X-Goog-Update-Updater`, and omits credentials ([extension_downloader.cc](https://raw.githubusercontent.com/chromium/chromium/main/extensions/browser/updater/extension_downloader.cc), [N]).
- **Required parameters and headers: not stated in official docs.** A plausible Bowser request is `GET .../update2/crx?response=redirect&os=win&arch=x64&prod=chromiumcrx&prodversion=<runtime version>&acceptformat=crx3&x=id%3D<ID>%26uc`. It is assembled from Chromium code plus community scripts and is UNCERTAIN. Whether to send a Chromium or Chrome `prod` value is an identity choice. The honest choice is not to impersonate Chrome, but the server's tolerance is undocumented.
- **Does Google block non-Chrome clients?** Not stated in official docs, and the notes found no report of blocking on this endpoint. The store page is a different case: Orion and Brave users report a disabled "Add" button, and one reports that spoofing the Chrome user agent did not help (Secondary, search-summary only) ([Orion thread](https://orionfeedback.org/d/6056), [N]).

**Store terms.** The Chrome Web Store Developer Agreement frames the store as distribution to users of Google Chrome and products as made for use with Chrome (summary, not verbatim). Its section 4.4.2 forbids developers from accessing the store by means other than Google's interface, and it binds developers, not end users. No clause forbids or permits a non-Chrome client fetching a package: "not found", not "allowed" ([Developer Agreement](https://developer.chrome.com/docs/webstore/terms/), [N]). A developer's own EULA can restrict use or redistribution, and Microsoft says store listing does not license use in WebView2 [V]. Do not redistribute or bundle store CRXs. Get a legal read before marketing "Chrome Web Store extensions". This is not legal advice.

**Does Chromium need enterprise policy to sideload a store package?** In Chromium, off-store or self-hosted installs on Windows are gated.

- The `ExtensionInstallForcelist` text says off-store apps and extensions can be force-installed on Windows only if the instance is domain-joined, Azure AD joined, or enrolled in Chrome Enterprise Core (summary, not verbatim; [policy yaml](https://raw.githubusercontent.com/chromium/chromium/main/components/policy/resources/templates/policy_definitions/Extensions/ExtensionInstallForcelist.yaml), [N], summary).
- Chrome's docs say that as of Chrome 33 no external installs are allowed from a local CRX path on Windows ([Chrome docs](https://developer.chrome.com/docs/extensions/how-to/distribute/install-extensions), [N]).
- `ExtensionInstallSources` only restores click-to-install for listed URLs, and needs both the CRX location and the referrer to match ([yaml](https://raw.githubusercontent.com/chromium/chromium/main/components/policy/resources/templates/policy_definitions/Extensions/ExtensionInstallSources.yaml), [N]).
- `ExtensionInstallAllowlist` only exempts ids from the blocklist ([yaml](https://raw.githubusercontent.com/chromium/chromium/main/components/policy/resources/templates/policy_definitions/Extensions/ExtensionInstallAllowlist.yaml), [N]).
- Policy, external and test installs use the plain `CRX3` verifier format. Only the Web Store path demands the Google publisher proof ([verifier_formats.cc](https://raw.githubusercontent.com/chromium/chromium/main/extensions/common/verifier_formats.cc), [V]).

**Does WebView2 honor those policies?** **Not stated in official docs.** The WebView2 policy page lists 15 policies under Loader Override, Network and Additional, none with "Extension" in its name, and says it "applies to Microsoft Edge version 87 or later" ([WebView2 policies](https://learn.microsoft.com/en-us/deployedge/microsoft-edge-webview-policies), [V], `ms.date` 2026-10-07). Whether Edge-wide extension policies reach WebView2 is unknown. Do not assume either way. `AddBrowserExtension` is an embedder API for unpacked folders, so (inference) the Chromium CRX install gates are not in its path.

**Edge Add-ons.** Microsoft documents `https://edge.microsoft.com/extensionwebstorebase/v1/crx` only as an admin update URL. Query format, publisher key and WebView2 support are not stated ([Edge FAQ](https://learn.microsoft.com/en-us/deployedge/microsoft-edge-manage-extensions-faq), [N]). It is out of scope here.

**The store page's "Add" button.** It talks to `chrome.webstorePrivate`. Chromium exposes that only to `https://chromewebstore.google.com/*` ([_api_features.json](https://raw.githubusercontent.com/chromium/chromium/main/extensions/common/api/_api_features.json), [N]). Electron's merged fix PR shows it also needs an embedder delegate, and navigating to the store without one crashed the browser process ([Electron PR 53752](https://releases.electronjs.org/pr/53752), [N]). Whether WebView2 exposes it: **not stated in official docs**. Do not depend on it.

*Cite (fetched 2026-10-14):* [crx_verifier.cc](https://raw.githubusercontent.com/chromium/chromium/main/components/crx_file/crx_verifier.cc), [crx3.proto](https://raw.githubusercontent.com/chromium/chromium/main/components/crx_file/crx3.proto), [verifier_formats.cc](https://raw.githubusercontent.com/chromium/chromium/main/extensions/common/verifier_formats.cc), [WebView2 policies](https://learn.microsoft.com/en-us/deployedge/microsoft-edge-webview-policies), [Developer Agreement](https://developer.chrome.com/docs/webstore/terms/).

## 5. Bowser can download, verify, unpack and load a CRX, but WebView2 verifies none of it

**Answer.** Yes, technically. WebView2 never sees the CRX, only the folder, so every check is Bowser's job. Per item 1, the folder must stay stable and unmodified after load.

**Which signature is required?**

- **For WebView2 itself: none.** It only reads the folder.
- **For Chrome's own installer:** every accepted CRX3 needs at least one proof by a key whose id equals the CRX's declared `crx_id` (the developer-key proof); without it the verifier returns `ERROR_REQUIRED_PROOF_MISSING`. The Web Store path additionally needs a Google publisher proof. [V] (source above).
- **The publisher proof is checkable with a pinned hash.** The verifier pins the SHA-256 of the DER SPKI of the `ecdsa_2017_public` key, not the key. The constant, re-read in source on 2026-10-14, is `61f7f2a6bfcf74cd0bc1fe2497cc9b04254c658f79f2145392867ea8366367cf` [V]. A third party takes each ECDSA proof in the header, hashes its `public_key`, compares with the pin, and verifies the signature with that same key. The notes also carry a test-key hash that matters only for Chromium's test format.
- **Two caveats.** Rotation policy is not stated, so keep a versioned allowlist of hashes. Whether a download from `clients2.google.com` by a non-Chrome client always carries the publisher proof is not documented; Chrome's own store path requires it, so it should. Untested.
- **What it buys.** The developer proof alone only proves "someone with this key signed this". The publisher proof shows Google's key signed this exact archive. Neither says the extension is safe.

**Verification order.** Check the requested id (32 characters, `a-p`) before download. Bound N. Reject version != 3. Reject headers containing the ZIP tokens Chromium rejects. Verify every proof over the exact signed bytes. Require a developer proof whose id equals both `crx_id` and the requested id. Check the publisher hash for store provenance. Only then unzip, and only from the byte range after the header.

**Unpacking rules.**

- Extract defensively. Refuse `..`, absolute paths, drive or stream syntax, Windows reserved names, duplicate entries and link entries. Cap bytes actually written, not declared sizes; Chromium's zip reader treats `original_size` as untrusted ([zip_reader.h](https://github.com/chromium/chromium/blob/main/third_party/zlib/google/zip_reader.h), [N]).
- **The underscore rule and `_metadata/`.** WebView2 documents `E_ACCESSDENIED` for "file or directory name starting with "_"" [V]. Chromium's equivalent check enumerates only direct children of the extension root and exempts `_locales`, `_platform_specific` and `__MACOSX`. `_metadata` is not exempt. Its message is nearly identical to WebView2's table row: "Cannot load extension with file or directory name ... Filenames starting with "_" are reserved for use by the system." ([file_util.cc](https://raw.githubusercontent.com/chromium/chromium/main/extensions/common/file_util.cc), [V]). That suggests WebView2 shares the check, but it does not prove it.
- **Chromium's unpacked installer.** For the unpacked location (not for command-line-loaded extensions) it treats an illegal filename as a hard error. First it runs `MaybeCleanupMetadataFolder`, which deletes `_metadata/verified_contents.json`, `_metadata/computed_hashes.json` and the generated indexed-ruleset folder, then removes `_metadata` if empty ([unpacked_installer.cc](https://raw.githubusercontent.com/chromium/chromium/main/extensions/browser/unpacked_installer.cc), [file_util.cc](https://raw.githubusercontent.com/chromium/chromium/main/extensions/common/file_util.cc), [V]). Whether WebView2 runs that cleanup: not stated.
- **Do Web Store zips contain `_metadata/`?** This is UNCERTAIN. The notes show Chrome itself writes `_metadata/verified_contents.json` at install from the CRX3 header's field 4 ([sandboxed_unpacker.cc](https://raw.githubusercontent.com/chromium/chromium/main/extensions/browser/sandboxed_unpacker.cc), [N]), so a plain store zip may not carry it. A repackaged one can.
- **Bowser's rule either way:** never create `_metadata/`, strip it and `__MACOSX` if present in the zip, keep `_locales`, refuse any other root name starting with `_`, and never start the install folder name with `_`. Whether `_locales` is accepted by WebView2 must be tested, because many store extensions use it.

**Extension id stability.** To get the store id from an unpacked load, inject the developer public key (base64 of the DER SPKI from the CRX header) into `manifest.json` as `"key"`, then compare the loaded `Id` with the id computed from that key. This is a design hypothesis. The `key` field's documented purpose is to keep the id stable in development ([manifest key](https://developer.chrome.com/docs/extensions/reference/manifest/key), [N]), and Chromium's `Extension::Create` uses it. Whether WebView2 honors it: not stated. If it does, record that the unpacked tree differs from the publisher's package by that one field.

**What fails if the extension uses an API WebView2 does not implement?** **Not stated in official docs.** No issue shows an undefined namespace, a thrown error, a silent no-op, or a manifest load failure. The only documented load failures are the four HRESULTs above. Reports exist of extensions that load but do not function with no error text (#3728, #3694, #5113) ([N]). So **L1 (loaded) does not imply L3 (works)**. The host has no API to read extension state beyond `Id`, `Name` and `IsEnabled`. Bowser's UI must say "Loaded in WebView2; compatibility not verified". This is not a Web Store compatibility claim.

**Other consequences.** Content verification: Chromium's delegate returns "none" for anything not Web Store-sourced and auto-updateable, so a Bowser-unpacked tree likely runs without it (inference, [N] [chrome_content_verifier_delegate.cc](https://raw.githubusercontent.com/chromium/chromium/main/chrome/browser/extensions/chrome_content_verifier_delegate.cc)). Updates are Bowser's job: WebView2 has no updater.

*Cite (fetched 2026-10-14):* [crx_verifier.cc](https://raw.githubusercontent.com/chromium/chromium/main/components/crx_file/crx_verifier.cc), [file_util.cc](https://raw.githubusercontent.com/chromium/chromium/main/extensions/common/file_util.cc), [unpacked_installer.cc](https://raw.githubusercontent.com/chromium/chromium/main/extensions/browser/unpacked_installer.cc), [Profile7](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2profile7).

## 6. Engine alternatives, scored for Bowser

### (a) Stay on WebView2 and deepen the host

- **Needed:**
  - a CRX pipeline (items 4 and 5);
  - an install-by-id dialog and a "store listing detected" button in Bowser's chrome;
  - a consent dialog in Bowser's HTML dialog surface;
  - a toolbar entry showing a static manifest icon (icons must reach the chrome as checked `data:` PNGs, per the audit);
  - Bowser-hosted popup and options pages (item 3, undocumented reliance).
- **Cannot do:** drive `action.onClicked`, read badge or title changes, host a side panel, or grant per-site access. No host API exists. A third-party test claims there is no way to send `action.onClicked` (Flutter issue #10, author-reported). Extensions that depend on those will not work. These are documented non-goals below.
- **Cost:** no engine rewrite. New code is the pipeline and UI. Footprint stays on the shared Evergreen runtime.

### (b) A compatibility shim for missing chrome.* APIs

- **An in-page shim cannot work.** `AddScriptToExecuteOnDocumentCreated` runs on web document navigations. Whether it touches extension pages or service workers is not stated. Content-script isolated worlds are unreachable from the host: Chrome documents an isolated world as a private execution environment that the page and other extensions cannot access (summary, not verbatim; [content scripts](https://developer.chrome.com/docs/extensions/develop/concepts/content-scripts), [N]).
- **The only route is rewriting a private copy.** Bowser would add shim files and wire them through the manifest, which breaks "genuine" and raises licence questions (Microsoft says developers must comply with each extension's licence, [V]).
- **The service-worker script API is profile-wide.** `AreWebViewScriptApisEnabledForServiceWorkers` gives `self.chrome.webview` to every site's worker in the profile ([CoreWebView2Profile](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/winrt/microsoft_web_webview2_core/corewebview2profile), [N]).
- **Cannot be shimmed because they need the browser process or a privileged context:**
  - Network interception: `webRequest` and `declarativeNetRequest` enforcement. `WebResourceRequested` is per WebView, defaults to file, http, https and registered custom schemes, and needs filters added before the main navigation ([N]).
  - Service-worker lifetime and wakeups: Chrome stops a worker after 30 seconds idle and an incoming event revives it ([lifecycle](https://developer.chrome.com/docs/extensions/develop/concepts/service-workers/lifecycle), [N]). A message posted to a terminated worker is discarded ([ICoreWebView2ServiceWorker](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2serviceworker), [N]).
  - Isolated-world access, `offscreen` documents, and `identity.getAuthToken` (needs a signed-in profile; "Profile and Identity" is off [V]).
  - `chrome.tabs` as the engine reports it: a shim in extension contexts could override it, but the browser process keeps listing every WebView2 as a tab.
- **Partially emulable** with a bridge to host state, but only if shim code can be placed in extension contexts: contextMenus, cookies, downloads, notifications, commands, management, permissions UI.
- **Every shim is a privileged API server.** It must authenticate the caller (for example by worker script URI in `WebMessageReceived`'s `Source`), use typed messages only, and deny private-tab data ([security guidance](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/security), [N]).
- **Verdict:** high fragility on an undocumented injection route, and the hard APIs stay unreachable. Not recommended.

### (c) Other engines

| Engine | Genuine extensions | Window and UI fit | Licence | Size and upkeep | What Bowser rewrites |
| --- | --- | --- | --- | --- | --- |
| CEF | The Alloy extension API was removed in M128. The Chrome extension API works with Chrome-style browsers and windows only ([CEF #3685](https://github.com/chromiumembedded/cef/issues/3685), [N]). A maintainer says extensions need Chrome-style windows that show some portion of the Chrome UI (summary, not verbatim; [forum](https://www.magpcss.org/ceforum/viewtopic.php?f=6&t=20003), [N]). Web Store install inside CEF: not stated. | Alloy-style browsers work in a client HWND, but a Chrome-style window hosts at most one Chrome-style BrowserView ([cef_types_runtime.h](https://github.com/chromiumembedded/cef/blob/master/include/internal/cef_types_runtime.h), [N]). | BSD-style ([LICENSE](https://github.com/chromiumembedded/cef/blob/master/LICENSE.txt), [N]). Prebuilt binaries lack H.264 and AAC; a 2023 issue says enabling them via `proprietary_codecs=true ffmpeg_branding=Chrome` raises patent-licensing questions ([CEF #3559](https://github.com/chromiumembedded/cef/issues/3559), [N]; current builds not checked). | Windows x64 stable 154.0.34: 164.7 MB (minimal) to 348.5 MB (standard), compressed ([index.json](https://cef-builds.spotifycdn.com/index.json), [N]). Engine must ship and update every Chrome cycle; Chrome moved to a two-week cycle from Chrome 153 ([Chrome blog](https://developer.chrome.com/blog/chrome-two-week-release), [N]). | `src/engine/` is 7,183 lines (`webview2.rs` is 5,618) behind a roughly 50-method `EngineAdapter`. Also the chrome-host transport (about 9,000 lines), downloads, permissions, the blocker, per-window process model and updater. `cef-rs` is Apache-2.0 ([cef-rs](https://github.com/tauri-apps/cef-rs), [N]). |
| Electron | Built-in support is a subset aimed primarily at DevTools extensions, and the docs say arbitrary store extensions are not supported (summary, not verbatim; [Electron docs](https://www.electronjs.org/docs/latest/api/extensions), [N]). Third-party `electron-chrome-extensions` and `electron-chrome-web-store` add actions, popups, tabs and store install; service-worker preload support arrived in Electron 35 ([PR #44411](https://github.com/electron/electron/pull/44411), [N]). | Electron owns the window and process model; no documented foreign-HWND embedding ([N]). | Electron is MIT. `electron-chrome-extensions` is GPL-3.0 or a paid licence ([README](https://github.com/samuelmaddock/electron-browser-shell), [N]). | Electron v44.7.2 win32-x64 zip is about 150 MB ([release](https://github.com/electron/electron/releases/tag/v44.7.2), [N]). | Leaves the Rust app shell. |
| WebView2 plus own Chromium fork | Not feasible. The WebView2 licence terms, as summarised, bar reverse engineering, working around technical limits, and redistributing other than the distributable; code must come from Microsoft ([EULA](https://developer.microsoft.com/microsoft-edge/api/eula/webview2), [N]). Plain reading, not legal advice. | - | Same | Fixed Version is unmodified Microsoft binaries, over 250 MB per the page (summary; [distribution](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution), [N]) | No supported route |
| Official Chromium, content layer plus `//extensions` | Igalia reports the Web Store Private API landed upstream in M152 and an MV3 service worker ran in their shell, which is not upstream and is maintained internally. Permissions, cookies and other //chrome-dependent APIs remain ([Igalia](https://blogs.igalia.com/mshin/?p=171), [N]). | Owns its windows, so a native window plus HTML chrome is possible; you write the C++ embedder (inference). | Chromium is BSD-3-Clause plus third-party licences; codec question as CEF ([chromium](https://github.com/chromium/chromium), [N]). | Windows build needs 8 GB RAM (16+ recommended), 100 GB disk, Visual Studio 2026 ([build doc](https://raw.githubusercontent.com/chromium/chromium/main/docs/windows_build_instructions.md), [N]). Rebase every two weeks. | Everything |
| `//chrome` fork (Brave, Vivaldi style) | Full. Brave installs from the store through an add button on the listing ([Brave support](https://support.brave.app/hc/en-us/articles/360017909112), [N], summary). | Brings Chrome's Views UI. Vivaldi says following the then four-week cycle needs at least two full teams, and about 10% of patched files conflict per rebase (summary, not verbatim; [Vivaldi](https://vivaldi.com/?p=441493), [N]). | BSD-3 plus third-party | Full browser footprint (inference) | A different product |

### (d) Helium or ungoogled-chromium as a cost yardstick

This is a cost comparison only. It does not recommend copying their adblock or adopting their UI.

- ungoogled-chromium-windows tagged about 12 builds in eight weeks (155.0.8059.39-1.1 on 2026-10-11 against Chrome 155 stable on 2026-10-06, a lag of about 5 days; about 11 days for 153; the lag is computed by the notes from two data points) ([releases](https://github.com/ungoogled-software/ungoogled-chromium-windows/releases), [N]).
- Helium shipped 12 tagged Windows releases between 2026-09-01 and 2026-10-08 ([helium-windows](https://github.com/imputnet/helium-windows/releases), [N]). Its own code is GPL-3.0, it is crowdfunded, and it states Web Store requests are anonymised via its services ([helium.computer](https://helium.computer/), [N]).
- Even patch-only forks need several releases a week to follow Chromium. Forking either means Bowser's tab model and Rust core are replaced.

### Scoring

Scores are the report author's judgement from the cited facts, 1 to 5, where 5 is best. For maintenance, 5 is cheapest. For rewrite, 5 is smallest. Columns: MV3 = can run genuine MV3 extensions; Store = Web Store install feasibility; Win32+HTML = keeps a native Win32 window and HTML/Tailwind chrome; Tabs = keeps Bowser's tab model; Priv = privacy-feature portability; Light = lightweight footprint; Lic = licensing simplicity; Maint = maintenance; Rewr = rewrite size. The unweighted sum is a crude aid, and the first two columns are the real gate. Several scores for the last four rows rest on inference because no source quantifies footprint or window fit. The MV3 score for rows (a) and (b) is UNCERTAIN until Stage 1 runs.

| Option | MV3 | Store | Win32+HTML | Tabs | Priv | Light | Lic | Maint | Rewr | Sum |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Bowser today (reference, not an option) | 2 | 1 | 5 | 5 | 5 | 4 | 4 | 5 | 5 | 36 |
| (a) WebView2 plus deepened host | 3 | 3 | 5 | 4 | 4 | 4 | 4 | 4 | 4 | **35** |
| (b) WebView2 plus shim | 3 | 3 | 5 | 4 | 4 | 4 | 3 | 2 | 3 | 31 |
| (c) CEF, Chrome-style tab plus Alloy chrome | 4 | 3 | 2 | 2 | 4 | 2 | 3 | 3 | 2 | 25 |
| (c) Electron plus third-party libraries | 3 | 4 | 2 | 3 | 3 | 1 | 3 | 4 | 1 | 24 |
| (c) WebView2 plus own Chromium fork | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 9 |
| (c) Chromium content layer plus //extensions | 3 | 3 | 4 | 4 | 4 | 3 | 3 | 1 | 1 | 26 |
| (c) //chrome fork | 5 | 5 | 1 | 2 | 3 | 1 | 3 | 1 | 1 | 22 |
| (d) ungoogled or Helium as base | 4 | 3 | 1 | 1 | 2 | 1 | 3 | 2 | 1 | 18 |

The reference row sums one point higher than (a) only because doing nothing costs nothing; it fails the product goal on the two gate columns (MV3 2, Store 1).

How to read it: only CEF (4), a `//chrome` fork (5) and ungoogled or Helium (4) score above (a) on running genuine MV3 extensions, and Electron beats (a) only on Store (4 against 3). Leaving out the MV3 and Store columns, every rival trails (a) by 9 to 22 points (CEF by 11, Electron by 12), mostly in rewrite size, footprint and Win32+HTML fit. Their MV3 and Store advantage is itself unproven in a Bowser-shaped setup, because CEF's extension support needs Chrome UI and Electron's needs third-party GPL or paid code.

## 7. Security requirements: what WebView2 enforces and what Bowser must

The notes' honest finding is that WebView2 documents very little. Rows marked "not stated" should be treated as unenforced until a headless or supervised test shows otherwise. Chromium engine properties (extension site isolation since Chrome 92 ([Site Isolation](https://www.chromium.org/Home/chromium-security/site-isolation/), [N]), isolated worlds) are expected to carry over, but Microsoft does not restate them for WebView2.

| Requirement | WebView2 or Chromium already does | Bowser must enforce | Bowser today |
| --- | --- | --- | --- |
| Package validation | WebView2 returns `ERROR_FILE_NOT_FOUND` for a missing or invalid manifest, `E_ACCESSDENIED` for `_` names, `E_FAIL` otherwise [V]. Only an HRESULT comes back; manifest warnings and supported manifest versions are not stated. | Strict pre-flight on its own copy: size and depth caps, key and permission checks, refusal of oddities before load. | Done for `manifest.json` (1 MiB, depth 32, caps). Not for `_locales/*` or rule files ([audit](extensions-capability-audit.md)). |
| CRX integrity and provenance | None: WebView2 never sees a CRX. | Stage 3 verifier: developer proof, id match, optional pinned publisher hash, hash-pin the unpacked tree. | Not applicable yet. |
| Permission prompt before grant | No prompt UI [V]. Chromium's unpacked installer calls `GrantActivePermissions` and does not reference `ExtensionInstallPrompt` ([unpacked_installer.cc](https://raw.githubusercontent.com/chromium/chromium/main/extensions/browser/unpacked_installer.cc), [V]). Whether WebView2 uses that path: not stated. Assume everything declared is granted at load. | Show permissions, host patterns, content-script matches and optional permissions before `AddBrowserExtension`; require explicit consent. Cannot narrow after load. | Native `MessageBoxW` shows them; not enforced. Gaps: Unicode `Cf` characters pass the text filter, and the list can be padded ([audit](extensions-capability-audit.md)). |
| Optional permissions, `permissions.request` | Chrome prompts with a user gesture. WebView2: not stated. | Decide a policy (deny or prompt) and disclose it. | Not handled. |
| Isolated extension origin | Chromium engine behaviour; not restated for WebView2. Storage is per profile [N]. | Never serve Bowser content on the extension origin; never relax CSP; host popups in a separate WebView. | Chrome views run in a separate InPrivate profile with strict CSP. |
| No extra privileges | Microsoft's guidance is generic: scripts, host objects and web messages reach whatever is loaded, so check `Source` and validate ([security](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/security), [N]). Whether they reach extension pages: not stated. | No host objects or web-message handlers reachable from an extension origin. Leave `AreWebViewScriptApisEnabledForServiceWorkers` off. No generic proxy. | Messages accepted only from the host's start URL. |
| Private-tab isolation | Extensions persist per profile [V]. InPrivate handling: not stated; reported broken (#5081). | Private tabs use a separate InPrivate profile with no extensions. Assert `GetBrowserExtensions` is empty there and fail closed. Test whether the default profile's `chrome.tabs.query` can see private or chrome-view WebViews. | `BowserPrivate` profile; extensions added only to a non-private tab's profile. Runtime behaviour untested. |
| Refuse symlinks and junctions | Not stated. Chromium's unpacked load follows symlinks (`FOLLOW_SYMLINKS_ANYWHERE`) ([unpacked_installer.cc](https://raw.githubusercontent.com/chromium/chromium/main/extensions/browser/unpacked_installer.cc), [V]). | Refuse links and junctions on walk, on open (`FILE_FLAG_OPEN_REPARSE_POINT`) and for zip link entries. | Done for folders, tested. |
| Size caps and zip bombs | WebView2 never unpacks. | Cap download bytes, header size, entry count, bytes written and nesting. Revisit the 2,000-file and 128 MiB caps against real extensions (unmeasured). | 2,000 files, 128 MiB, depth 12. |
| Zip-slip and path traversal | Not applicable to WebView2. | Canonical containment, safe entry names, no duplicates, `_` rule. | Entry-name and id checks exist for folders. |
| Load-time swap (TOCTOU) | WebView2 reads in place and removes the extension if content changes later [V]. A swap before first load is not prevented. | Unpack to a fresh private folder, validate that copy, load that copy, never edit it; re-hash at startup. | Copy-then-inspect exists. No re-check after install. |
| Update and revocation | Nothing stated. `Remove` takes effect immediately in running documents [V]. | Own denylist by id and content hash, startup recheck, re-consent when permissions grow. | None. |
| Install source | Staff: store install unsupported; BYO only [V]. | User-initiated single id; no bundling, no mirroring. | Folder picker only. |

## 8. Performance: Microsoft publishes no number for extension cost

**Answer.** **None.** The notes searched Learn, release notes, the spec, samples and WebView2Feedback and found no figure and no qualitative statement about memory, process count or startup cost of enabling extensions ([N]).

What is documented, structurally:

- Microsoft says WebView2 uses Edge's multi-process architecture, so each control launches several browser-engine processes that add memory and startup overhead (summary, not verbatim; [Performance best practices](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance), [N]). The page does not mention extensions.
- The process model page and `COREWEBVIEW2_PROCESS_KIND` (browser, renderer, utility, sandbox helper, GPU, deprecated PPAPI kinds) name no extension process ([process model](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model), [N]).
- An enabled extension "is enabled and running in WebView instances", and a disabled one is "not running" [V].
- The option must match across environments sharing a user data folder, or creation fails with `ERROR_INVALID_STATE` ([N]).

Anything beyond that is inference: real extensions have a background context and content-script injection per matching navigation, so they likely cost something. Whether merely setting the option to TRUE with zero extensions has a cost is not stated. Bowser's existing mitigation is sound: it turns the option on only when the profile has or may hold extensions, and `Enable(FALSE)` stops a dormant one. Measure it: cold start, process count and working set with the option off, on with zero extensions, and with one or N extensions. Bowser's product promise is "very little application overhead" ([PRODUCT.md](../PRODUCT.md)), so this is a gate, not a footnote.

## Where the sources disagree, and how this report resolved it

- **Edge-vs-WebView2 table versus API reference** on extensions: relied on the API reference (item 1).
- **Extension URL scheme** (`chrome-extension://` versus `extension://` versus `ms-browser-extension://`): presented, not resolved (item 3).
- **CRX3 RSA proof type:** `crx3.proto` says PSS and `crx_verifier.cc` uses PKCS#1 v1.5. The code governs (item 4).
- **"Stable in" version:** a third-party issue says SDK 1.0.4191.47; Microsoft says 1.0.2210.55. Microsoft governs. One note also quotes a staff comment citing runtime build 120.0.2210.66 against the release notes' 120.0.2210.55. The release notes govern.
- **Web Store "Add" button in WebView2:** staff say store install is by design unsupported [V]; #3312 shows an inconsistent history on the button. Treated as not usable.
- **Dev-machine runtime:** `docs/extensions.md` says 154.0.4258.62; the audit read 155.0.4283.45 on 2026-10-14. The audit is newer.
- **Earlier notes on release-note retrieval:** one note said the 1.0.2210.55 page returned only a stub. This report fetched it directly and confirmed the entry [V].

## Recommendation: stay on WebView2 and build a Bowser-owned pipeline and UI

**Decision.** Keep WebView2 as the engine. Bowser acquires the CRX, verifies it, unpacks it safely into a private folder, asks consent, loads it with `AddBrowserExtension`, and hosts the extension's popup and options pages itself. No shim, no engine swap. This is the only option in which extension execution is already wired into this codebase: Microsoft documents that the extension "is running right after installation", and Bowser already calls the API. Every alternative replaces the engine layer. That is 7,183 lines of engine plus the chrome-host transport, blocker, permissions, downloads and updater. In exchange for it, CEF still needs a Chrome-style browser with Chrome UI, and Electron needs GPL or paid third-party code and abandons the Rust window. A shim cannot reach extension contexts or the browser process. WebView2 plus a fork is not licensable. The WebView2 path keeps the native Win32 window, HTML chrome, tab model, privacy blocker and shared-runtime footprint. Its limits are real and are listed as non-goals: no host-driven action state, no side panel, no per-site grants, and compatibility depends on which APIs each extension uses. The recommendation stands or falls with Stage 1.

**Blocker (one sentence).** WebView2 offers the host no CRX or Chrome Web Store install entry point, because `AddBrowserExtension` accepts only an unpacked folder, so the store page's own "Add" button cannot install into Bowser.

**Closest feasible path.** User supplies an id or a store URL; Bowser fetches the CRX3 from the update endpoint, verifies it, unpacks, shows consent, and loads it. When the active tab is a store listing, Bowser's own chrome shows "Install" and parses the id in Rust.

**Remaining work for true store-page install:**

- The page's own button stays non-functional: it depends on `chrome.webstorePrivate` and browser detection, and WebView2 support is not stated.
- Update checks (`x=id=...&v=...&uc`) and reinstall on a new version.
- Re-consent when permissions grow, and a revocation denylist.
- Tolerance testing of the endpoint for a non-Chrome client, since parameters and any blocking are undocumented.
- A legal read of the store terms.
- Per-extension compatibility evidence before any "works" wording.

## Staged implementation plan

Verification rules for every stage (project rules):

- Every test launch redirects `LOCALAPPDATA` to an isolated temp directory and passes `--data-dir`. Benchmark flags do not isolate app data, and benchmark launches load no extensions.
- Agents run only unit-level or headless checks.
- Any real-window check is one batched, supervised pass, run with the user present and with the user's OK. Do not drive the GUI on the user's desktop.

**Stage 1. Prove a local MV3 extension executes (no network, no CRX).**

- Change:
  - Add fixtures under `tests/fixtures/extensions/`, plain text only. `probe-mv3`: MV3 manifest, `host_permissions` for `http://127.0.0.1/*` only, a content script, a `background.service_worker`, and the `storage` permission. The content script writes a DOM marker, sends `runtime.sendMessage` to the worker, and reports the worker's reply to a same-origin loopback beacon URL. The worker writes `chrome.storage.local` in `onInstalled` and answers the message.
  - Add three variants: with `_locales/en/messages.json` and `default_locale`; with a manifest `key`; with a root `_metadata/` holding a dummy file.
  - Log every `AddBrowserExtension`, `Enable`, `Remove` and `GetBrowserExtensions` result with HRESULT, WebView2 `Id` and `Name`.
  - Replace the dead `browser_extensions_unavailable` branch with a log of the `Profile7` cast result.
  - Gate on a runtime floor of 134.0.3124.44 (inferred).
- Done when (headless): `cargo test extensions::` passes with every fixture accepted or refused as intended by `inspect_source`; log-format tests pass.
- Done when (supervised batch):
  - `AddBrowserExtension` returns success, with the `Id` format recorded.
  - The loopback server receives the beacon carrying the worker's reply.
  - After a restart the extension is still listed and the beacon repeats.
  - The `_locales`, `key` and `_metadata` variants each load or return their exact HRESULT.
  - The `key` variant's `Id` equals the id computed from that key.
  - A file path ending `.crx` returns its HRESULT, which documents the unknown in item 1.
- Stop rule: if the content script or worker does not run, the recommendation is falsified. Stop and reopen the engine question with that evidence.

**Stage 2. Bowser-hosted popup and options pages for the fixture.**

- Change:
  - Read `action.default_popup` and `options_ui.page` or `options_page` from Bowser's copy and validate them (relative, no `..`, no scheme, file present).
  - Build `<scheme>://<WebView2 Id>/<path>` using the scheme observed in Stage 1.
  - Open it in a Bowser-owned WebView2 in the same environment and profile, with no Bowser message handler (`IsWebMessageEnabled` off, host objects off).
  - Use a one-shot navigation grant for exactly that URL, not a general scheme unblock. Add `chrome-extension` and `extension` to the navigation-policy tests.
- Done when (headless): unit tests for URL building and every rejection case; navigation-policy tests per scheme.
- Done when (supervised): the fixture's popup loads, reads the worker-written `chrome.storage.local`, and a button's `runtime.sendMessage` round trip succeeds. Record what `chrome.tabs.query` returns from the popup.
- Go or no-go: if hosted pages lack `chrome.*`, popups and options pages become a documented non-goal.

**Stage 3. CRX3 parser and verifier (pure Rust, unit-level).** Cap N. Reject version != 3 and headers with the ZIP tokens. Verify RSA PKCS#1 v1.5 SHA-256 and ECDSA P-256 SHA-256 proofs over `"CRX3 SignedData\x00"` plus LE32 length plus signed data plus archive. Derive the id (SHA-256 first 16 bytes, mapped `a-p`). Require a developer proof matching `crx_id` and the requested id. Support a versioned allowlist of publisher key hashes. Done when: unit tests build synthetic CRX3 files in-test (generated keys, no downloads) and pass for the good case and fail for a tampered archive byte, wrong id, header with a ZIP token, version 2, oversize header, no proofs, and a wrong key type.

**Stage 4. Safe unpacker, `_` handling, key injection (unit-level).** Extract from the verified byte range into a new empty private folder named by Bowser id. Apply the unpacking rules in item 5, including the `_` rule, caps on bytes written, and inject `key` if Stage 1 showed WebView2 honors it. Then run existing `inspect_source` and consent. Done when: unit tests with crafted zips cover traversal, absolute paths, duplicates, link entries, reserved names, a zip bomb, a root `_metadata/`, and `_locales` retained; junction tests follow the existing `mklink /J` style.

**Stage 5. Fetch by id.** Parse an id or store URL (32 characters `a-p`). Fetch from the update endpoint over TLS with a size cap, timeout and redirect handling. Send an honest client identity and the real runtime version. Done when (headless): unit tests against a loopback fake server cover redirects, oversize, truncation, a non-CRX body, and a wrong-id CRX. Done when (supervised, user's OK): one user-chosen extension is fetched from the real endpoint. Record the parameters that worked, whether the publisher proof is present, whether the zip carries `_metadata/` or `key`, and file counts and sizes against Bowser's caps.

**Stage 6. UI in Bowser's chrome.** An install-by-id dialog. A store-listing "Install" button that parses the id from the active tab URL in Rust, without injecting anything into the store page. A consent dialog that shows counts, strips or escapes Unicode `Cf` characters, and cannot be padded. A toolbar entry with static manifest icons as checked `data:` PNGs. Honest status wording: "Loaded in WebView2; compatibility not verified". Done when: Rust unit tests for URL-to-id parsing, `npm test` for the logic modules, and a supervised visual pass.

**Stage 7. Lifecycle.** Update checks, re-consent when permissions grow, a denylist by id and hash with startup recheck and immediate `Remove`, startup re-hash of the unpacked tree, and a fix for the audit's multi-process races (`extension_ids.txt` last-writer-wins, unlocked `extensions.txt`). Done when: unit tests; one supervised multi-window pass.

**Documented non-goals** (WebView2 lacks the API, or the report found no evidence it works):

- Driving `chrome.action.onClicked`, reading badge, title or icon changes, or `action.openPopup`.
- Hosting `chrome.sidePanel`.
- Per-site access, runtime permission grants, "allow in private".
- Host-side emulation of `webRequest` or `declarativeNetRequest`, or any `chrome.*` shim, and control of service-worker keepalive.
- `chrome.identity.getAuthToken` and Google sign-in in extensions.
- Extensions in private tabs, which fail closed.
- Native messaging and DevTools panels.
- Making the store page's own "Add" button work.
- Any claim of Web Store compatibility.

## Open questions to test first

Ordered by how much of the plan each one gates. Stage 1's supervised batch answers 1 to 8.

1. Does Runtime 155 load an unpacked MV3 folder through `AddBrowserExtension`? What is the `Id` format? Bowser's parser requires ASCII alphanumeric, at most 128 characters, and drops other mappings.
2. Do content scripts run in default-profile tabs, and stay out of `BowserPrivate`, `BowserChrome` and `bowser-ui:` pages?
3. Does an MV3 service worker start, handle `onInstalled`, wake on a message, and keep `chrome.storage.local` across restart? Does `Remove` wipe the storage?
4. What extension URL scheme appears in `NavigationStarting`, `Source` and `chrome.runtime.getURL`? Does a Bowser-owned WebView2 navigated there get `chrome.*` privileges?
5. Is `_locales/` accepted? Does a root `_metadata/` return `E_ACCESSDENIED`, or is it cleaned up? Is a `.crx` path `ERROR_FILE_NOT_FOUND`?
6. Does a manifest `key` produce the store id? Without it, is the id path-derived and stable for the same folder across restarts?
7. Privacy check: what does `chrome.tabs.query` from a worker or popup return? Can it see private tabs, chrome-view WebViews, or Bowser's popup host? Does `GetBrowserExtensions` on `BowserPrivate` return empty?
8. Which unimplemented or unavailable APIs return what: undefined namespace, throw, or no-op? What do `tabs.create`, `runtime.openOptionsPage` and `permissions.request` do?
9. Is extension network traffic visible to Bowser's blocker (`WebResourceRequested`, CDP `Fetch`)? Are declarativeNetRequest rules actually enforced?
10. Do host objects, `chrome.webview` or injected scripts appear in extension pages or workers?
11. What happens with two Bowser processes on one data folder and different `AreBrowserExtensionsEnabled` values (#4174)?
12. Do MV2 extensions still load on this runtime? Not stated by Microsoft.
13. Does the store endpoint tolerate a non-Chrome client, which parameters are required, does the CRX include the publisher proof, and do real extensions fit Bowser's 2,000-file and 128 MiB caps?
14. Do Edge-wide extension policies reach WebView2? Not stated.
15. Cost: process count, working set and cold-start time with the option off, on with zero extensions, and with extensions.

## Conclusion

The research moved the question from "can WebView2 run extensions" to "can Bowser honestly show what runs". WebView2 executes the extension and leaves the host blind to everything but `Id`, `Name` and `IsEnabled`, so Bowser can report L1 at most from the host. Anything stronger needs per-extension evidence, and the UI has to say so. The store itself is the smaller problem. A CRX3 is signed in a documented way, the id derives from the key, and a verifier is small. The decisive unknowns are runtime behaviours the documentation never states. Chief among them is whether a hosted extension page gets `chrome.*` privileges and how `chrome.tabs` maps to Bowser's tabs, including whether it can see private tabs.

One structural point should shape the consent design. Chromium's own unpacked loader grants every declared permission without an install prompt, and WebView2 has no prompt UI. If WebView2 follows that path, Bowser's pre-load consent dialog is the only gate the user ever gets, and weakening it is not an option. The first supervised pass is cheap and answers most of the list. It should run before any further engine or store work.

## Sources

All fetched or read on 2026-10-14. Tag [V] means this report re-fetched and read the page itself. Tag [N] means the page was read by the research notes through a summarising fetch tool. Chromium raw-file reads under [V] were done with `curl` against `raw.githubusercontent.com`, and `gh api` for issue #3694; nothing was executed or kept on disk. All URLs below were fetched 2026-10-14.

**Microsoft Learn and Microsoft docs**

- [V] https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2profile7
- [V] https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2browserextension
- [V] https://learn.microsoft.com/en-us/microsoft-edge/webview2/release-notes/sdk/1-0-2210-55
- [V] https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/browser-features
- [V] https://learn.microsoft.com/en-us/deployedge/microsoft-edge-webview-policies
- [N] https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2environmentoptions6
- [N] https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2contextmenuitem
- [N] https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2serviceworker
- [N] https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/winrt/microsoft_web_webview2_core/corewebview2profile
- [N] https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/overview-features-apis
- [N] https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance
- [N] https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model
- [N] https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/security
- [N] https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution
- [N] https://learn.microsoft.com/en-us/microsoft-edge/webview2/release-notes/runtime/115
- [N] https://learn.microsoft.com/en-us/microsoft-edge/webview2/release-notes/runtime/123
- [N] https://learn.microsoft.com/en-us/microsoft-edge/webview2/release-notes/runtime/134
- [N] https://learn.microsoft.com/en-us/microsoft-edge/extensions/developer-guide/api-support
- [N] https://learn.microsoft.com/en-us/deployedge/microsoft-edge-manage-extensions-faq
- [N] https://learn.microsoft.com/en-us/answers/questions/1691245/support-of-browser-extensions-with-native-messagin
- [N] https://developer.microsoft.com/microsoft-edge/api/eula/webview2
- [N] https://github.com/MicrosoftDocs/webview2-win32-reference/tree/main/1-0-4312-prerelease
- [N] https://github.com/MicrosoftEdge/WebView2Samples/tree/main/SampleApps/WebView2APISample/assets/extensions/example-devtools-extension

**WebView2Feedback issues** (https://github.com/MicrosoftEdge/WebView2Feedback/issues/N)

- [V] 3694 (Microsoft staff quotes read through `gh api`)
- [N] 3258, 3312, 3728, 3748, 3766, 3789, 3854, 3855, 3856, 4390, 4607, 4608, 4627, 5019, 5081, 5084, 5088, 5113
- Third party, author-reported: [N] https://github.com/Lukas-Bohez/ConvertTheSpireFlutter/issues/10

**Chromium and Chrome**

- [V] https://raw.githubusercontent.com/chromium/chromium/main/components/crx_file/crx_verifier.cc
- [V] https://raw.githubusercontent.com/chromium/chromium/main/components/crx_file/crx3.proto
- [V] https://raw.githubusercontent.com/chromium/chromium/main/extensions/common/verifier_formats.cc
- [V] https://raw.githubusercontent.com/chromium/chromium/main/extensions/common/file_util.cc
- [V] https://raw.githubusercontent.com/chromium/chromium/main/extensions/browser/unpacked_installer.cc
- [N] https://raw.githubusercontent.com/chromium/chromium/main/components/crx_file/id_util.cc
- [N] https://raw.githubusercontent.com/chromium/chromium/main/extensions/common/extension.cc
- [N] https://raw.githubusercontent.com/chromium/chromium/main/extensions/common/extension_urls.cc
- [N] https://raw.githubusercontent.com/chromium/chromium/main/components/update_client/update_query_params.cc
- [N] https://raw.githubusercontent.com/chromium/chromium/main/extensions/browser/updater/manifest_fetch_data.cc
- [N] https://raw.githubusercontent.com/chromium/chromium/main/extensions/browser/updater/extension_downloader.cc
- [N] https://raw.githubusercontent.com/chromium/chromium/main/extensions/common/api/_api_features.json
- [N] https://raw.githubusercontent.com/chromium/chromium/main/extensions/browser/sandboxed_unpacker.cc
- [N] https://raw.githubusercontent.com/chromium/chromium/main/chrome/browser/extensions/chrome_content_verifier_delegate.cc
- [N] https://github.com/chromium/chromium/blob/main/third_party/zlib/google/zip_reader.h
- [N] https://raw.githubusercontent.com/chromium/chromium/main/components/policy/resources/templates/policy_definitions/Extensions/ExtensionInstallForcelist.yaml
- [N] https://raw.githubusercontent.com/chromium/chromium/main/components/policy/resources/templates/policy_definitions/Extensions/ExtensionInstallSources.yaml
- [N] https://raw.githubusercontent.com/chromium/chromium/main/components/policy/resources/templates/policy_definitions/Extensions/ExtensionInstallAllowlist.yaml
- [N] https://raw.githubusercontent.com/chromium/chromium/main/docs/windows_build_instructions.md
- [N] https://github.com/chromium/chromium
- [N] https://www.chromium.org/Home/chromium-security/site-isolation/
- [N] https://developer.chrome.com/docs/extensions/how-to/distribute/install-extensions
- [N] https://developer.chrome.com/docs/extensions/reference/manifest/key
- [N] https://developer.chrome.com/docs/webstore/terms/
- [N] https://developer.chrome.com/docs/extensions/develop/migrate/mv2-deprecation-timeline
- [N] https://developer.chrome.com/docs/extensions/develop/concepts/service-workers/lifecycle
- [N] https://developer.chrome.com/docs/extensions/develop/concepts/content-scripts
- [N] https://developer.chrome.com/docs/extensions/reference/api/webRequest
- [N] https://developer.chrome.com/blog/chrome-two-week-release

**Engines and forks**

- [N] https://github.com/chromiumembedded/cef/issues/3685
- [N] https://github.com/chromiumembedded/cef/issues/3559
- [N] https://www.magpcss.org/ceforum/viewtopic.php?f=6&t=20003
- [N] https://github.com/chromiumembedded/cef/blob/master/include/internal/cef_types_runtime.h
- [N] https://github.com/chromiumembedded/cef/blob/master/LICENSE.txt
- [N] https://cef-builds.spotifycdn.com/index.json
- [N] https://github.com/tauri-apps/cef-rs
- [N] https://www.electronjs.org/docs/latest/api/extensions
- [N] https://github.com/electron/electron/pull/44411
- [N] https://github.com/electron/electron/releases/tag/v44.7.2
- [N] https://releases.electronjs.org/pr/53752
- [N] https://github.com/samuelmaddock/electron-browser-shell
- [N] https://blogs.igalia.com/mshin/?p=171
- [N] https://vivaldi.com/?p=441493
- [N] https://support.brave.app/hc/en-us/articles/360017909112
- [N] https://github.com/ungoogled-software/ungoogled-chromium-windows/releases
- [N] https://github.com/imputnet/helium-windows/releases
- [N] https://helium.computer/

**Community, secondary**

- [N] https://gist.github.com/slhck/547773a8b1bd104e621e2a36b4960ef7
- [N] https://blog.httpwatch.com/?p=2397
- [N] https://orionfeedback.org/d/6056

**Bowser, local (read only)**

- `docs/extensions.md`
- `docs/extensions-capability-audit.md`
- `PRODUCT.md`
