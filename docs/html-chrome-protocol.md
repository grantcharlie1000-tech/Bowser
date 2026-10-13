# HTML chrome: architecture and bridge contract (v1)

Status: toolbar, tab strip and rail are implemented. Panel, popup and dialog views have foundation stubs; their content and native hosts follow in later backlog tasks. Plain `cargo build` keeps the GDI chrome as the default and the HTML chrome is opt-in with `BOWSER_UI=html`; builds made with `--features html-chrome-default` (what `Update-Bowser.ps1` builds, so the installed exe and its shortcuts open the HTML chrome) default to HTML, and `BOWSER_UI=native` selects the GDI chrome in any build. The HTML chrome falls back to GDI at runtime if its WebView2 process fails. The chrome shares the tab WebView2 environment (one set of browser processes) in a private chrome profile.

## Architecture (reviewed by Opus)

- Two WebView2 **chrome host** controllers, each a child of a Bowser-owned container window: the toolbar host covers the 34 px toolbar row (back/forward/reload, omnibox, site-info, menu) below a 6 logical px top inset, and the tab strip host covers the tab row when it is horizontal (see Milestone 2). The inset stays native and draggable; caption buttons, the resize border and every other native hit-test stay native.
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
| `open` | `a`: `menu`/`site`/`newtab`/`split`/`zoomreset`/`reader` | open native menu / site-info / new tab / toggle split view (same action as Ctrl+Shift+\) / reset the active page's zoom to 100% (the toolbar zoom chip; same as Ctrl+0; not a popup command, so it is not throttled) / toggle reader view (same action as Ctrl+Alt+R; counts toward the 250 ms popup-command limit) |
| `drag` | - | left mousedown on an empty toolbar area (drag surface only, never a button, input or pill); Rust begins a native window move |
| `caption` | `a`: `toggleMaximize`/`systemMenu` | double-click (`toggleMaximize`) or right-click (`systemMenu`) on an empty toolbar area |

Any message with more than six keys is rejected as soon as the seventh key starts (`v` and `t` count). Rate and popup limits are listed under "Limits" below.

## Rust -> JS (`PostWebMessageAsJson`, object)

| `t` | fields |
|---|---|
| `state` | `scale` number (device pixel ratio hint), `pillx`, `pillw` and `menux` non-negative integers in CSS px relative to the toolbar host's left edge, `solid` bool, `dark` bool, `back` bool, `forward` bool, `loading` bool, `progress` int 0-100, `security` one of `secure`/`notsecure`/`certproblem`/`internal`/`unknown`/`none`, `label` string (security summary, short), `permblocked` bool, `zoom` integer 25 to 500 (the active tab's page zoom in percent; optional, so older hosts omit it; the toolbar shows a chip with a reset action while it is not 100), `private` bool (optional: the active tab is a private tab; the pill shows the private indicator), optional `reader` one of `none`/`available`/`on` (the reader view button: hidden, offered because the page looks like an article, or pressed; older states omit it, which means hidden), `theme` object of CSS colour strings keyed by token: `bg`, `fill`, `border`, `focusBorder`, `text`, `placeholder`, `completion`, `selection`, `hover`, `progress`. Older states without the three placement fields use flex centring. |
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
| `tabs` | `active` id, `items` array of `{id, title, loading, pinned, muted, private (bool: a private tab, never grouped, so `g` is `0`), marker (int, -1 none), g (group key int, 0 none), gl (bool: group leader), gname (leader only), gcol (leader only, int colour id, -1 accent), gcollapsed (leader only), gsize (leader only), fav (bool: a favicon is cached for this id)}`. Titles are length-capped by Rust. |
| `fav` | `id`, `uri` (`data:image/png;base64,...`, validated PNG, <= 16 KiB) sent once per favicon change |
| `layout` | `mode`: `vertical`/`horizontal` | tells the Tabs view which renderer to show |
| `workspace` | `name`, `icon` (`g:` plus a known workspace glyph) | current workspace label and glyph for the vertical header |

