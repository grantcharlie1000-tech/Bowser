# Chrome extension capability audit

Read-only audit of what Bowser can and cannot do with Chrome extensions today. It is for the later
implementer of real extension execution. No product code was changed.

- Checkout: `t3/chrome-extension-support` at `8544fd98` (clean worktree).
- Bindings checked: `webview2-com` / `webview2-com-sys` `=0.39.1` (Cargo.toml pins `webview2-com =
  "=0.39.1"`; Cargo.lock agrees). Source read from the local cargo registry.
- Runtime on this machine: EdgeUpdate `pv` = **155.0.4283.45** (read from the registry on
  2026-10-14). `docs/extensions.md` still says 154.0.4258.62, so the runtime has updated since that
  doc was written.
- Method: source reading only. No Bowser window was started, no tests were run, and no WebView2
  call was made. The security-audit skill was used in guidance mode, and its rule applies: target
  code is not executed without a full sandbox. So every runtime behaviour below is **Unknown**
  unless the code itself settles it.
- Status labels: **Code fact** means the source settles it. **Unknown** means it needs a live
  runtime or Microsoft documentation. None of the gaps below reached the bar for a confirmed
  cross-boundary vulnerability, so none has a severity.

---

## 1. Process and engine architecture

### Who creates the environment

- One Bowser process per window. "Move tab to new window" and "open profile window" start a new
  process. `profile::spawn_window_inner` (`src/profile.rs:855`) passes `--new-window`, and its
  comment says the new window "intentionally shares that profile's session and WebView2 folder".
- In each process, `App::start_engine_creation` (`src/app.rs:5054`) calls
  `WebView2Engine::begin_create_with_extras` (`src/app.rs:5504` →
  `src/engine/webview2.rs:1989`). That creates **one** `ICoreWebView2Environment` on
  `profile.webview2_dir()`.
- Exclusive access is turned off on purpose (`src/engine/webview2.rs:2164`):
  `options2.SetExclusiveUserDataFolderAccess(false)`. The comment explains that a second window of
  the same profile is another process on the same folder. `running.lock` / the session lock still
  refuses a plain second launch.
- The engine checks that the runtime really used Bowser's folder (`UserDataFolder` compared at
  `src/engine/webview2.rs:2080-2105`). If not, it fails with "WebView2 did not use Bowser's isolated
  profile directory".
- There is one other place that creates an environment: web apps and sidebar web panels
  (`src/engine/web_app.rs:152`). They use their own `WebApps` folder and **never** set the
  extension option. `SetAreBrowserExtensionsEnabled` appears only in `src/engine/webview2.rs`.
- When the environment is created: in native mode, lazily, at the first navigation or when the user
  starts typing. In HTML-chrome mode, right after the first paint (`App::request_chrome_environment`,
  `src/app.rs:4985`; see `docs/html-chrome-protocol.md`, "Startup").

### When AreBrowserExtensionsEnabled is set

- `src/app.rs:5486-5501`:
  `browser_extensions: plan.installed || profile_may_hold_extensions`, where
  `profile_may_hold_extensions` means `extension_ids.txt` exists.
- `extension_startup_plan` (`src/app.rs:765`) and `extension_records` (`src/app.rs:783`) return
  nothing for benchmark launches (`benchmark_url.is_some()`) and when
  `profile.webview2_dir() != user_data_dir`.
- `src/engine/webview2.rs:2168-2177` casts the options to `ICoreWebView2EnvironmentOptions6` and calls
  `SetAreBrowserExtensionsEnabled(true)`. If that fails it logs `browser_extensions_unavailable`.
- **Code fact: that failure branch can never run.** `CoreWebView2EnvironmentOptions` is a Rust
  COM object written by webview2-com, not the runtime
  (`webview2-com-0.39.1/src/options.rs:34`, `#[implement(... ICoreWebView2EnvironmentOptions6 ...)]`).
  The setter only stores a bool (`options.rs:566`). So the cast and the call always succeed, even
  on a runtime that ignores the option. Bowser actually finds out whether the runtime supports
  extensions later, in `capture_extension_profile` (`src/engine/webview2.rs:1566-1583`). If the cast
  to `ICoreWebView2Profile7` fails there, it records "This WebView2 runtime does not support browser
  extensions."
- The decision is made once per process. Installing the first extension into a window whose
  environment was created without the option returns "This window was started without extensions.
  Restart Bowser to load it." (`src/engine/webview2.rs:2265`).

### Profile lifetime versus window lifetime

