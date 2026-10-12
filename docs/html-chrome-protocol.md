# HTML chrome: architecture and bridge contract (v1)

Status: toolbar, tab strip and rail are implemented. Panel, popup and dialog views have foundation stubs; their content and native hosts follow in later backlog tasks. The GDI chrome remains the default; the HTML chrome is opt-in with `BOWSER_UI=html` and falls back to GDI at runtime if its WebView2 process fails. The chrome shares the tab WebView2 environment (one set of browser processes) in a private chrome profile.

## Architecture (reviewed by Opus)

- Two WebView2 **chrome host** controllers, each a child of a Bowser-owned container window: the toolbar host covers the 28 px toolbar row (back/forward/reload, omnibox, site-info, menu), and the tab strip host covers the tab row when it is horizontal (see Milestone 2). Caption buttons, the resize border and every other native hit-test stay native.
- **Shared environment, private chrome profile.** The chrome views are controllers in the active browsing profile's own tab environment (the engine's `ICoreWebView2Environment`, same user-data folder, same browser/GPU/network/utility processes), so the HTML chrome adds no WebView2 browser process of its own. Each chrome controller is created with `ICoreWebView2Environment10::CreateCoreWebView2ControllerOptions` + `CreateCoreWebView2ControllerWithOptions`, `ControllerOptions.ProfileName = "BowserChrome"` and `IsInPrivateModeEnabled = true`: a separate WebView2 profile, so its cookies, storage, cache and permissions never mix with the browsing profile's, and it writes nothing to disk. After creation the host verifies (`ICoreWebView2_13::Profile`) that the webview really is in that profile and InPrivate, otherwise it fails. The browsing profile's extensions are added to its own profile only and never load in the chrome profile. There is no `ChromeUI` folder any more.
- **Startup.** In native mode the engine environment is created lazily (when the user starts typing or a page is opened). In HTML mode the window asks for it right after the first paint (`App::request_chrome_environment`): it reuses an environment that is already ready or being created, or starts creation with no tab controller and no navigation (a navigation submitted meanwhile waits for, then reuses, the same environment). When creation settles, the window gets `WM_HTML_CHROME_ENV` and requests both chrome controllers (toolbar, tabs) concurrently.
- **Lifetime and failure.** The engine owns the environment. On window close the chrome hosts are closed before the engine closes. Any failure (environment creation, scheme registration, controller creation, profile check) leaves the native chrome. A `ProcessFailed` for a browser-process exit or the chrome's own renderer exiting/hanging falls back to native chrome; a browser-process crash now also affects the tabs, which is expected. Failures of shared processes that WebView2 restarts itself (GPU, utility, sandbox helper, plugin, unknown) leave the chrome in place.
- Guest sessions and `--profile` windows each run in their own process with their own profile's environment, so each window's chrome lives in that window's environment.
- Controller `DefaultBackgroundColor` is alpha 0 (set through ControllerOptions at creation, not an env var). The controller stays hidden until the page posts `ready`, so there is no white flash. Translucency comes from CSS over a transparent body. When the native backdrop is solid, Rust sends `solid:true` plus an opaque `--bg` and CSS paints it.
- Assets are embedded with `include_bytes!` from `ui/dist/` and served by a **custom scheme** `bowser-ui://app/`. In HTML mode only (`BOWSER_UI=html`), the scheme is registered on the tab environment's options at creation (`ICoreWebView2EnvironmentOptions4`, treat-as-secure, authority component, no allowed origins); native mode creates the environment exactly as before. Only the chrome webviews have a `WebResourceRequested` handler (filter `*`) that serves it: every request is answered from the embedded table, otherwise 404. No network, no files on disk. Tabs have no handler for the scheme, so a tab-origin request fails, and the tab policy blocks it outright: `bowser-ui` is in the tab `NavigationStarting` Block list (never the external-protocol path), frame navigation allows only web/document schemes, `navigate_tab` accepts only http(s), and popups only open http(s).
- Rust owns all geometry. JS never sizes the webview.