`tabs`, `fav`, `layout`, and `workspace` are Rust->JS only, so nested arrays are fine. The toolbar view ignores them and the Tabs view ignores `state` except `theme`/`solid`/`dark`/`scale`.

The Limits section above applies to these messages too: at most 512 items, titles and group names capped, favicons validated.

## Milestone 3: left rail view (third controller)

Third chrome host, `View::Rail`, start URL `bowser-ui://app/index.html?view=rail`, bounds = `layout.rail` (full-height pill at the left edge, rounded 20px, same translucent look as native). Active only when the page sent `ready`; while active native rail painting and hit-testing are skipped. History, Bookmarks and Customize open as rail flyouts (popovers beside their button; see "Rail flyouts" below and `docs/design/flyouts.md`); web app panels stay docked to the right of the rail.

Rail keys are strings: `home`, `customize`, `ws:<id>` (workspace id, decimal), `addws`, `app:<id>` (decimal), `addapp`, `bookmarks`, `history`, `menu`. Sections top to bottom: top (`home`, `customize`), workspaces (`ws:*`, `addws`), apps (`app:*`, `addapp`), bottom (`bookmarks`, `history`, `menu`). Workspaces and apps scroll (vertical, no visible scrollbar) when they overflow; top and bottom stay fixed. Section separators are hairlines.

### JS -> Rust

| `t` | fields | meaning |
|---|---|---|
| `rail` | `a`: `click`/`menu`/`hover`, `k` (key string <= 24 chars matching the key grammar), `r` (optional for `click`/`menu`, required for `hover`: the button rectangle in rail CSS px, hover-layer grammar) | left click / right click (Rust shows the native context menu at the cursor) on a rail button; `hover`: the pointer rested 120 ms on a flyout button (`customize`, `bookmarks`, `history`, `menu`) while another flyout is open, so Rust switches the flyout |
| `railmove` | `k` (a `ws:*` or `app:*` key), `before` (key of the item it is dropped before, `""` = end of that section) | drag-reorder within a section (Rust uses the same workspace_move / app reorder logic as native) |

Rust validates the key against the current rail model; unknown keys are ignored.

### Rust -> JS

| `t` | fields |
|---|---|
| `rail` | `items`: array of `{k, icon, label, active, dot, accent, fav}`; `icon` is one of `home`, `customize`, `plus`, `heart`, `clock`, `dots`, `apptile`, `letter`, or `g:` + one of `briefcase`, `home`, `code`, `book`, `cart`, `game`, `music`, `heart`, `star`, `bolt`, `globe`, `leaf`; `label` string (also the tooltip and the letter-tile source; capped 64 chars); `active` bool (current workspace or open panel); `dot` bool; `accent` CSS colour string from Rust's safe-colour set or `""` for the theme accent; `fav` bool (a favicon is cached for `k`) |
| `railfav` | `k`, `uri` (`data:image/png;base64,...`, same validation as `fav`) |

`state` still supplies theme/solid/dark/scale. Tooltips use the element's `data-tip` attribute and are drawn by the hover layer (below), so they escape the 40px-wide view.

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

## Hover layer (tooltips, tab hover card, island menu)

One lazily created, owned `WS_POPUP` per window (`src/win32/html_hover.rs`), created hidden with the
popup host ~2 s after the toolbar activates, hosting `View::Hover` at the exact start URL
`bowser-ui://app/index.html?view=hover`. Styles `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`, plus
`WS_EX_TRANSPARENT` while it shows a tooltip; `WM_MOUSEACTIVATE` returns `MA_NOACTIVATE`, and it is
shown with `SWP_NOACTIVATE`, so hovering or clicking it never takes focus. Only the island menu's
name field activates it, when the page asks (`hact` `focus`) after a click on the field; Escape,
Enter or closing gives the keyboard back to the window. The host is `TrySuspend`ed 2 s after the
layer hides. HTML mode only: native mode keeps `card.rs`, `island_menu.rs` and the native tooltips.