- Bowser profile: the folder `profile.root()`. It holds `Extensions\` (`src/profile.rs:454`),
  `extensions.txt` (`src/profile.rs:459`), `extension_ids.txt` (`src/app.rs:752`) and the
  `WebView2` user-data folder (`src/profile.rs:393`). The extension copies are **outside** the
  WebView2 folder.
- WebView2 profile handle: taken from the **first non-private tab** once its controller is ready
  (`src/engine/webview2.rs:1448-1458`, `.filter(|_| !private)`), through `ICoreWebView2_13::Profile` →
  `ICoreWebView2Profile7`. It is stored in `EngineCore.extension_profile` for the engine's lifetime
  (`src/engine/webview2.rs:1130`).
- Loaded extension objects live in `EngineCore.extensions_loaded` (per process), and the mapping
  lives in `EngineCore.extension_ids`. Both are dropped when the window or engine closes. Whether the
  WebView2 profile keeps extensions after that is **Unknown** (`docs/extensions.md` says so too).

### Private tabs, chrome views, multiple windows

- Private tabs: `create_private_controller` (`src/engine/webview2.rs:4385`) uses
  `SetProfileName("BowserPrivate")` + `SetIsInPrivateModeEnabled(true)` in the **same**
  environment, then `verify_private_profile` (`:4405`) checks the result. Extensions are only ever
  added to the profile taken from a non-private tab. Whether the runtime keeps default-profile
  extensions out of the InPrivate `BowserPrivate` profile is **Unknown**. The API is per profile, so
  that is the expected behaviour.
- HTML chrome views: `CHROME_PROFILE = "BowserChrome"` (`src/chrome_host/host.rs:221`), InPrivate,
  in the same environment (`host.rs:562-563`). `docs/html-chrome-protocol.md` says "The browsing
  profile's extensions are added to its own profile only and never load in the chrome profile." The
  code never adds extensions there. Whether the runtime agrees is **Unknown**.
- Multiple windows mean several processes on one user-data folder, each with its own
  `EngineCore`. **Code facts:**
  - Each process reconciles the profile on its own (`reconcile_profile`,
    `src/engine/webview2.rs:1669`), using whatever `extension_ids.txt` held when it started.
  - `save_extension_ids` (`:1828`) rewrites the whole file from that process's in-memory map, so
    the last writer wins. Window B can overwrite a mapping that window A just wrote. Once a mapping
    is lost, the next startup removes that profile extension and adds it again. The doc already
    warns that this may reset the extension's storage.
  - `extensions.txt` is read, changed and written again with no lock between processes
    (`install_confirmed`, `set_enabled`, `uninstall`). Two windows changing it at once can drop a
    record.
  - In window B, removing an extension that window A installed finds no loaded object and no
    mapping, so `remove_extension` returns `done(Ok(()))` (`:2298`). The copied folder is then
    deleted while the profile may still hold the extension.
  - Two processes creating environments on the same folder with **different**
    `AreBrowserExtensionsEnabled` values (for example, window A from before the first install and
    window B after it): the runtime's behaviour is **Unknown**. It may refuse the second
    environment. Check the Microsoft docs.

---

## 2. Current extension lifecycle

### Install (`src/win32/window.rs:6841` `install_extension`)

1. `pick_extension_folder` (`:6871`): `IFileOpenDialog` with
   `FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM`. Only unpacked folders are accepted. There is no CRX,
   zip or URL path.
2. `extensions::install_confirmed` (`src/extensions.rs:398`): checks the install cap, checks that
   the source is a plain directory, makes a new id, creates `Extensions\<id>`, **copies first**
   (`copy_tree`), then runs `inspect_source` on **the copy**. Only after that does it call
   `confirm`.
3. Confirmation is a native `MessageBoxW` (`crate::win32::ask_yes_no`, `src/win32/mod.rs:86`, owner
   `HWND::default()`). It shows the name, the version and `permission_summary`
   (`src/extensions.rs:675`): manifest version, permissions, host access, content-script matches,
   optional permissions and optional host access. Then: "Only install extensions you trust."
4. If the user says yes, the record is written with `enabled: true`, then
   `App::set_extension_enabled` (`src/app.rs:808`) → `WebView2Engine::set_extension_enabled`
   (`src/engine/webview2.rs:2241`) → `add_extension` (`:1842`) →
   `ICoreWebView2Profile7::AddBrowserExtension(<copy path>)`. If the user says no, the copy is
   deleted.

### Enable, disable, remove

- Toggle (`src/win32/window.rs:6787`): `extensions::set_enabled` writes the record **first**, then
  the engine runs `ICoreWebView2BrowserExtension::Enable(bool)` (`set_loaded_enabled`, `:1883`).
  If the extension is not loaded yet, enabling calls `AddBrowserExtension` instead.
- Remove (`src/win32/window.rs:6804`): asks "Remove the extension …?" with `ask_yes_no`, then
  `App::remove_extension` → `ICoreWebView2BrowserExtension::Remove`. If the extension is not loaded
  in this session but the profile still has it, Bowser lists the profile
  (`remove_unloaded`, `:1758`, `GetBrowserExtensions`) and removes the match by WebView2 id. Only
  after that succeeds does `extensions::uninstall` (`src/extensions.rs:492`) delete the record and
  the folder.

### Startup reconcile

- `capture_extension_profile` (`src/engine/webview2.rs:1558`) → `reconcile_profile` (`:1669`):
  `GetBrowserExtensions`, then `reconcile_action` (`:1600`) decides for each one:
  - no Bowser mapping or no record → `Remove`;
  - recorded as disabled → `Enable(false)`;
  - recorded as enabled with its folder present → `Enable(true)` and reuse it;
  - recorded as enabled with its folder missing → `Remove`.
- Enabled records that are not in the profile are added fresh. If the profile cannot be listed,
  every enabled extension is added fresh (`:1684-1690`).

### Id mapping

- Bowser id: `ext-` followed by 16 lowercase hex digits from a `RandomState` hash of time, pid and
  attempt (`new_id`, `src/extensions.rs:569`). `is_valid_id` (`:561`) is the only path component
  ever joined.
- WebView2 id: `ICoreWebView2BrowserExtension::Id`, read in `extension_webview_id` (`:1654`). It is
  saved as `bowser-id<TAB>webview-id` in `extension_ids.txt`. When parsed, the WebView2 id must be
  ASCII alphanumeric and at most 128 characters (`parse_extension_ids`, `:1799`). The real format of
  the id is **Unknown**. If a runtime returns an id containing `-` or `_`, the line is dropped and
  the mapping is lost.

### Records files and caps

| Item | Value | Where |
|---|---|---|
| Records | `extensions.txt`: `id\tenabled\tname\tversion`, written atomically (temp file + rename) | `src/extensions.rs:590`, `:625` |
| Id map | `extension_ids.txt`, temp file + rename | `src/engine/webview2.rs:1828` |
| Manifest size | 1 MiB | `MAX_MANIFEST_BYTES` `:24` |
| JSON nesting | 32; strict RFC 8259; duplicate keys refused | `:34`, `json` module `:741` |
| Installed per profile | 64 | `MAX_INSTALLED` `:26` |
| Files per extension | 2,000 | `:28` |
| Bytes per extension | 128 MiB | `:30` |
| Folder depth | 12 | `:32` |
| Name | 1-128 chars, no `Cc` controls | `:35`, `is_plain_text` `:255` |
| Permission lists | at most 256 items of at most 256 chars each; non-strings refused | `:36-37`, `string_list` `:227` |
| Links / junctions | refused when walking (`symlink_metadata` + reparse attribute), refused again when a file is opened (`FILE_FLAG_OPEN_REPARSE_POINT`), and each folder is re-checked before it is listed | `walk_with_limits` `:313`, `copy_file_no_follow` `:456` |
| Entry names | no separators, `:`, wildcards, controls, `.`/`..`, trailing dot or space | `is_valid_entry_name` `:369` |
| Delete containment | the folder is canonicalised and must be a direct child of `Extensions` | `remove_folder` `:509` |

### UI that exists today

- Main menu "Extensions…" with the `puzzle` glyph and Ctrl+Shift+E (`src/win32/window.rs:5969`,
  dispatch `:6100`). The same menu is reached from the `"extensions"` command (`:6401`), the command
  palette (`src/win32/command_palette.rs:630`), the shortcut table (`src/shortcuts.rs:188`) and the
  HTML toolbar accelerator map (`src/chrome_host/host.rs:1761`).
- `show_extensions_menu` (`src/win32/window.rs:6686`) shows:
  - one checked or unchecked row per record: `"{name} {version} (enabled|disabled)"`, plus
    `" (failed: {reason})"` from the in-memory `LOAD_RESULTS`;
  - a "Remove extension…" submenu;
  - "Install unpacked extension…";
  - the footer "Unpacked extensions".

  In HTML mode it is drawn through `menu::show` → `html_popup::show_menu`, and falls back to the
  native menu.
- **Code fact:** the HTML popup cuts every label to `MAX_LABEL = 128` UTF-16 units
  (`src/win32/html_popup.rs:46`, `:1199`). A long name plus version can push the `(failed: …)`
  reason out of view.
- `LOAD_RESULTS` lives only in memory and only in this process (`src/extensions.rs:694`). A failure
  in another window is not shown.
- Things that do **not** exist: a management page (`bowser://extensions`), a toolbar or action
  button, action popups, a way to open an options page, any runtime permission prompt, site-access
  controls, or extension icons anywhere. `ui/src` has no extension code. Its only mentions are the
  `puzzle` / `extensions` icon names (`ui/src/icons.gen.ts`, `ui/src/menu-logic.ts`,
  `ui/src/palette-logic.ts`).
