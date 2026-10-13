# Reader view, distraction hiding and the media check

## Reader view

**Entry points.** Ctrl+Alt+R, the book button in the address pill (HTML chrome only; it appears when the page looks like an article and stays pressed while reader view is on), **Reader view** and **Hide distractions** in the main menu, and the same two commands in the command palette. **Leaving:** Ctrl+Alt+R again, the pressed toolbar button, Esc, or any navigation (including a reload and a same-document address change).

**Design.** Reader view is a restyle of the live document, not a new page: the script marks the article block and its ancestors, hides everything that is not on that path, neutralises layout on the ancestors (so a grid or flex page lays the article out as one column), and applies one constructed stylesheet. There is no navigation, no network request and no history entry, and leaving puts every attribute, element, style and scroll position back (the fixtures check `outerHTML` before and after). Replacing the body (the alternative) was rejected because it is not reversible without a reload.

**On demand only.** Nothing is injected at document creation (`AddScriptToExecuteOnDocumentCreated` is not used), so a page that is never read pays nothing. The one automatic step is a read-only probe through `ExecuteScript` after a page finishes loading in the active tab (and when a background tab that finished loading is shown). It decides whether the toolbar button is offered. Its work is a scan of at most 1500 paragraphs inside a 250 ms budget; it changes nothing and returns three numbers.

**Where the code is.**

| File | Role |
|---|---|
| `src/reader/reader.js` | The page script: a function expression called with an options object. Scoring, restyle, hiding, control bar, cleanup |
| `src/reader/reader.css`, `bar.css` | The styles it applies (passed in as strings, not read from the page) |
| `src/reader/mod.rs` | Pure decisions: eligible URLs, the article threshold, building the call, strict result parser, per-tab state |
| `src/engine/webview2.rs` `run_reader_script` | The only place a script is run; re-checks that the live address is http(s) |
| `src/app.rs` (reader view section) | Per-tab state, the probe, toggle, leave and the stale-answer guard |
| `src/shortcuts.rs`, `src/win32/window.rs` | Ctrl+Alt+R, the menu items, the palette commands, the toolbar button, the passive "nothing to read" toast |
| `ui/src/view.ts`, `ui/src/protocol.ts` | The pill button, `state.reader`, `open` target `reader` |

**Finding the article.** Written from scratch (no Readability code): every paragraph of at least 50 characters votes for its nearest three ancestors (1, 1/2, 1/3 of 1 + commas + length/100 capped at 3), starting from a base that favours `article`, `main`, `[role=main]` and `[itemprop=articleBody]` and article-like class words and penalises sidebar, comment, share, ad and similar class words. Paragraphs inside `nav`, `aside`, `footer` or the usual landmark roles, or within three levels of a negative class word, do not vote. The eight best candidates are scored by votes times (1 minus link density) and the best wins; it then widens to an enclosing `article` (or, failing that, `main`) when that adds little text, so the title and lead come along. Word counts ignore script and style text and weigh CJK characters as half a word.

**What stays and goes.** Kept: text, images, figures and captions, lists, tables, code, block quotes, video. Hidden: scripts, styles, iframes, objects, forms and form controls, navigation, asides, footers, dialogs, `[aria-hidden=true]`, fixed and sticky elements, cookie and consent banners, and blocks whose id or class says sidebar, comments, share, related, newsletter and similar. A missing title is added as an `h1` from the page's first heading or its title.

**Controls.** A closed shadow root at the bottom of the page holds A-, A+ and Auto, Light, Sepia and Dark, built with DOM calls only (`createElement`, `textContent`; no HTML strings) and styled from Bowser's colours. Auto uses the colours Rust passes in (Bowser's solid theme palette, validated by `is_safe_css_colour`); Light, Sepia and Dark are fixed. The size is 14 to 30 px in steps of 2 and the choices are returned to Rust when reader view ends, so the next use starts from them (session only). The same holds in private tabs, where reader view is allowed: the settings live only in memory for the session and are never written to a file (`reader::tests::reader_settings_are_session_only_and_never_touch_disk`, `app::tests::reader_view_works_in_a_private_tab_and_its_settings_stay_in_memory`).