WM_HTML_CHROME_START, the WM_HTML_*_FAILED and WM_HTML_CHROME_ENV handlers, and WM_HTML_DEFERRED act only when `html_ui_enabled()` is true, so native mode never starts or touches a chrome host.
- Page-controlled text (titles, URLs, completions) is rendered with `textContent` only. No `innerHTML`/`outerHTML`/`insertAdjacentHTML`/`eval`/`new Function`/inline handlers. Trusted Types enforced by CSP.
- The same CSP is also in the page as a `<meta http-equiv="Content-Security-Policy">` (`ui/src/index.html`), so it holds even if header delivery fails. Meta cannot carry `frame-ancestors`, `report-uri` or `sandbox`, so those are left out of both copies. A test checks that the two policies match.
- Startup self-test (`ui/src/main.ts`): before any `ready`, the page assigns a string to an HTML sink on a detached element. With Trusted Types enforced that throws. If it does not throw, the page sets `document.title = 'csp-not-enforced'` and sends nothing, so the host keeps the native chrome.
- `ui/lint.mjs` fails the build on any banned sink in `src/` or in the built `dist/app.js`. The rules live in `ui/lint-rules.mjs`.

## Response headers for every asset

```
Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; require-trusted-types-for 'script'; trusted-types 'none'
X-Content-Type-Options: nosniff
Cache-Control: no-store
```
(`'self'` for a custom scheme: the CSP matches `bowser-ui:`; if the runtime rejects `'self'` use `bowser-ui://app`.)

## JS -> Rust (`chrome.webview.postMessage(string)`, JSON text)

Rules: UTF-8 text <= 16 KiB; one flat JSON object; unknown keys rejected; `v` must be `1`; `t` selects the command. No nesting, no arrays.

| `t` | extra fields | meaning |
|---|---|---|
| `ready` | - | page loaded; show controller and send first `state` |
| `nav` | `a`: `back`/`forward`/`reload`/`stop` | navigation buttons |
| `edit` | `seq` int >= 0, `text` string <= 2048 chars | omnibox text changed by user |
| `submit` | `text` string <= 2048 | Enter in omnibox; Rust normalises (never raw navigate) |
| `escape` | - | Esc in omnibox: Rust restores text and returns focus to tab |
| `focus` | `on` bool | omnibox gained/lost focus |
| `open` | `a`: `menu`/`site`/`newtab`/`split` | open native menu / site-info / new tab / toggle split view (same action as Ctrl+Shift+\) |
| `drag` | - | left mousedown on an empty toolbar area (drag surface only, never a button, input or pill); Rust begins a native window move |
| `caption` | `a`: `toggleMaximize`/`systemMenu` | double-click (`toggleMaximize`) or right-click (`systemMenu`) on an empty toolbar area |

Any message with more than six keys is rejected as soon as the seventh key starts (`v` and `t` count). Rate and popup limits are listed under "Limits" below.

## Rust -> JS (`PostWebMessageAsJson`, object)

| `t` | fields |
|---|---|
| `state` | `scale` number (device pixel ratio hint), `solid` bool, `dark` bool, `back` bool, `forward` bool, `loading` bool, `progress` int 0-100, `security` one of `secure`/`notsecure`/`certproblem`/`internal`/`unknown`/`none`, `label` string (security summary, short), `theme` object of CSS colour strings keyed by token: `bg`, `fill`, `border`, `focusBorder`, `text`, `placeholder`, `completion`, `selection`, `hover`, `progress` |
| `setText` | `text` string, `select` bool (Rust-set omnibox text, e.g. on tab switch or Ctrl+L). Sent when the active tab changes even while the omnibox is focused (a blank new tab sends `""` with `select: true`); within the same tab, only while the omnibox is unfocused, so typing is never overwritten |
| `focusOmnibox` | `select` bool |
| `completion` | `seq` int, `suffix` string (inline completion for edit `seq`; JS ignores stale seq or if value no longer matches) |

`theme` is the only nested value and is Rust->JS only (Rust is trusted).

## Display rules enforced in JS

- Rust formats display URLs; JS never parses URLs.
- `dir="auto"`, `unicode-bidi: isolate` on text from pages.
- Favicons are used by the tab strip: a `fav` message carries the tab's PNG, sent when that tab's favicon changes (see Milestone 2 and Limits).

## Rust module API contract (`src/chrome_host/`)