- Small findings:
  - The menu item is not disabled for guest profiles (most items use `.disabled_if(guest)`;
    Extensions does not). An extension installed in a guest profile is deleted with the guest
    directory.
  - `__MSG_name__` names (localised manifests) are shown raw. Bowser does not read `_locales`.

---

## 3. What webview2-com 0.39.1 exposes

The binding's `CORE_WEBVIEW_TARGET_PRODUCT_VERSION` is `"145.0.3800.47"`
(`webview2-com-sys-0.39.1/src/bindings.rs:1013`). The highest interface versions in the bindings
are:

- `ICoreWebView2_28`
- `ICoreWebView2Profile8`
- `ICoreWebView2Environment15`
- `ICoreWebView2EnvironmentOptions8`
- `ICoreWebView2Settings9`
- `ICoreWebView2Controller4`
- `ICoreWebView2Frame7`

### Every extension-related interface and method (complete list)

| Interface | Methods | bindings.rs |
|---|---|---|
| `ICoreWebView2EnvironmentOptions6` | `AreBrowserExtensionsEnabled`, `SetAreBrowserExtensionsEnabled` | `:17131` |
| `ICoreWebView2Profile7` | `AddBrowserExtension(extensionfolderpath: PCWSTR, handler)`, `GetBrowserExtensions(handler)` | `:32470` |
| `ICoreWebView2BrowserExtension` | `Id`, `Name`, `Remove(handler)`, `IsEnabled`, `Enable(bool, handler)` | `:4180` |
| `ICoreWebView2BrowserExtensionList` | `Count`, `GetValueAtIndex` | `:4441` |
| `ICoreWebView2ProfileAddBrowserExtensionCompletedHandler` | (completion: `ICoreWebView2BrowserExtension`) | `:32777` |
| `ICoreWebView2ProfileGetBrowserExtensionsCompletedHandler` | (completion: `ICoreWebView2BrowserExtensionList`) | `:32934` |
| `ICoreWebView2BrowserExtensionEnableCompletedHandler` / `...RemoveCompletedHandler` | (completions) | `:4399`, `:4565` |