Flow: Rust posts content with a fresh token, the page renders and measures it and replies
`{v:1,t:"size",tok,w,h}`, and Rust places the popup (tooltip centred under its anchor with a 6 px
gap, above it when there is no room; card and island left-aligned under it with a 4 px gap, 8 px
inside the edge), clamped to the anchor monitor's work area and scaled by that monitor's DPI, then
clips it to a rounded region (4 px tooltip, 8 px overlay). A 100 ms leave check closes a tooltip once
the pointer is off its anchor or on the tooltip, and the card or island once the pointer leaves the
hover square (`hover_square`), unless the island's name field has the keyboard; any of them closes
when another application comes to the front.

Tooltips: views mark elements with `data-tip` (never `title`). The toolbar, tab strip and rail
report a target after a 500 ms rest, at once when moving on to a neighbour (while one is up or
within 300 ms of it hiding), and hide on pointer leave, scroll, wheel, key press, pointer down and
blur (`ui/src/tooltip-logic.ts`). The card opens after `card::HOVER_DELAY_MS` on a non-active tab and
the island menu after `island_menu::HOVER_DELAY_MS` on a domain island's ball; a right-click on a ball
(`island_menu::open_now`) opens the HTML island menu at once when the layer is ready.

### JS -> Rust

| `t` | fields | from | meaning |
|---|---|---|---|
| `tip` | `text` (1-256 chars, not blank), `r` | toolbar, tabs, rail | show a tooltip under rectangle `r` |
| `tipoff` | - | toolbar, tabs, rail | hide the tooltip |
| `hover` | `k`: `tab`/`ball` with `id` (tab wire id / group key) and `r`; or `k`: `none` alone | tabs | pointer over a tab or ball, or neither |
| `hact` | `tok`, `a`, `n`/`text` | hover | `a`: `marker` (`n` 0-4), `next`, `row` (`n` 0-63), `swatch` (`n` 0-31), `close`, `name` (`text` <= 64 chars), `focus`, `dismiss` |
| `size` | `tok`, `w`, `h` | hover (and popup) | measured content size |