- `protocol.rs`: `pub enum Command { Ready, Nav(NavAction), Edit{seq,text}, Submit(String), Escape, Focus(bool), Open(OpenTarget), BeginDrag, Caption(CaptionAction), Tab{action,id}, TabMove{id,before}, Group{action,key} }`, `pub fn parse_command(raw:&str)->Result<Command,ProtocolError>` (strict, size-capped, field-capped, pure), `pub struct ViewState{...}` with `pub fn to_json(&self)->String`, `pub fn tabs_json(active:u32, items:&[TabItem])->String`, `pub fn fav_json(id:u32, png:&[u8])->Option<String>`, `pub fn set_text_json(text:&str,select:bool)->String`, `pub fn focus_json(select:bool)->String`, `pub fn completion_json(seq:u32,suffix:&str)->String`, `pub fn is_popup_command(&Command)->bool`, and the rate helpers `TokenBucket` and `Throttle` (time is passed in). No external JSON dependency.
- `assets.rs`: `pub const SCHEME:&str="bowser-ui"; pub const START_URL:&str="bowser-ui://app/index.html?view=toolbar"; pub const TABS_START_URL:&str="bowser-ui://app/index.html?view=tabs"; pub fn lookup(path:&str)->Option<Asset>` where `Asset{bytes:&'static [u8], mime:&'static str}`; `pub const RESPONSE_HEADERS:&str` (CRLF-joined for `CreateWebResourceResponse`).
- `host.rs`: `pub enum View { Toolbar, Tabs }` with `start_url()`; `CHROME_PROFILE`, `valid_profile_name(&str)`, `register_ui_scheme(&options)` (called by the engine when creating the tab environment in HTML mode), `pub struct ChromeEnv` (`ChromeEnv::new(ICoreWebView2Environment)`, a clone-able wrapper over the shared tab environment, from `WebView2Engine::chrome_environment()`), `pub type AccelHandler = Rc<dyn Fn(Accel) -> bool>` with `pub fn accel_command(Accel, omnibox_editing) -> Option<Command>` (`on_accelerator` sees keys the page did not consume), `ChromeHost::create(env:&ChromeEnv, parent:HWND, bounds:RECT, view:View, on_command:Rc<dyn Fn(Command)>, on_failed:Rc<dyn Fn()>, on_accelerator:Option<AccelHandler>, on_ready:impl FnOnce(Result<ChromeHost>))`, `set_bounds`, `post_json(&str)`, `focus()`, `set_visible(bool)`, `close()`.
- `mod.rs`: the pure glue: `security_of`, `css_colour`, `theme_tokens`, `toolbar_bounds`, `tab_row_bounds`, `tab_items`, `tab_to_wire`/`tab_from_wire`, `drop_index`, `group_wire_key`, `favicon_sendable`, `state_changed`, `should_set_text`, `html_ui_enabled`.
- Chrome webview lock-down (unchanged): only the exact start URL may navigate; subframe navigations are cancelled; new windows swallowed; downloads cancelled; every permission denied; devtools only in debug builds; external drop off; host objects off; context menus, zoom, autofill, password autosave, swipe and browser accelerator keys off; web messages accepted only when both the sender's source and the message's own source are exactly the host's start URL.

## Limits

Enforced by Rust (`protocol.rs`, `host.rs`, `window.rs`), with the matching limits in `ui/src/protocol.ts`:

| Limit | Value |
|---|---|
| Raw message from the page | <= 16 KiB (`MAX_RAW_BYTES`) |
| Keys in one JS -> Rust object | <= 6 (`MAX_FIELDS`); the seventh key is rejected before the duplicate check |
| Text field (`edit`, `submit`) | <= 2048 characters |
| Page message rate, per host | token bucket: 120 per second, burst 60 (`MESSAGE_RATE_PER_SEC`, `MESSAGE_BURST`); excess messages are dropped unparsed |
| Menu, popup and window commands, per host | at most one per 250 ms (`POPUP_COMMAND_INTERVAL`): `drag`, `caption`, `open` with `menu`/`site`/`split`, `tab` with `menu`, `group` with `menu`; extra ones are dropped |
| Deferred command queue, per host | 8 (`HTML_DEFERRED_LIMIT`); a full queue drops the command |
| Tabs per `tabs` message | <= 512 (`MAX_TABS`). Rust keeps the first 512 in order; when the active tab lies beyond them it replaces the last one, so the active tab is always sent. The page rejects a larger message |
| Tab title / group name | 200 / 64 characters |
| Favicon (`fav`) | PNG <= 16 KiB (`MAX_FAVICON_BYTES`), IHDR present first, width and height each 1 to 256 (`MAX_FAVICON_DIMENSION`) |
| Theme colour | only the forms in `is_safe_css_colour`: `#` with 3, 4, 6 or 8 hex digits, or `rgb(`/`rgba(` over digits, commas, dots, spaces and `%`, at most 64 characters. Anything else (`url(`, other functions, `;`, `}`) is dropped |