The Rust wrappers are in `webview2-com-0.39.1/src/callback.rs:558-601`. A search of all 266
interface names in the bindings found **none** whose name contains Action, Popup, ServiceWorker,
Storage, WebRequest, DeclarativeNetRequest, ContentScript, Crx, or any later
`BrowserExtension2`.

### What the bindings can do

| Capability | Bindings | Notes |
|---|---|---|
| Load an unpacked folder | **Yes** | `AddBrowserExtension(folder)` |
| Load a CRX | **No method** | The only parameter is a folder path. Whether the runtime accepts a `.crx` path there is Unknown; the expected answer is no. Bowser could unpack a CRX3 (zip) itself, but it would have no provenance. |
| Open an action popup | **No method** | |
| List action icons, titles or badges | **No method** | The only metadata is `Id`, `Name` and `IsEnabled`. Bowser has to parse its own copy of `manifest.json` for icons, `action` and `options_ui`. |
| Inject content scripts ourselves | **No extension API** | `ICoreWebView2::AddScriptToExecuteOnDocumentCreated`, `ExecuteScript` / `ExecuteScriptWithResult` and CDP through `CallDevToolsProtocolMethod` (which Bowser already uses) can inject JavaScript, but not as an extension: no `chrome.runtime`, `chrome.storage` or extension isolated world. Whether these scripts are isolated from page scripts is Unknown; check the docs. |
| Talk to an extension's service worker | **No method** | There are no service-worker interfaces in 0.39.1. Whether a tab's CDP session can attach to extension worker targets is Unknown, and would not be a supported path. |
| Read `chrome.storage` | **No method** | |
| Subscribe to `webRequest` | **No method** | `WebResourceRequested` (which Bowser uses for its blocker) is per WebView and is not `chrome.webRequest`. |
| Call `declarativeNetRequest` | **No method** | Only extension code can call it, inside the runtime. |
| Per-extension site access or "allow in private" | **No method** | Bowser's only controls are whole-extension enable, disable and remove. |
| Delete a profile | `ICoreWebView2Profile8::Delete` | Not about extensions; listed so nobody mistakes it for one. |

---

## 4. Security gaps: what Bowser enforces and what only the runtime enforces

### Bowser enforces (code facts)

- **Where extensions come from.** Only a folder the user picks. Nothing is downloaded or
  auto-installed, and there is no Web Store path.
- **Path traversal.** Ids are strictly `ext-` + 16 hex, and the id is the only component joined into
  a path (`contained_path`, `src/extensions.rs:553`). Tests: `ids_are_strictly_formatted`, which
  covers `folder_for(&dir, "../../Windows")`. Entry names refuse separators, drive and stream
  syntax, and `..`.
- **Symlinks and junctions.** Refused on the source walk, refused at file open
  (`FILE_FLAG_OPEN_REPARSE_POINT` + `is_plain_file`), refused when a folder is re-listed, refused at
  startup (`startup_plan` skips linked folders), and refused on delete (`remove_folder`
  canonicalises the path). Tests: `inspection_refuses_links_and_junction_like_entries`,
  `install_refuses_links_during_the_copy_and_keeps_nothing`,
  `remove_never_follows_a_link_out_of_extensions`.
- **Manifest bombs.** Bowser's own parser is capped (1 MiB, depth 32, list caps) and strict. This
  covers only `manifest.json`. Bowser does not parse `_locales/*/messages.json`, DNR rule files or
  any other file; the only limits on those are the 2,000-file and 128 MiB caps. The runtime's
  parser handles them.
- **Consent matches what is loaded.** The copy is inspected and approved, and that same copy is
  loaded (`install_confirmed`).
- **Navigation policy.** This applies to every tab WebView2 and so to any navigation the runtime
  drives in a tab:
  - Top-level: `NavigationStarting` (`src/engine/webview2.rs:3370`) uses `navigation_start_policy`
    (`:284`). It allows only http(s) and the one-shot benchmark `file:` grant. It blocks `file`,
    `about`, `blob`, `chrome`, `data`, `devtools`, `edge`, `javascript`, `ms-appx(-web)`,
    `ms-browser-extension`, `res`, `vbscript`, `view-source` and `bowser-ui`. Every other scheme
    goes to `ExternalProtocol`, which is **not cancelled** in `NavigationStarting`; it waits for
    `LaunchingExternalUriScheme` (`:3851`), which cancels the OS launch.
  - Frames: `frame_navigation_scheme_allowed` (`:246`) allows only `http`, `https`, `blob`, `data`
    and `about:blank|srcdoc`. Any extension-scheme iframe is cancelled.
  - New windows (`:3711`): only http(s) can become a tab (`popup_decision`, `:62`).
  - Omnibox / `navigate_tab_internal` (`:1509`): http(s) only.
- **Private tabs.** These are never taken as the extension profile (`.filter(|_| !private)`, which
  `private_tab_guards_stay_wired_into_the_engine` checks), and they are created in the verified
  InPrivate `BowserPrivate` profile.
- **Web permissions.** `PermissionRequested` (`:3630`) denies every kind unless there is a stored
  Allow, and denies everything in private tabs. Whether requests that come from an extension reach
  this handler at all is **Unknown**.