`r` is `"x,y,w,h"`: four canonical decimals (no sign, spaces or leading zeros), each <= 16384, in CSS
pixels of the sending view; the page clamps negative edges to 0. Rust converts it with the sending
host's own screen bounds and the window DPI. `n` and `text` appear exactly for the actions that take
them. Tab ids and group keys are checked against the live model (a ball only for a domain island),
and every action is checked against the token of the content shown and run from a posted window
message, outside WebView2's callback; row and swatch indices are checked against the rows and slots
last sent. `name` is accepted for the island session it was typed in, even after the menu closed (a
commit sent from the field's blur).

### Rust -> JS

| `t` | fields |
|---|---|
| `hover` | `tok` (>= 1), `kind: "tooltip"`, `text` |
| `hover` | `tok`, `kind: "card"`, `title`, `host`, `marker` (-1-4), `markers` (<= 5 colours), `sleeping`, `fav`, `rows`, `more` |
| `hover` | `tok`, `kind: "island"`, `name`, `hint`, `slot`, `swatches` (<= 32 colours), `rows`, `more`, `close` |
| `hoverhide` | - (the layer hid; the page clears its content, committing a changed island name first) |

`rows` is at most 12 `{title, active, linked, fav}`; `fav` is `""` or a validated PNG data URI, and
all favicons in one message share a 128 KiB budget. Colours are `""` (theme accent) or pass
`is_safe_css_colour`. Every key is required and no other key is accepted. The card mirrors
`tab_card.rs` (marker swatches and "+", title clamped to two lines, host, group rows with linked
underline, "+N more") and adds the tab's favicon and a "Sleeping" badge for discarded or suspended
tabs; the island menu mirrors `island_menu.rs` (name field with clear button and placeholder hint,
swatches, member rows, "+N more", "Close Tab Island · N Tabs").

Markup uses stable class names (`.bw-tooltip`, `.bw-card`, `.bw-island-menu` and their `-*` parts);
styles live in `ui/src/styles/hover.css` over the `--bw-*` design tokens with fallbacks to the
themed tokens, imported by `app.css` and inlined into `dist/app.css` by `build.mjs`.

## Popup pages: command palette and site information; name prompt dialog

The popup host (`?view=popup`) shows one page at a time, chosen by the message `t`: `menu` (menus),
`palette` and `siteinfo`. Rust owns every model; the page only renders and reports intent.

### Rust -> JS

| `t` | fields |
|---|---|
| `palette` | `v`, `tok`, `kind` (`"palette"`), `placeholder`, `items`: array (<= 1024) of `{id, s, label, detail, acc?}`; `id` unique u32 (the index Rust maps back to its action), `s` one of `tab`/`command`/`bookmark`/`history`, `label` <= 128 and `detail` <= 256 UTF-16 units, `acc` (accelerator label, <= 32) only for commands bound in `shortcuts.rs`. Rust caps tabs 100, bookmarks 200, history 200 (newest first, one row per address, http(s) only) and the whole message to 240 KiB (`html_popup::MAX_PAGE_BYTES`). |
| `siteinfo` | `v`, `tok`, `kind` (`"site-info"`), `host`, `origin`, `security` (as in `state`), `summary`, `perms`: array (0 or 5) of `{id, k, label, state}` with `k` `camera`/`mic`/`location`/`notifications`/`popups` and `state` `allow`/`block`/`default` (`default` = nothing stored = blocked), `cookies` `{state: count/unavailable/none, n, clear}`, `blocker` `{state: on/off/unavailable, n}`, `tracking` (level or `""`), `zoom` int 25..=500 (the page zoom in percent; 100 shows no zoom row), `reset` bool, `blockedHere` bool. Every key is required; the page rejects extra keys. |

The page filters the palette itself (exact, prefix, word prefix, detail prefix, then ordered fuzzy
matches, the same scoring as `productivity::rank_items`; 12 results grouped by section) and reports
its natural size with `size` as menus do; a page that grows or shrinks while open sends `size`
again and Rust only re-bounds the popup (no refocus). Placement: menus and site info at their
anchor; the palette centred on the window, its top edge 12% down the window, 560 CSS px wide.

### JS -> Rust

| `t` | fields | meaning |
|---|---|---|
| `pick` | `tok`, `id` | palette row chosen; `id` must be in the set Rust declared for that page (every row), then the action runs from the deferred queue (`MenuResult`) and the popup closes |
| `act` | `tok`, `id`, `n` (0..=7, `MAX_ACT_CHOICE`) | in-place action on an open page (site info); the popup stays open. Rust checks `tok` and that `id` was declared by the page with `n` within its range, runs it from the deferred queue (`DeferredChromeCommand::PopupAct`), then re-posts the page under the same `tok` |

Site info act ids: permission row `id` (1 + index in `Permission::ALL`: 1 microphone, 2 camera,
3 location, 4 notifications, 5 popups) with `n` 0 Allow, 1 Block, 2 Default; `10` clear cookies (only
declared when there are cookies); `11` blocker with `n` the wanted state (1 on, 0 off; only when
the blocker is available); `12` reset permissions (only when something is stored); `13` reset zoom to 100% (only declared while the page is zoomed). Policy is
unchanged: a request is granted only by a stored Allow, Default is blocked, and nothing ever
prompts. If the active site changed since the page opened, an `act` closes the popup instead.

### Toolbar indicator: `state.permblocked`

`state` carries `permblocked` (bool): a camera, microphone, location or notification request from
the active tab's current page was denied (no stored Allow). It is set by the WebView2
`PermissionRequested` handler (`EngineEvents::permission_denied`) and cleared when the tab starts a
new navigation. It is passive (the decision is already final; nothing prompts). The toolbar owner
styles it as a small icon in the pill; the site info page shows the same fact as `blockedHere`.
The field is optional for the page parser (older states omit it) and must be a bool.

### Toolbar zoom chip: `state.zoom`, `open` `zoomreset`

`state` carries `zoom` (integer 25 to 500, percent): the active tab's page zoom. The page parser
accepts it only as a whole number in that range (anything else drops the message) and treats an absent
field as 100. While it is not 100 the toolbar shows `.tb-zoom` inside the pill, between the address
field and the split button: a 20 px chip (inside the unchanged 26 px pill) with a 12 px magnifier
glyph and the percentage. Activating it sends `{v:1,t:"open",a:"zoomreset"}`, which runs the same
`App::zoom_active(Reset)` as Ctrl+0. The chip is hidden by the `hidden` attribute at 100% and by a
container query when the pill is narrower than 130 px. Site information carries the same value as
`siteinfo.zoom` with a Reset button (act `13`). The zoom itself is stored per origin in
`site-prefs.txt` (`src/site_prefs.rs`) and applied at navigation commit; see `docs/shortcuts.md`.

### Privacy dashboard permissions (`priv`, `src/chrome_host/panels/privacy.rs`)

The `privacy` snapshot's former `permissions` array is replaced by two members, built by
`panels::privacy::write_permissions`:

- `sites`: array (<= 1000, three keys each so the 4096-key page budget holds) of
  `{origin, current, kinds}`. `origin` is a canonical http(s) origin (scheme, host, port with the
  default port dropped; <= 256 chars), `current` marks the active tab's site (listed first, even when
  nothing is stored), and `kinds` is exactly five strings in the order `camera`, `microphone`,
  `location`, `notifications`, `popups`, each `allow`, `block` or `default` (`default` = nothing stored
  = blocked). The list holds every origin with a stored decision, the current site, and every origin
  in the recent denied list, so a site that just asked can be changed. Origins are unique.