Popup and window commands run from the UI thread, never inside WebView2's message callback. Each one is queued and posted as `WM_HTML_DEFERRED`, and each post runs the oldest queued command, so a modal menu or move loop never runs inside the callback. `drag` and `caption` count only when the cursor is inside the sending host's own region (toolbar region or tab row region). `drag` also needs the primary button held: the right button when the system buttons are swapped.

## Milestone 2: tab strip view (second controller)

The tab row is a second chrome host (same shared environment and private chrome profile, own controller) showing the same bundle with a view selector. Start URLs (exact match required, one per host): `bowser-ui://app/index.html?view=toolbar` and `bowser-ui://app/index.html?view=tabs`. Asset lookup ignores the query; JS reads `location.search`. Bridge validation checks both the sender's Source and the message's own Source (`args.Source()`) against the host's own start URL. A message that fails either check is dropped.

The Tabs host is active in either layout after its page has said `ready`. Horizontal mode covers the full tab row. Vertical mode covers the sidebar up to (but not including) the native resize handle; native sidebar paint and hits are skipped only inside that region. If the page fails or the region is too narrow, native painting and hits resume.

Wire ids: a tab's wire id is `TabId + 1`, so `0` is never a tab (`tabmove.before = 0` means the end of the strip). Group keys are a stable non-zero FNV-1a hash of the group's name (domain or opener).

### JS -> Rust additions (flat objects, ids are unsigned ints that must exist or are ignored)

| `t` | fields | meaning |
|---|---|---|
| `tab` | `a`: `select`/`close`/`menu`/`pin`/`mute`, `id` | tab actions (`menu` = Rust shows the native tab context menu at the cursor) |
| `tabmove` | `id`, `before` (tab id, `0` = end of strip) | drag-reorder drop; Rust uses the same move/drop logic (including group join/leave rules) as native |
| `group` | `a`: `toggle`/`menu`, `key` | group ball click (collapse/expand) or group menu |
| `side` | `a`: `workspace`/`newtab` | open the native workspace switcher or create a tab in the current workspace |

### Rust -> JS additions

| `t` | fields |
|---|---|
| `tabs` | `active` id, `items` array of `{id, title, loading, pinned, muted, marker (int, -1 none), g (group key int, 0 none), gl (bool: group leader), gname (leader only), gcol (leader only, int colour id, -1 accent), gcollapsed (leader only), gsize (leader only), fav (bool: a favicon is cached for this id)}`. Titles are length-capped by Rust. |
| `fav` | `id`, `uri` (`data:image/png;base64,...`, validated PNG, <= 16 KiB) sent once per favicon change |
| `layout` | `mode`: `vertical`/`horizontal` | tells the Tabs view which renderer to show |
| `workspace` | `name`, `icon` (`g:` plus a known workspace glyph) | current workspace label and glyph for the vertical header |

`tabs`, `fav`, `layout`, and `workspace` are Rust->JS only, so nested arrays are fine. The toolbar view ignores them and the Tabs view ignores `state` except `theme`/`solid`/`dark`/`scale`.

The Limits section above applies to these messages too: at most 512 items, titles and group names capped, favicons validated.

## Milestone 3: left rail view (third controller)

Third chrome host, `View::Rail`, start URL `bowser-ui://app/index.html?view=rail`, bounds = `layout.rail` (full-height pill at the left edge, rounded 20px, same translucent look as native). Active only when the page sent `ready`; while active native rail painting and hit-testing are skipped. The rail's panels (customize, bookmarks, history, web apps) stay native for now and open to the right of the rail exactly as before.

Rail keys are strings: `home`, `customize`, `ws:<id>` (workspace id, decimal), `addws`, `app:<id>` (decimal), `addapp`, `bookmarks`, `history`, `menu`. Sections top to bottom: top (`home`, `customize`), workspaces (`ws:*`, `addws`), apps (`app:*`, `addapp`), bottom (`bookmarks`, `history`, `menu`). Workspaces and apps scroll (vertical, no visible scrollbar) when they overflow; top and bottom stay fixed. Section separators are hairlines.