- **Chrome UI.** The chrome views run in a separate InPrivate profile, under a strict CSP with
  Trusted Types, and render labels with `textContent` (`ui/src/menu-view.ts:335`).

### Only the WebView2 runtime enforces (Bowser has no hook)

- **Declared permissions, host permissions and content-script matches.** Bowser shows them and does
  not enforce them (`docs/extensions.md`, "Bowser does not enforce them"). Once an extension is
  loaded, what it can do is whatever the runtime gives it. Bowser adds no sandbox beyond
  enable/disable/remove.
- **Extension network traffic** (service-worker `fetch`, DNR, webRequest). Bowser's blocker hooks
  belong to each tab WebView: CDP `Fetch` / `setBlockedURLs` (`install_fetch_listener`,
  `refresh_blocked_urls`) and `WebResourceRequested` (`:3909`). Nothing in the code observes
  extension background traffic, and whether the runtime sends it through any hook Bowser has is
  **Unknown**. So the privacy stats and blocker do not cover extension traffic.
- **What a loaded extension can reach.** Page content in normal tabs, cookies and storage as its
  APIs allow, possibly `chrome.tabs` control of Bowser's tabs. All of this is Unknown in detail.
  Navigations it makes in a tab still pass through Bowser's `NavigationStarting` and
  `FrameNavigationStarting` handlers, so `file:`, `chrome:`, `edge:` and `javascript:` stay blocked
  there.
- **Private-tab, chrome-profile and `bowser-ui:` isolation from extensions** is enforced by the
  runtime's per-profile model, not by Bowser code. **Unknown** until it is tested.

### Gaps found (code facts; impact needs a runtime where noted)

1. **The extension scheme may not be blocked.** Bowser blocks `ms-browser-extension`. The scheme
   WebView2 actually uses for extension origins is **Unknown**. `chrome-extension:` and
   `extension:` are not on the Block list, so they fall into `ExternalProtocol`, which
   `NavigationStarting` does not cancel. If the runtime handles that scheme itself (so
   `LaunchingExternalUriScheme` never fires), a top-level navigation started by a page or extension
   to an extension page would go ahead in a tab. No test covers this:
   `external_protocols_reach_the_explicit_cancel_handler_only` lists neither `ms-browser-extension`
   nor `chrome-extension`; only `top_host_for_source` tests `ms-browser-extension`. Needs
   validation.
2. **The install prompt can be spoofed with Unicode format characters.** `is_plain_text` refuses
   only `char::is_control` (Unicode `Cc`). Bidi overrides (U+202E), zero-width characters and other
   `Cf` characters are accepted in names and permission strings, then shown verbatim in the
   `MessageBoxW` prompt and the menu. How they render, and whether that can hide `<all_urls>`,
   needs validation on a live dialog. The principal is a third-party extension author; the control
   at stake is the consent prompt.
3. **The install prompt can be padded.** Up to 256 entries of up to 256 characters each, across
   five lists, all go into one `MessageBoxW`. A padded `permissions` list can push "Site access" and
   "Content scripts run on" out of view. How `MessageBoxW` clips this needs validation. The summary
   has no counts and no length limit.
4. **Nothing is re-checked after install.** `startup_plan` (`src/extensions.rs:529`) checks only
   that the folder exists and is not a link. The manifest is not parsed again and there is no hash
   pin. If the copy changes on disk after install (any process running as the same user), the
   prompt the user saw no longer describes what loads. This is the same principal, so it is not a
   boundary violation, but the doc comment "what the user approved is what runs" holds only at
   install time.
5. **A doc comment overstates the copy limit.** `copy_file_no_follow` says "no more than the size
   the walk saw is copied", but it copies the open handle's current length
   (`take(meta.len())` after `input.metadata()`). The caps still hold because the copy is inspected
   again afterwards. Only transient disk use can exceed them.
6. **Windows device names are not refused** (`CON`, `NUL`, `COM1`…) by `is_valid_entry_name`. On
   Windows 10 a copy to such a name may hit the device instead of creating a file. The effect is a
   broken copy, not an escape. Low; Unknown in detail.
7. **Support detection cannot fail** (see section 1): the `browser_extensions_unavailable` event can
   never be raised, and the comment at `src/engine/webview2.rs:2169` is wrong.
8. **The id format check is fragile** (section 2): if the runtime's ids are not ASCII
   alphanumeric, every mapping is dropped on read.
9. **Multi-process races** (section 1): the last writer wins on `extension_ids.txt`, read-change-write
   on `extensions.txt` has no lock, and a remove from another window deletes the folder while the
   profile still holds the extension.

---

## 5. Frontend integration points for a later UI

These are constraints and existing parts to use, not a UI design.

- **Bridge.** `docs/html-chrome-protocol.md` (v1) is the contract.
  - JS → Rust: `chrome.webview.postMessage(string)`. One flat JSON object, at most 64 KiB, `v:1`,
    `t` selects the command, unknown keys rejected. It is parsed by `parse_command` in
    `src/chrome_host/protocol.rs`, and `ui/src/protocol.ts` has the matching limits.
  - Rust → JS: `PostWebMessageAsJson`.
  - Messages are accepted only when the sender's source and the message's own source are exactly
    the host's start URL.
  - Popup commands are throttled to 250 ms.