- `denied`: array (<= 50, `DENIED_LOG_CAP`) of `{origin, kind, ago, n}`, newest first. `kind` is one of
  the five kinds above, `ago` is whole seconds since the latest request (so the page needs no clock
  or timezone), `n` is how many identical requests in a row the entry stands for (<= 999). The log is
  an in-memory ring in `App`: it is never written to disk, is gone when Bowser exits, and holds only the
  origin, kind and time, never a path, title or page content.

Commands (`{v:1,t:"priv",...}`, four fields at most, all validated in `panels::privacy::map_command`
before anything reaches the permission store; anything else is dropped):

| `a` | fields | meaning |
|---|---|---|
| `perm-set` | `text` canonical origin, `id` kind index 0..=4 (the `kinds` order), `n` 0 Allow / 1 Block / 2 Default | store a decision (Default removes the line) |
| `perm-reset` | `text` canonical origin | clear every stored decision for one site ("Reset site") |
| `perm-reset-all` | none | clear every stored decision, all sites ("Reset all"; the page asks for confirmation first, in the page) |
| `denied-clear` | none | empty the recent denied list |

`text` is accepted only when it is already a canonical origin (`permissions::canonical_origin`: an
http(s) origin equal to its own normal form, so no path, query, user info, uppercase host or default
port, and at most 256 characters). The old index-based `permission` command is removed. Policy is
unchanged: a request is granted only by a stored Allow for that exact origin, and nothing ever
prompts. The store refuses a new decision past 2048 stored rules (`MAX_RULES`).

Popups are a fifth stored kind (`popups` in `permissions.txt`). The `NewWindowRequested` handler asks
the store about the opener's exact origin: a click-initiated http(s) request opens a new tab as
before, a script-opened window opens as a tab only with a stored Allow, and every other request is
blocked (the default). An allow never widens the scheme gate: `javascript:`, `data:`, `file:`,
`about:`, `blob:` and `bowser-ui:` targets stay blocked. A blocked script popup is logged to the denied
list and sets `permblocked` like a refused permission request.

### Private tabs: `tabs[].private`, `state.private`