**Strict CSP pages.** Styles are applied as a constructed stylesheet (`adoptedStyleSheets`) and through CSSOM custom properties, neither of which a page's `style-src` blocks; a `<style>` element is only a fallback. A fixture with `style-src 'none'` covers this.

**Security.**

- The script reads and changes only the page it is run in, and returns only `r1|code|state|words|link|theme|size`. Rust's `parse_result` accepts exactly that shape (alphanumerics and `|`, at most 48 characters, known tokens, digits in range) and drops anything else, so nothing a page controls reaches Rust or any other place.
- The script has no way out: no `fetch`, XHR, beacons, sockets, storage, cookies, `postMessage`, `eval`, `new Function`, dynamic `import` or HTML-string sinks (checked by a Rust test and a Node test). It adds no web message channel, so the page cannot talk back to Bowser.
- It never runs on `bowser-ui:` (tab WebViews cannot show it), `about:`, `file:`, `data:`, `blob:`, `view-source:` or any non-http(s) address: `reader::url_allowed` gates the app call and the engine re-reads the live address before `ExecuteScript`.
- A hostile page can tamper with natives, because the script shares its world. Natives are captured once into a frozen table, every loop is bounded (1500 paragraphs, 4000 scanned elements, a 250 ms budget), reads that can throw (`getComputedStyle`, `document.title`) are guarded, and the cleanup path uses only indexed loops. Worst case the script returns `err` and the page is put back as it was (the `hostile.html` fixture). A page can always break its own reader view; it cannot make the script return anything but the fixed shape.
- A stale answer (the tab navigated while a call was running) is recognised by a per-document epoch and dropped.

**Limits.** Reader view works on content that is in the page's light DOM and rendered when you ask; content in shadow roots, canvases or cross-origin frames is not seen. Pages that are single-screen apps, search results written with `p` snippets, or very short articles can be misjudged either way. After a same-document route change the button is hidden until the next load. The toolbar button needs the HTML chrome; the shortcut, menu and palette work in both. AltGr+R toggles it on layouts where AltGr reports as Ctrl+Alt.

**Tests.** `src/reader/mod.rs` (eligibility, result parser, call builder, tab state), `src/shortcuts.rs` and `src/chrome_host/host.rs` (the chord), `src/chrome_host/protocol.rs` (wire values). `ui/test/reader.mjs` runs the real script in headless Edge against `ui/test/fixtures/reader/` (semantic article, div-soup blog, encyclopedia, title outside the body, CJK, front page, dashboard, search results, overlay page, hostile page, strict-CSP page, and a generated 8000-element page) and checks the choice of block, what stays and goes, the 68-character measure, controls, preferences, exact restoration and the time budget. It skips itself when Edge is not installed (set `BOWSER_TEST_EDGE` to use another Chromium).

## Media support check

Settings has a **Media support** section. Opening it runs a client-side check once, and **Check again** repeats it. It runs inside the settings panel's own page (the `bowser-ui` chrome WebView, same WebView2 runtime as tabs), so no page is scripted and no network request is made: `HTMLMediaElement.canPlayType`, `MediaSource.isTypeSupported` and `MediaCapabilities.decodingInfo` for H.264, VP9, AV1 and HEVC at 1080p30, and `navigator.requestMediaKeySystemAccess` for Widevine and PlayReady (availability only; no key session is created). Each probe has a 3 s timeout. A format is shown as hardware decode when `decodingInfo` reports supported and power efficient, software decode when supported but not power efficient, unsupported, or unknown when the API is missing. Advice is text only (for example, install the HEVC or AV1 video extension from the Microsoft Store). The result describes the chrome profile; tabs use the same runtime, codecs and CDMs.