- **Hosts.** `View` (`src/chrome_host/host.rs:169`) has Toolbar, Tabs, Rail, Panel, Popup, Dialog,
  DialogNotes/Privacy/Rename, Hover, Split, Settings, HistoryPage and DownloadsPage. Assets are
  embedded from `ui/dist` and served from `bowser-ui://app/`.
  - Internal pages are `bowser://` routes drawn in a trusted chrome host, not in a content WebView
    (`src/internal_page.rs`; settings wire in `src/chrome_host/settings.rs`, where `map_command`
    accepts each action only with exactly its own fields).
  - A `bowser://extensions` page would follow the same pattern as settings, history and downloads:
    an `internal_page` host, a new `View`, `?view=`, a new styles file, and an exact-field command
    mapper.
- **Menus.** `menu::show` (`src/win32/menu.rs:593`) → `html_popup::show_menu`
  (`src/win32/html_popup.rs`, labels capped at 128). Rendered by `ui/src/menu-view.ts` and styled in
  `ui/src/styles/menu.css`. The native GDI fallback is `src/win32/menu.rs`.
- **Toolbar.** The `open` actions today are `menu`, `site`, `newtab`, `split`, `zoomreset` and
  `reader`. An extensions or action button needs a new action in `protocol.rs` **and**
  `ui/src/protocol.ts`, plus layout work in `ui/src/view.ts` and `ui/src/styles/toolbar.css`. The
  `pillx`, `pillw` and `menux` placement fields come from Rust.
- **Dialogs and prompts.** Today these are native `MessageBoxW` with no owner
  (`src/win32/mod.rs:73`, `:86`). HTML dialogs exist (`src/win32/html_dialog.rs`, `ui/src/dialog.ts`,
  `ui/src/styles/dialog.css`) and are a candidate for a richer permission prompt.
- **Images.** The chrome's CSP is `img-src 'self' data:`. Extension icons must reach the chrome as
  `data:` PNGs, checked and capped by Rust the same way favicons are (`fav_json`). The chrome must
  never load files from the extension folder by path.
- **Visual language the implementer must match:**
  - `ui/STYLEGUIDE.md`: `--bw-*` tokens only, `tw:`-prefixed Tailwind v4, one surface file per view
    with a `Surface: … Owner:` header, cascade layers, motion and reduced-motion rules, and the
    "Adding a surface" checklist.
  - Components: `.bw-row`, `.bw-toggle`, `.bw-button`, `.bw-dialog`, `.bw-menu*`,
    `.bw-section-header`, `.bw-icon-btn` and `.bw-scroll`.
  - Existing surfaces to model on: `ui/src/settings-page.ts` + `styles/settings.css`,
    `ui/src/panels/*` + `styles/panels.css`, `ui/src/menu-view.ts` + `styles/menu.css`,
    `ui/src/dialog.ts` + `styles/dialog.css`.
  - Icon: `puzzle`, alias `extensions` (`ui/src/icons.ts`). `docs/design/defects.md` D3 notes the
    native menu uses the Grid icon for Extensions.
  - Also read `PRODUCT.md` and `docs/design/reference-spec.md`.
- **Build rules.** `ui/dist` and `ui/dist.manifest` are committed. `build.rs` refuses a `dist/`
  that does not match the manifest. `npm run lint` bans `innerHTML`, `eval` and inline handlers.

---

## 6. Test surface

### Existing tests (none were run in this audit)

- `src/extensions.rs` tests (`:995-1454`):
  - manifest V2/V3 accept and reject cases;
  - malformed JSON and duplicate keys;
  - UTF-8 and size limits;
  - permission-list types and caps;
  - entry names;
  - ids;
  - records round trip;
  - install/uninstall;
  - missing manifest;
  - file, size and depth caps;
  - junction refusal on inspect, copy and remove;
  - `startup_plan`;
  - load results;
  - permission summary;
  - content scripts;
  - declined and confirmed install.

  The junction tests run `cmd /C mklink /J` and write under `%TEMP%`.
- `src/engine/webview2.rs`:
  - `profile_extensions_follow_bowser_records` (the pure `reconcile_action`);
  - `extension_id_file_round_trips_and_drops_bad_lines`;
  - `private_tab_guards_stay_wired_into_the_engine` (a source-text check that includes
    `.filter(|_| !private)`);
  - the navigation and frame scheme tests;
  - `top_host_for_source("ms-browser-extension://x/")`.
- Shortcut and accelerator tests for Ctrl+Shift+E (`src/shortcuts.rs:264`, `:546`) and the profile
  path layout tests (`src/profile.rs:1043`, `:1063`).
- `extensions/bowser-shield`: `node --test extensions/bowser-shield/test/run-tests.js` (437 tests
  according to its `VERIFICATION.md`). All of them run against fakes of `chrome.*`.
- `ui/`: `npm test` has **no** extension tests.

### Commands that already exist

- `cargo test extensions::`
- `cargo test profile_extensions_follow_bowser_records`
- `cargo test extension_id_file`
- `cd ui && npm test`
- `node --test extensions/bowser-shield/test/run-tests.js`

### Not covered

- Any live `AddBrowserExtension`, `Enable`, `Remove` or `GetBrowserExtensions` call. There is no
  WebView2 test double.
- The whole of `reconcile_profile` and `remove_unloaded`.
- The install UI flow, the prompt text rendering and the menu labels (including the 128-unit
  truncation).