A private (InPrivate) tab is an ordinary tab whose controller Rust creates with
`ICoreWebView2ControllerOptions.IsInPrivateModeEnabled = true` and `ProfileName = "BowserPrivate"` (not the
chrome's `BowserChrome`), via the same `Environment10::CreateCoreWebView2ControllerOptions` path the chrome
hosts use; after creation Rust verifies `ICoreWebView2_13::Profile` really is that InPrivate profile and that
Strict tracking prevention read back, else the tab is not created. The wire carries only presentation state:

- Every `tabs` item has a required `private` bool placed after `muted`. A private tab is never grouped, so its
  `g` is `0` and it carries no group fields, and pinning does not put it in a group. The page validates the key
  strictly (a missing or non-bool value rejects the whole message) and sets `data-private="true"` on the tab.
- `state.private` (optional bool, rejected when present and not a bool) is true while the active tab is private;
  the toolbar pill shows the private indicator and sets `data-private="true"`.
- `siteinfo` carries an optional `private: true` (sent only for a private tab): `perms` is then empty,
  `reset` is false and no permission or blocker action is declared.

Nothing new goes JS -> Rust: the six-key and 16 KiB inbound limits are unchanged, and the page cannot make a tab
private (only the native `New private tab` command, Ctrl+Shift+P, can).

Interaction with per-site zoom and the permissions dashboard: a private tab keeps its zoom in the tab only (no
read from or write to `site-prefs.txt`, and the toolbar chip and the site information Zoom row still work); a
script-opened popup from a private tab is always blocked whatever `permissions.txt` holds; a private tab's
refused requests set `permblocked` but are not added to the recent denied list, and its site is never the
dashboard's `current` row.

### Name prompt dialog (`?view=dialog&p=rename`, `View::DialogRename`)

`profile_rename::prompt` (rename profile or workspace, new workspace, save session) hosts this
page in its own small window with `html_dialog` (`Kind::Rename`); the window stays hidden until
the page is ready and falls back to its native EDIT control after 2.5 s or on failure.
Rust -> JS: `{t:"snap",p:"rename",d:{title,save,value,hint,error,max}}`. JS -> Rust: the `ren`
family (`{v:1,t:"ren",a:"save",text}` or `{v:1,t:"ren",a:"cancel"}`, the usual panel field caps)
and `{t:"panel",a:"dismiss"}` (Escape). Rust validates again (trimmed, non-empty, <= `max`, no
control characters, plus the caller's rules: profile names use `profile::validate_name`) and
answers an invalid name with `error` in a fresh snapshot. Only the rename dialog host accepts `ren`.

### Popup readiness

The popup controller is requested together with the toolbar, tab strip and rail controllers when
the environment settles (last in that order), instead of 2 s after the toolbar became ready. The
chrome controllers share one renderer, so the hidden popup page is ready about when the toolbar
is. A menu requested earlier still falls back to native (and asks for the host if it is missing).
Perf-log events (`--benchmark-log`): `html_toolbar_ready`, `html_popup_prewarm`,
`html_popup_ready`, `html_popup_fallback`.

## Rail flyouts (History, Bookmarks, Customize)

With the HTML chrome, these three rail panels are flyouts, not docked panels (`docs/design/flyouts.md`).
The `?view=panel` controller lives in one owned, activating `WS_POPUP` per window
(`src/win32/flyout.rs`, created hidden and suspended with the chrome controllers); the page is
never re-bounded. The window is the surface plus a transparent shadow room (left 4, top 8, right
16, bottom 24 DIP; DWM frame extended over the client area so per-pixel alpha is honoured).

| Direction | Message | Meaning |
|---|---|---|
| Rust -> panel | `{v:1,t:"flyout",caret}` | `caret`: integer -1 to 4000, the caret centre in CSS px from the surface top (-1: no caret). Sent before `panelshow` on every open and switch; the page restarts its enter motion |
| panel -> Rust | `{v:1,t:"panel",a:"shown"}` | the page drew the snapshot that followed `flyout`; Rust shows and activates the window (a 300 ms fallback shows it anyway) |
| panel -> Rust | `{v:1,t:"panel",a:"dismiss"}` | Esc, the close button, or a press in the transparent shadow room |

Rust closes the flyout when it loses activation (an outside click, another app, a menu popup), on
a tab or workspace switch, on owner move or size, and when the same rail button is clicked again
(a click on the flyout a deactivation closed under 400 ms ago does not reopen it). While a
confirmation the flyout asked for is up, its deactivation is ignored. Opening a page in the current
tab closes the flyout and focuses the page; opening one in a new tab keeps it open. The panel
state's `solid` is `false`: the page paints its own opaque surface.