### JS -> Rust

| `t` | fields | meaning |
|---|---|---|
| `rail` | `a`: `click`/`menu`, `k` (key string <= 24 chars matching the key grammar) | left click / right click (Rust shows the native context menu at the cursor) on a rail button |
| `railmove` | `k` (a `ws:*` or `app:*` key), `before` (key of the item it is dropped before, `""` = end of that section) | drag-reorder within a section (Rust uses the same workspace_move / app reorder logic as native) |

Rust validates the key against the current rail model; unknown keys are ignored.

### Rust -> JS

| `t` | fields |
|---|---|
| `rail` | `items`: array of `{k, icon, label, active, dot, accent, fav}`; `icon` is one of `home`, `customize`, `plus`, `heart`, `clock`, `dots`, `apptile`, `letter`, or `g:` + one of `briefcase`, `home`, `code`, `book`, `cart`, `game`, `music`, `heart`, `star`, `bolt`, `globe`, `leaf`; `label` string (also the tooltip and the letter-tile source; capped 64 chars); `active` bool (current workspace or open panel); `dot` bool; `accent` CSS colour string from Rust's safe-colour set or `""` for the theme accent; `fav` bool (a favicon is cached for `k`) |
| `railfav` | `k`, `uri` (`data:image/png;base64,...`, same validation as `fav`) |

`state` still supplies theme/solid/dark/scale. Tooltips use the element `title` attribute (WebView2 shows it as a native tooltip that may overflow the 40px-wide view).

## Foundation extension: panel, popup and dialog views

The same embedded `index.html` serves three more exact start URLs:
`bowser-ui://app/index.html?view=panel`, `bowser-ui://app/index.html?view=popup`, and
`bowser-ui://app/index.html?view=dialog`. Each host accepts a message only when both WebView2 source
URLs equal its own start URL exactly. The stubs apply `state` theme colours, send `ready`, and leave
an empty container for later tasks.

Panel and dialog commands remain flat JS -> Rust objects with `v:1`. The families are `panel`,
`hist`, `bm`, `cust`, `notes`, `set`, and `priv`, all shaped as
`{v:1,t:"hist",a:"open",id?:number,parent?:number,before?:number,n?:number,on?:boolean,text?:string,url?:string}`.
`a` is a required 1–24 character lower-case ASCII token. `id`, `parent` and `before` are unsigned
integers at most 2^53−1; `n` is signed i32; `text` and `url` are at most 2048 Unicode characters.
The allowed field names are exactly `a,id,parent,before,n,on,text,url`. Any four of those names may
appear, including `a`: `MAX_FIELDS` remains six keys total, counting `v` and `t`; the seventh key
is rejected before duplicate-key inspection. The panel host accepts `panel/hist/bm/cust`; the
dialog host accepts `panel/notes/set/priv`.

The popup host alone accepts `{v:1,t:"size",tok,w,h}` (`tok` unsigned u32; `w` and `h` whole CSS
pixels from 1 to 4000), `{v:1,t:"pick",tok,id}` (`tok` and `id` unsigned u32), and
`{v:1,t:"dismiss",tok}` (`tok` unsigned u32). Toolbar, tab-strip and rail hosts reject all panel,
dialog and popup command families before dispatch. `ready` is accepted by every host.

Rust -> JS additions are `{v:1,t:"panelshow",p:"history"}` (a 1–24 character lower-case ASCII panel
token), `{v:1,t:"layout",mode:"vertical"|"horizontal"}`, and snapshots shaped as
`{v:1,t:"snap",p:"history",d:{...}}`. The UI accepts panel tokens `history`, `bookmarks`,
`customize`, `notes`, `settings` and `privacy`; a snapshot's `d` is an object whose nested JSON
values have at most eight levels, 128 keys per object, 4096 keys overall and 4096 items per array.
The complete inbound message is at most 256 KiB encoded as UTF-8. Unknown top-level keys or
malformed values are rejected. The Rust `json.rs` writer closes containers structurally and escapes
`<`, control characters and Unicode line separators.

`ChromeHost::try_suspend()` starts WebView2 suspension and returns initiation errors; asynchronous
failure invokes the host failure callback. `resume()` returns WebView2 errors. Both reject closed
or re-entered host state without panicking.