- Multi-window and multi-process behaviour.
- Private-tab and chrome-profile exclusion at runtime.
- Navigation policy for `chrome-extension:` and `extension:`.
- Persistence across restarts.
- **Existing live harnesses cannot exercise extensions:** `extension_startup_plan` returns an empty
  plan whenever `benchmark_url` is set. A live check needs a launch with its data isolated (redirect
  `LOCALAPPDATA` and use `--data-dir`) and no benchmark flag. Driving the GUI on the user's desktop
  is not allowed without their OK.

---

## 7. Capability matrix

"Works today" means proven working end to end. Nothing is: no extension has ever been loaded into a
live window.

| Feature | Works today | Partial | Impossible on current WebView2 bindings | Unknown, needs runtime proof |
|---|---|---|---|---|
| Unpacked MV3 load | No (never run live) | Bowser side is coded and unit-tested: copy, check, record, `AddBrowserExtension`, reconcile | — | Whether runtime 155 loads it; error HRESULTs; id format |
| Content scripts | No | Bowser does not block same-world injection; extension iframes **are** cancelled by `FrameNavigationStarting` | Bowser cannot inject *as an extension* (no API) | Whether they run in tabs; not in `BowserPrivate`, `BowserChrome` or `bowser-ui:` |
| Service workers | No | — | No host API to message or inspect them | Start, lifetime, wake-up with no tab |
| Action popup | No | Bowser could read `action` from its own manifest copy | **Yes**: no API to open a popup or read action state or badges | — |
| Options page | No | Bowser knows the page path from the manifest | No API returns the options URL | Extension-page scheme; whether a tab can show it; Bowser blocks `ms-browser-extension` and only the omnibox path allows http(s) |
| chrome.storage | No | — | Host cannot read or write it | Whether it exists and persists; whether `Remove` wipes it |
| Permissions | No (not enforced by Bowser) | Shown at install, all or nothing | No per-permission or per-site grant API | Runtime enforcement; whether `chrome.permissions.request` prompts or reaches `PermissionRequested` |
| declarativeNetRequest | No | bowser-shield probes and falls back to DOM mode | No host API | Static, dynamic and session rules; whether rules are actually enforced |
| webRequest | No | — | No host API (`WebResourceRequested` is a different thing) | Whether it exists for MV3 in WebView2 |
| scripting | No | — | — | `executeScript`, `insertCSS`, worlds |
| tabs | No | — | No event routes `tabs.create` into Bowser's tab model | How Bowser's controller tabs map to `chrome.tabs`; which event `tabs.create` raises |
| contextMenus | No | Bowser's `ContextMenuRequested` only inserts its own item and keeps the rest | — | Whether the runtime adds extension menu items |
| sidePanel | No | — | **Yes**: no host API to host a side panel | — |
| Install from a .crx | No | Bowser could unpack CRX3 itself, but with no signature or provenance check | **Yes**: no CRX method | Whether `AddBrowserExtension` accepts a `.crx` path (expected no) |
| Install from Chrome Web Store URL | No | — | **Yes**: no API, and out of scope. Bowser makes no Web Store compatibility claim | — |

---

## Facts established from code

1. Bindings 0.39.1 (SDK target 145.0.3800.47) expose exactly seven extension interfaces. The only
   operations are: enable the option, add an unpacked folder, list, read `Id` / `Name` /
   `IsEnabled`, enable or disable, and remove.
2. There is one tab environment per window process, on a shared, non-exclusive user-data folder.
   The extension option is decided once at creation, from `extensions.txt` and `extension_ids.txt`.
   It is never set for benchmark launches, web apps or sidebar web panels.
3. The `Options6` capability check cannot fail. Real support detection is the `Profile7` cast at
   the first non-private tab.
4. Extensions are added only to the profile taken from a non-private tab. Private tabs
   (`BowserPrivate`) and chrome views (`BowserChrome`) are separate InPrivate profiles in the same
   environment.
5. Installing is folder-only and copy-then-inspect, with strict manifest parsing, caps, refusal of
   links and junctions, a strict id format, containment on delete and atomic records.
6. Permissions are shown in a native `MessageBoxW` and never enforced by Bowser. Unicode `Cf`
   characters pass the text filter, and the summary has no length limit.
7. Tab navigation blocks `ms-browser-extension` but sends any other unknown scheme (including
   `chrome-extension`) to the external-protocol path. Frames allow only web and document schemes.
   Only http(s) popups open.
8. The UI is one menu (Ctrl+Shift+E): toggle, remove, install. There is no management page,
   toolbar action, popup, options entry or icon. The HTML popup cuts labels to 128 UTF-16 units.
9. Id mapping is per process and last writer wins. `extensions.txt` has no lock between processes.
10. Benchmark launches never load extensions, so existing live harnesses cannot test them.

## Facts that still need a live WebView2 runtime or Microsoft docs

For the parallel research task. Do not treat these as known.

1. Does runtime 155.0.4283.45 honour `AreBrowserExtensionsEnabled` and load an unpacked **MV3**
   folder through `AddBrowserExtension`? Which HRESULTs or messages come back for unsupported
   manifests or APIs?
