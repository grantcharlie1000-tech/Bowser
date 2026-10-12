# HTML chrome: architecture and bridge contract (v1)

Status: milestone 1 (toolbar row only). The GDI chrome remains the default; the HTML chrome is opt-in with `BOWSER_UI=html` and falls back to GDI at runtime if its WebView2 process fails.

## Architecture (reviewed by Opus)

- One extra WebView2 **chrome host** controller, child of a Bowser-owned container window, covering only the 28 px toolbar row (back/forward/reload, omnibox, site-info, menu). Caption buttons, tab strip, resize border and every native hit-test stay native in milestone 1.
- Own WebView2 **environment** with its own user-data folder (`<local app data>\Bowser\ChromeUI`), no extensions, exclusive access, never shared with browsing profiles.
- Controller `DefaultBackgroundColor` is alpha 0 (set through ControllerOptions at creation, not an env var). The controller stays hidden until the page posts `ready`, so there is no white flash. Translucency comes from CSS over a transparent body. When the native backdrop is solid, Rust sends `solid:true` plus an opaque `--bg` and CSS paints it.
- Assets are embedded with `include_bytes!` from `ui/dist/` and served by a **custom scheme** `bowser-ui://app/` registered only in the chrome environment. A `WebResourceRequested` handler (filter `*`) answers every request from the embedded table, otherwise 404. No network, no files on disk. Tab environments never register the scheme; tab `NavigationStarting` also cancels `bowser-ui:`.
- Rust owns all geometry. JS never sizes the webview.
- Page-controlled text (titles, URLs, completions) is rendered with `textContent` only. No `innerHTML`/`outerHTML`/`insertAdjacentHTML`/`eval`/`new Function`/inline handlers. Trusted Types enforced by CSP.

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
| `open` | `a`: `menu`/`site`/`newtab` | open native menu / site-info / new tab |
| `drag` | - | left mousedown on an empty toolbar area (drag surface only, never a button, input or pill); Rust begins a native window move |
| `caption` | `a`: `toggleMaximize`/`systemMenu` | double-click (`toggleMaximize`) or right-click (`systemMenu`) on an empty toolbar area |

## Rust -> JS (`PostWebMessageAsJson`, object)

| `t` | fields |
|---|---|
| `state` | `scale` number (device pixel ratio hint), `solid` bool, `dark` bool, `back` bool, `forward` bool, `loading` bool, `progress` int 0-100, `security` one of `secure`/`notsecure`/`certproblem`/`internal`/`unknown`/`none`, `label` string (security summary, short), `theme` object of CSS colour strings keyed by token: `bg`, `fill`, `border`, `focusBorder`, `text`, `placeholder`, `completion`, `selection`, `hover`, `progress` |
| `setText` | `text` string, `select` bool (Rust-set omnibox text, e.g. on tab switch or Ctrl+L) |
| `focusOmnibox` | `select` bool |
| `completion` | `seq` int, `suffix` string (inline completion for edit `seq`; JS ignores stale seq or if value no longer matches) |

`theme` is the only nested value and is Rust->JS only (Rust is trusted).

## Display rules enforced in JS

- Rust formats display URLs; JS never parses URLs.
- `dir="auto"`, `unicode-bidi: isolate` on text from pages.
- Favicons are not used in milestone 1.

## Rust module API contract (`src/chrome_host/`)

- `protocol.rs`: `pub enum Command { Ready, Nav(NavAction), Edit{seq:u32,text:String}, Submit(String), Escape, Focus(bool), Open(OpenTarget) }`, `pub enum NavAction{Back,Forward,Reload,Stop}`, `pub enum OpenTarget{Menu,Site,NewTab}`, `pub fn parse_command(raw:&str)->Result<Command,ProtocolError>` (strict, size-capped, pure), `pub struct ViewState{...}` with `pub fn to_json(&self)->String`, `pub fn set_text_json(text:&str,select:bool)->String`, `pub fn focus_json(select:bool)->String`, `pub fn completion_json(seq:u32,suffix:&str)->String`. No external JSON dependency.
- `assets.rs`: `pub const SCHEME:&str="bowser-ui"; pub const START_URL:&str="bowser-ui://app/index.html"; pub fn lookup(path:&str)->Option<Asset>` where `Asset{bytes:&'static [u8], mime:&'static str}`; `pub const RESPONSE_HEADERS:&str` (CRLF-joined for `CreateWebResourceResponse`).
- `host.rs`: `ChromeHost::begin_create(parent:HWND, bounds:RECT, user_data_dir:&Path, on_command:Rc<dyn Fn(Command)>, on_failed:Rc<dyn Fn()>, on_ready:impl FnOnce(Result<ChromeHost>))`, `set_bounds`, `post_json(&str)`, `focus()`, `set_visible(bool)`, `close()`.