2. What is the exact format of `ICoreWebView2BrowserExtension::Id`? Is it ASCII alphanumeric
   (what Bowser's parser requires)? Is it stable for the same folder across restarts?
3. What scheme do extension origins use (`chrome-extension://`, `extension://`,
   `ms-browser-extension://`) as seen in `NavigationStarting`, `FrameNavigationStarting`, `Source`
   and `chrome.runtime.getURL`? Does a top-level navigation to it raise
   `LaunchingExternalUriScheme`?
4. Do content scripts run in tabs of the default profile? Do they stay out of the InPrivate
   `BowserPrivate` profile, the `BowserChrome` profile and `bowser-ui://` pages?
5. Do MV3 service workers start (`onInstalled`, `onStartup`) and stay alive or wake normally? Can
   they run with no tab open?
6. Which `chrome.*` APIs are present and working: `storage`, `scripting`, `tabs` (what `tabs.create`
   does in a host app, and which WebView2 event fires), `contextMenus`, `action`,
   `runtime.openOptionsPage`, `permissions.request`, `sidePanel`, `webRequest`, and
   `declarativeNetRequest` (static, dynamic and session rules, and whether they are enforced on
   real requests, not just accepted).
7. Do profile extensions persist across restarts? Does `GetBrowserExtensions` return them before or
   after the first navigation? Does `Remove` delete the extension's storage? Does `Enable(false)`
   take effect at once in open tabs?
8. Does `AddBrowserExtension` read the folder in place or copy it? Does the runtime write into the
   folder? (This decides whether Bowser may delete or modify `Extensions\<id>` while it is loaded.)
9. What happens when two host processes on one user-data folder create environments with different
   `AreBrowserExtensionsEnabled` values?
10. Is extension background traffic visible to any tab's `WebResourceRequested` or CDP `Fetch`
    session (that is, to Bowser's blocker)?
11. Do extension items appear in WebView2's context menu?
12. Are scripts added with `AddScriptToExecuteOnDocumentCreated` / `ExecuteScript` isolated from page
    scripts? (This matters only if someone considers emulating content scripts; recommended
    against.)
13. Check the claim in `extensions/bowser-shield/DESIGN.md` §7.1/§10 that Microsoft documents
    extension UI entry points (icon, badge, popup) as missing in WebView2. Find the primary source.
14. Can extension service workers be inspected with DevTools in WebView2?

## Smallest change set

### If the runtime already supports it: one genuine unpacked MV3 extension end to end

Aim for content script + service worker + `chrome.storage`, with no popup and no options page. The
execution path itself may need **no product change**: install → `AddBrowserExtension` → the runtime
runs it. What is missing is proof and visibility.

1. **Fixture.** Add `tests/fixtures/extensions/probe-mv3/`:
   - an MV3 manifest with `storage` and host access to `http://127.0.0.1/*` only;
   - a content script that writes a DOM marker;
   - a service worker that writes `chrome.storage.local` on `onInstalled` and answers one
     `runtime.onMessage`.

   Plain text files only.
2. **Observability.** In `add_extension`, `set_loaded_enabled`, `remove_loaded` and
   `reconcile_profile` (`src/engine/webview2.rs`), emit policy events such as `extension_added`,
   `extension_add_failed` and `extension_reconciled`. Log the WebView2 id and `Name`. Today
   `add_extension` logs nothing. Fix the dead `browser_extensions_unavailable` branch, or log the
   `Profile7` cast result instead.
3. **A supervised live check.** Launch with isolated data and no benchmark flag. Load a loopback
   http page and confirm the marker. This needs a narrow, debug-only read-back, for example the
   existing `run_reader_script` pattern, which is http(s)-only and size-capped. Then restart and
   confirm reconcile and persistence. Record the runtime version and outcomes in
   `docs/extensions.md` under "Uncertainty, stated plainly".
4. **Cheap hardening found here:**
   - refuse Unicode `Cf` / bidi characters in `is_plain_text`, or escape them for display;
   - put counts on the prompt and limit its length;
   - show a load failure on a row of its own instead of a suffix cut at 128 units;
   - add `chrome-extension` and `extension` to the tests (and to the Block list once item 3 of the
     runtime list is answered).
5. **Only if multi-window use matters:** re-read `extension_ids.txt` and merge before writing, and
   lock `extensions.txt` while reading, changing and writing it, or route extension management
   through one process.
6. **Options page, later and only if the runtime proves the scheme and in-tab rendering:** a
   one-shot, Bowser-started grant for exactly `<scheme>://<webview2-id>/<options path from the
   copied manifest>`, modelled on `consume_allowed_file_navigation`. Never unblock the scheme in
   general.

### Changes that cannot work because the API is missing (0.39.1 / SDK 145.0.3800.47)

- Opening an extension's action popup, or reading or showing its action icon, title or badge from
  the runtime.
- Hosting an extension side panel.
- Reading or writing `chrome.storage`, managing DNR rules, subscribing to webRequest, or messaging a
  service worker from Rust.
- Per-extension, per-site access, runtime permission grants, or "allow in private" through
  WebView2.
- Getting the options URL, version or icons from WebView2. Only `Id`, `Name` and `IsEnabled` exist,
  so Bowser must use its own manifest copy.
- Installing a `.crx` or a Chrome Web Store URL through WebView2. Bowser makes **no** Web Store
  compatibility claim.
- Making Bowser-injected scripts (`AddScriptToExecuteOnDocumentCreated`, CDP) into real extension
  content scripts. They would get no `chrome.*` APIs. Do not use this as a substitute.
