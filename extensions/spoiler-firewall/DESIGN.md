# Spoiler Firewall for GTA VI — architecture and threat model

Status: draft 1, 2026-11-17 (GTA VI launches 2026-11-19). Unofficial; not affiliated with Rockstar
Games or Take-Two Interactive.

One detection engine, two distributions:

1. A Manifest V3 Chrome extension (Chrome Web Store).
2. A built-in Bowser feature, on by default, injected natively by Bowser's WebView2 engine.

Primary objective: minimise the probability that a user ever sees an unwanted GTA VI spoiler.
Priority order: (1) no accidental exposure, (2) detection breadth, (3) deliberate per-item reveal,
(4) responsiveness and privacy, (5) a beautiful, unobtrusive interface.

---

## 1. Platform facts that shape the design

### Chrome (MV3)
- Content scripts declared in `manifest.json` with `run_at: "document_start"` run after manifest
  CSS is inserted and **before any DOM is constructed or any page script runs**. Manifest CSS is in
  place before first paint.
- Content scripts run in an isolated world. They cannot patch page prototypes (`attachShadow`,
  `HTMLMediaElement.prototype.play`). A second content script with `"world": "MAIN"` (Chrome 111+)
  can. `chrome.dom.openOrClosedShadowRoot(el)` lets the isolated world reach closed shadow roots.
- `customElements` is `null` in isolated worlds, so the overlay uses an undefined autonomous custom
  element name (`sfw-shield`) as a shadow host. That is legal without a definition.
- MV3 service workers are suspended after ~30 s idle. The text engine therefore runs **entirely in
  the content script**. The service worker only coordinates OCR, settings and counters.
- OCR needs WebAssembly + workers: an offscreen document (`chrome.offscreen`, reason `WORKERS`)
  hosts Tesseract (bundled locally; remote code is forbidden by store policy).
- Cross-origin images taint canvases. The service worker can `fetch()` image URLs with host
  permission, so OCR input is fetched there, not read from the page.

### Bowser (WebView2)
- WebView2 can load unpacked extensions, but only if the environment is created with
  `AreBrowserExtensionsEnabled`. Extensions do not run in Bowser's private tabs, and Microsoft does
  not publish which extension APIs WebView2 supports. Building on that path would leave private tabs
  and guest windows unprotected and depend on undocumented APIs.
- Bowser already ports extension behaviour natively (Bowser Shield → `src/shield.rs`) and injects
  scripts (`AddScriptToExecuteOnDocumentCreated`, `ExecuteScript`, DevTools protocol).
- **Decision:** Bowser injects the shared engine bundle with
  `ICoreWebView2::AddScriptToExecuteOnDocumentCreated` in every tab (normal, private, guest),
  which runs before any page script in the top document and in child frames. Settings are embedded
  into the injected script text by native code, so they are available synchronously.
- That script runs in the **main world**. Consequences, accepted and documented: page scripts can
  observe or tamper with it. The adversary we defend against is a *poster* on a platform (who cannot
  run script), not the platform itself. The bundle captures the DOM primitives it needs before page
  scripts run, keeps no global except one non-enumerable frozen hook, and uses closed shadow roots.
- Main world means page CSP and Trusted Types apply: the engine never uses `innerHTML`, never
  creates `<style>` elements, never uses `eval`/`Function`, and styles only through constructable
  stylesheets (`adoptedStyleSheets`) and CSSOM, which CSP does not block.
- OCR in Bowser cannot rely on page workers (CSP `worker-src`). The native adapter is
  **Windows.Media.Ocr** (built into Windows 10/11, offline). See §8.

---

## 2. Threat model

### Assets
The user's ignorance of GTA VI story content: plot, endings, deaths, betrayals, character arcs,
late-game reveals, leaked footage.

### Adversaries
| Adversary | Capability | In scope |
|---|---|---|
| Casual poster | Writes "GTA 6 ending: …" plainly | Yes |
| Evasive poster | Misspellings, leetspeak, homoglyphs, zero-width chars, spacing tricks, hashtags, text in images, coded references, other languages, splitting text across elements | Yes |
| Troll/raider | Posts spoilers in unrelated threads ("cold context") with no franchise name | Partially: cue-only text in cold context is undetectable without GTA context; images of the game without text need visual classification (limited, §7) |
| Platform/page script | Full control of its own page | Out of scope (cannot be defended against from inside the page) |

### Exposure channels (each must be covered or documented)
1. Rendered text and media in the page (first paint, dynamic insertion, virtualised list recycling).
2. Text inside shadow DOM and iframes (same- and cross-origin).
3. Image pixels (screenshots, memes, thumbnails) and video frames/audio (autoplay).
4. Hover cards, hover-to-play previews, tooltips (`title`), link status bubble (`href` slug).
5. Text selection/copy, find-in-page, accessibility tree (`aria-label`, alt text).
6. Document title (tab strip, history, window title), URL slugs (address bar).
7. Search suggestions (site-rendered dropdowns; Bowser's own omnibox).
8. Browser-generated previews (Bowser tab hover thumbnails).
9. Notifications (web notifications text) — out of reach for the Chrome extension; documented.

### Failure modes we design against
- **Flash**: content painted before classification. Mitigation: classification is synchronous
  in the MutationObserver callback (runs before the next rendering opportunity) plus a
  `requestAnimationFrame` safety pass for parser text appends that do not create mutation records;
  pre-paint CSS quarantine for media on high-risk hosts.
- **Silent failure**: exception, budget overrun, OCR crash, SW suspension. Mitigation: every
  failure path ends in a *protective* state (pending shield or page shield), never in a clear state.
  Failures increment visible counters.
- **Reveal leak**: revealing one item exposes neighbours, or recycled DOM shows new content under a
  revealed container. Mitigation: reveal binds to (element, content fingerprint); a changed
  fingerprint re-classifies.
- **Tamper by site framework**: React removes our attributes/overlay. Mitigation: we observe our
  own attributes and re-apply.

---

## 3. Module layout (one engine, two hosts)

```
extensions/spoiler-firewall/
  DESIGN.md                 this document
  README.md                 install, maintain, release
  LIMITATIONS.md            limitations and security assessment (kept honest, updated with tests)
  engine/                   SHARED. Plain classic-script fragments; no chrome.*, no Bowser APIs.
    normalize.js            SFW.normalize: adversarial text normalisation → views
    lexicon.js              SFW.lexicon: data (terms, entities, cues, safe patterns, languages)
    classify.js             SFW.classify: text → {score, level, signals}
    policy.js               SFW.policy: modes, thresholds, media/hot-context rules → decision
    adapters.js             SFW.adapters: platform adapter table + generic resolver
    styles.js               SFW.styles: CSS text (page sheet, shield sheet, quarantine sheet)
    shield.js               SFW.shield: overlay create / reveal / re-blur / size variants
    runtime.js              SFW.runtime: observers, scheduler, unit resolution, guards
  hosts/
    chrome/                 MV3 wrapper: manifest, content.js (isolated), content-main.js (MAIN),
                            service-worker.js, offscreen.html/js (OCR), popup, options, icons
    bowser/                 bowser-host.js: boots SFW.runtime with native-injected config
  vendor/                   third-party (Tesseract) used by the Chrome host only
  build/                    build.mjs (no deps), package.mjs (zip), validate.mjs (release checks)
  dist/                     generated: chrome/ (unpacked), bowser/spoiler-firewall.js, node/engine.cjs
  test/                     unit (node --test), corpus (generator + metrics), e2e (Playwright)
  store/                    Chrome Web Store listing, privacy policy, graphics
```

### Build
`build/build.mjs` (Node, no dependencies) concatenates `engine/*.js` in fixed order inside one
closure `(function(){ "use strict"; const SFW = {}; …; return SFW; })()` and emits:
- `dist/node/engine.cjs` — `module.exports = SFW` (tests exercise the shipped code).
- `dist/chrome/…` — the unpacked extension; `content.js` = engine + chrome host in one file.
- `dist/bowser/spoiler-firewall.js` — engine + Bowser host, one IIFE, with the literal placeholder
  `/*__SFW_CONFIG__*/null` which Bowser replaces with a JSON object. Bowser embeds this file via
  `include_str!`; the file is committed, and a check fails if it is stale.

### Host interface (what the engine needs from a host)
```js
host = {
  config,                         // {enabled, mode, reducedEffects, siteOverrides, version}
  configReady: Promise|null,      // null when config is synchronous (Bowser)
  ocr: null | { request(req) -> Promise<{status:"ok", text} | {status:"unavailable"|"error"|"skipped"}> },
                                  // req = {url (http/https/data/blob), kind: img|poster|bg|frame, width, height, naturalWidth, naturalHeight}
                                  // null => the engine behaves exactly as without OCR. The promise may take seconds; the host enforces its own timeout.
  ocrMaybe: boolean,              // optional: true while the host config is loading and OCR may turn out to be enabled (media is held, not released)
  report(code, n=1),              // counters only; codes never contain page text
  onConfigChange(cb),
  privileged: { openOrClosedShadowRoot(el) } | null,
}
```

### Engine module contract (binding for all implementers)
Fragments assign `SFW.<name> = (function () { …; return api; })();`. A fragment may reference
other `SFW` members only inside functions (lazy), never at fragment top level. No fragment touches
`document`/`window` at top level (the Node build must load without a DOM).

```text
SFW.normalize.views(text) -> Views            (opaque to callers other than classify)
SFW.normalize.clean(text) -> string           (NFKC/confusable/zero-width-stripped, lowercased)

SFW.classify.text(text, ctx?) -> Result
  ctx    = { hot: 0|1|2, source: "text"|"ocr"|"title"|"url"|"meta"|"caption"|"alt" }
  Result = { score: 0..1, level: "none"|"mention"|"risk"|"spoiler",
             signals: [{ kind, source }],                 // categories only, never page text
             flags: { franchise, entity, entityWeak, cue, cueWeak, marker, safe } }
SFW.classify.url(urlString) -> Result          (path slugs, query params like q=/search_query=)
SFW.classify.isHotText(text) -> boolean        (franchise or strong entity present)

SFW.policy.MODES = ["lockdown", "strict", "balanced"]; SFW.policy.DEFAULT_MODE = "strict"
SFW.policy.decide(result, opts) -> "shield" | "clear" | "hold"
  opts = { mode, hot: 0|1|2, kind: "item"|"media"|"title"|"page", hasMedia, highRisk, ocrDone }
SFW.policy.decideAll(result, opts) -> { lockdown, strict, balanced }   // for pending-until-config

SFW.adapters.forLocation(locationLike) -> Adapter
  Adapter = { id, highRisk, item, thread, context, media, hotUrl(url) -> boolean }
  (selectors are strings; generic fallback always merged in)

SFW.styles.PAGE_CSS / QUARANTINE_CSS / SHIELD_CSS : string
SFW.shield.apply(container, { variant: "spoiler"|"pending"|"error"|"media"|"page" })
SFW.shield.reveal(container) / reblur(container) / release(container) / state(container)
SFW.runtime.start(host)
```

---

## 4. Detection

### 4.1 Normalisation (`SFW.normalize`)
Input string → several views, each with an index map back to token boundaries:
1. NFKC; NFD then strip combining marks (`\p{M}`); strip format/invisible chars (`\p{Cf}`, ZWJ/ZWNJ,
   variation selectors, soft hyphen, word joiner, tag characters, bidi controls).
2. Confusables fold: Cyrillic/Greek/Armenian/Cherokee look-alikes, enclosed/squared/regional-
   indicator letters, fullwidth (NFKC), small caps, mathematical alphanumerics (NFKC), upside-down
   text, common emoji-letters (🅶 🆃 🅰). Lowercase.
3. `spaced` view: runs of non-letter/digit → single space.
4. `compact` view: letters/digits only, with token-boundary map. A match in the compact view counts
   only if it starts at a token start and ends at a token end ("g.t.a 6", "G T A V I", "#GTAVI"
   match; "bigtavish" does not).
5. `leet` fold of both views (0→o 1→i/l 3→e 4→a 5→s 7→t @→a $→s !→i |→l), digits preserved in a
   parallel view so "gta 6" is never destroyed by the fold.
6. Roman numeral / number-word equivalence: vi ≡ 6 ≡ six ≡ sechs ≡ seis ≡ шесть ≡ 六 …
7. Repeated-letter squash ("gtaaaa 66" → "gta 6"), hashtag/camel-case split (#GtaSixEnding).

### 4.2 Lexicon (`SFW.lexicon`)
Data only, versioned, maintained per release. Categories:
- `franchise_strong` — GTA VI / GTA 6 / GTA6 / Grand Theft Auto VI / GTA Six / "the new GTA" and
  multilingual names (グランド・セフト・オートVI, 侠盗猎车手6, ГТА 6, …).
- `entity_strong` — names specific to GTA VI: Lucia Caminos, Jason Duval, Leonida, Port Gellhorn,
  Ambrosia, Grassrivers, Mount Kalaga, Leonida Keys, Cal Hampton, Boobie Ike, Dre'Quan Priest,
  Real Dimez, Raul Bautista, Brian Heder, Only Raw Records, …
- `entity_weak` — ambiguous names that are only evidence alongside other signals: Lucia, Jason,
  Vice City, Cal, Raul, Boobie, Rockstar, R*, "GTA" alone, Take-Two.
- `cue_strong` — spoiler, ending, final mission, dies/died/killed, death, betray, twist, leak(ed),
  plot, epilogue, post-credits, "who dies", "ending explained", "full playthrough", "all cutscenes",
  "turns out", "[spoilers]", Reddit `>!…!<`, "spoiler alert", and their multilingual forms.
- `cue_weak` — story, mission, character, cutscene, chapter, boss, villain, reveal, ending-adjacent.
- `safe_logistics` — release date, price, preorder, preload, download size, platforms, trailer 1/2,
  PC port, countdown, "can't wait", reviews score. Lowers risk only in Balanced/Strict, never in
  Lockdown, and never when any entity or cue is present.

### 4.3 Matching
- Exact token/phrase match on `spaced` views; token-aligned match on `compact` views; leet views.
- Bounded fuzzy match: Damerau-Levenshtein ≤1 for terms ≥5 letters, ≤2 for ≥9 letters, token-aligned,
  on franchise/entity terms only (protects against "Grand Thef Auto", "Lusia Caminos").
- Text is classified in windows (≤4,000 chars, 500 overlap) so long text cannot hide a spoiler past
  a cap; each container aggregates the max window score.

### 4.4 Risk score (`SFW.classify`)
Independent signals combine by noisy-OR (`1 − Π(1 − pᵢ)`) plus interaction rules:
- One high-confidence signal is enough (e.g. franchise_strong + cue_strong ⇒ spoiler ≥ 0.95).
- Weak signals compound (entity_weak + cue_weak + hot context ⇒ above threshold).
- Context (`hot`) raises the prior: in a GTA-context thread, cue_strong alone ⇒ spoiler.
Output `{score, level: none|mention|risk|spoiler, signals:[{kind, source}]}`. **Signals carry
categories and sources only, never matched text.**

### 4.5 Policy (`SFW.policy`), three modes
| | Lockdown | **Strict (default)** | Balanced |
|---|---|---|---|
| franchise/entity_strong mention alone | shield | shield (unless short + safe_logistics + no cue/entity) | clear |
| franchise/entity + any cue | shield | shield | shield |
| hot context, any cue / weak entity | shield | shield | shield |
| hot context, any item at all | shield | shield items with media; text items need ≥ weak signal | needs cue |
| media in hot context or flagged unit | shield | shield | shield |
| media on high-risk host, cold context | hold until OCR clean | hold until OCR clean | metadata only |
| classification failure / budget overrun | shield | shield | shield |

Before an asynchronous config load completes (Chrome), a unit whose decision differs between modes
is held `pending` (protective) and resolved when config arrives.

### 4.6 Context ("hot")
Hot scopes propagate down: page (URL, title, `h1`, `og:title`, `meta description`, search query in
URL), thread (adapter thread root: Reddit post, X conversation, YouTube watch page, forum topic).
Page context selectors are page-level only (search box, page / channel heading, subreddit header), never a
selector that matches every post of a feed. A thread's head post defines its heat only on real thread pages
(`threadUrl`) or per-post roots (`threadFirst`), and `threadContext` is read inside the root only.

Revision 3 (2026-11-18, after real-feed measurements): feed warmth (≥3 GTA shields within 30 s) no longer heats
other posts. During the launch window every feed is warm within seconds, and warmth covered any post that said
"dying", "the end" or "story", and re-ran the whole page on every warm/cold flip. One "GTA 6 before ..." joke reply
also made its whole X conversation hot (the context selector matched every tweet). In Strict / Balanced, media is
covered for its post's words only at mention level or with a strong franchise / entity signal; a bare "risk"
(one common first name, a "spoiler alert" about something else) is not GTA evidence. Lockdown is unchanged.

Counter ticks (revision 3): a text node or a text attribute whose value changes only in its numbers ("12 Likes" ->
"13 Likes", "5h" -> "6h", "0:31" -> "0:32", also a text node swapped for such a value) does not re-classify its post. Feeds
tick these many times a second, and each tick re-read and re-classified the whole post. Posters cannot change text in
place; only the site's script can. A change next to franchise words (gta, grand theft, vice city, leonida, rockstar, гта
in the value or its parent / grandparent text) still takes the full path, so "GTA 5" -> "GTA 6" in place is re-judged.

---

## 5. Runtime (`SFW.runtime`)

### 5.1 Zero-flash pipeline
1. **document_start**: adopt the page sheet into `document`; on high-risk hosts also the media
   quarantine sheet (media blurred by default unless `[data-sfw-m="ok"]`). Install a
   MutationObserver on `document` (`childList, subtree, characterData, attributes` with
   `attributeFilter: [alt, title, aria-label, aria-description, placeholder, href, src, srcset,
   poster, style, data-sfw*]`).
2. **MutationObserver callback** (microtask, before rendering): collect affected units, dedupe,
   classify synchronously, apply shields. Budget 12 ms per callback; units beyond the budget are
   marked `pending` (shielded, "Checking…") and finished in the next idle slice. Pending is never
   cleared by a timer, only by a completed classification.
3. **rAF safety pass**: while `document.readyState === "loading"` (and for 2 s after any large
   insertion), re-check text nodes that may have grown via parser appends without mutation records.
4. **Unit resolution**: adapter item selector → generic item selectors (`article, [role=article],
   li, [role=listitem], [role=option], blockquote, figure, tr, dd`) → nearest block ancestor below a
   size cap. Classification runs on the unit's aggregated text: `textContent` + `alt`, `title`,
   `aria-label`, `aria-description`, `placeholder`, `href` path slugs, `img` file names, `video`
   `title`/`aria-label`, captions/track labels.
5. **Fingerprint cache**: `WeakMap<Element, {fp, decision}>`; FNV-1a of normalised text. Unchanged
   units are not re-classified.

### 5.2 Shield presentation (`SFW.shield`)
- Container gets `data-sfw="shield"`; if computed `position` is `static` it also gets
  `data-sfw-pos` (→ `position: relative`). Always `isolation: isolate`.
- Element children (except our overlay) get `inert` (removes them from focus, selection, hover,
  find-in-page and the accessibility tree) and `filter: blur()` via the sheet. We record which ones
  we set and restore exactly those.
- Overlay `<sfw-shield>` is appended as the last child: `position:absolute; inset:0;
  z-index:2147483647`, closed shadow root with `backdrop-filter: blur(24px) saturate(.6)` + tint
  (covers direct text nodes and silhouettes). Contents: a centred **SPOILER** stamp and the subtitle
  "GTA VI content detected · Click to reveal". Size variants: pill (< 64 px tall), stamp, large.
- The overlay is a `role="button"` with an accessible name ("Reveal GTA VI spoiler"), `tabindex=0`,
  Enter/Space/click/tap reveals. `pointerdown` is stopped so hover cards and hover-autoplay never
  trigger under a shield.
- **Reveal**: ~180 ms fade/unblur on that container only; records (element, fingerprint). A small
  "Hide again" control (focusable, visible on hover/focus) re-shields it. Neighbours are untouched.
- Reduced effects (setting or `prefers-reduced-motion`/low-end device): opaque tint, no backdrop
  blur, no animation.
- Layout is never changed: no element removal, no collapsing, no size changes; the overlay is out of
  flow.

### 5.3 Media
- `img`, `picture`, `video`, `canvas`, `svg image`, and elements with `background-image` are
  resolved to a media unit (the smallest ancestor that contains only that media, up to 3 levels,
  else the item unit).
- Video in a shielded/pending unit: paused, muted, `autoplay` removed. The MAIN-world hook (Chrome)
  or the bundle itself (Bowser) wraps `HTMLMediaElement.prototype.play` to refuse playback for held
  media, so there is no audible blip.
- OCR requests (engine/runtime.js "media OCR gate", only when `host.ocr` is non-null): media of units that are clear in a cold
  context on a high-risk host stays CSS-quarantined (no flash) until the host settles; eligible = img / video[poster] / inline
  background-image, rendered and natural size both >= 96x64 CSS px (smaller: v1 rule, released). Requests only for media within
  ~1.5 viewports (IntersectionObserver rootMargin), nearest to the viewport first, at most 3 in flight per frame, deduped and
  cached by URL for the page lifetime; src / srcset / currentSrc / inline-style changes re-check. The OCR text is classified
  with source "ocr" and also concatenated with the unit's own text; either shielding shields the whole unit, else the media is
  released with a short fade. unavailable / error: strict + lockdown keep a click-to-reveal "Couldn't check image" cover on the
  media (balanced releases); skipped: released unless lockdown. Counters: ocr.ok / ocr.unavailable / ocr.error / ocr.skipped.
- Profile pictures (adapter `avatars`) are left out of all of this: the quarantine rules carry a `:not(<avatars>)` (only when the
  combined selector parses; otherwise they stay held), they get no OCR record, and they never make a unit a picture post. A
  covered unit still covers its profile picture with the rest of it.
  The Chrome host implements it with Tesseract.js in an offscreen document (hosts/chrome/offscreen.js, vendor/).

### 5.4 Other guards
- Shadow DOM: `attachShadow` is wrapped to remember every root and report it synchronously. For the Chrome host (MAIN-world
  hook, isolated-world runtime) it also marks hosts with `data-sfw-sr`, and the isolated world reaches closed roots via
  `chrome.dom.openOrClosedShadowRoot`; Bowser and the test host share the hook's world and read its map instead (no mark,
  §13). Each root gets its own observer, and the page sheet when a mark is first written in it (§13). Declarative shadow
  roots are found on insertion.
- Iframes: `all_frames`, `match_about_blank`, `match_origin_as_fallback`; each frame runs its own
  engine. The top frame shares page context with child frames through `postMessage` (best effort).
- Title guard: if `document.title` classifies ≥ mention, the title becomes "Spoiler-protected page"
  until the user reveals the page title explicitly.
- Copy guard: on `copy`, text from shielded units is removed from the clipboard payload.
- Page shield: if the page itself is hot at `spoiler` level from URL/title, or if the engine fails to
  start, a fixed full-viewport shield ("This page may contain GTA VI spoilers · Click to view page")
  is shown. Revealing the page does not reveal individual shielded items.

### 5.5 Platform adapters
Data-driven `{id, hosts, item, thread, context, media, highRisk, hotUrl}` for X, Reddit (new/old/
sh), YouTube, TikTok, Instagram, Facebook, Threads, Bluesky, Twitch, Discord Web, Mastodon (any
host with Mastodon markup), Google, Bing, DuckDuckGo, Yahoo, Brave Search, Startpage. Selector drift
degrades to the generic resolver, never to "no protection".

---

## 6. Settings and persistence
- Defaults: enabled, Strict. Stored in `chrome.storage.local` (Chrome) / the profile's
  `settings.txt` (Bowser: `spoiler_protection`, `spoiler_mode`, `spoiler_reduced_effects`).
- Turning protection off or loosening the mode is an explicit user action, shown in the UI as
  "Protection off". Nothing turns protection off automatically. Restart never resets protection.
- Reveals are per page view; they are not persisted.

## 7. Image and video detection limits (honest)
- OCR finds text in images. A screenshot of the game with no text and no GTA context cannot be
  identified by OCR. A local visual classifier would need a trained model of GTA VI imagery; no
  such licensed dataset is available, so v1 ships without one. Strict mode holds media on
  high-risk hosts until OCR completes; images with no readable text in a cold context are then
  released. Lockdown mode keeps them shielded. This is the main residual risk and is stated in the
  listing and in LIMITATIONS.md.
- Video: titles/descriptions/captions/thumbnails are inspected; frames only where same-origin or
  CORS-readable (MSE players). Local audio transcription (e.g. Whisper-tiny WASM, ~40 MB) is
  investigated but not shipped in v1.

## 8. Bowser integration
- `src/spoiler.rs`: settings model, config JSON, `include_str!` of the bundle with placeholder
  substitution, unit tests.
- `src/engine/webview2.rs`: `AddScriptToExecuteOnDocumentCreated` with the bundle for every tab
  (normal, private, guest) at controller configuration, before first navigation, fire-and-forget
  (same rule as the login script: never wait on the completion inside the creation callback).
  Settings changes re-register the script for new documents.
- Settings page section and Extensions list entry ("Spoiler Firewall · built in", toggle, mode).
- OCR adapter (phase 2): Windows.Media.Ocr over image responses observed by the tab
  (`WebResourceResponseReceived` for image resources on high-risk hosts), results delivered with
  `ExecuteScript` to the bundle's hook. Native never fetches on behalf of the page.

## 9. Testing
- Unit: normaliser, matcher, classifier, policy (node --test on `dist/node/engine.cjs`).
- Corpus: generator producing ≥5,000 labelled synthetic positives and negatives across languages and
  evasion transforms; metrics per category and mode: recall, false-positive rate.
- E2E (Playwright Chromium, extension loaded): fixtures mimicking each platform's markup with
  synthetic spoiler-free stand-in content. **Unsafe-pixel test**: spoiler text uses a sentinel
  colour; every rendered frame is checked (DOM invariant probe in `requestAnimationFrame` registered
  after the engine, plus CDP screencast pixel sampling) for unshielded sentinel nodes or sentinel
  pixels. Navigation races, infinite scroll, rapid DOM updates, virtualised recycling, shadow DOM,
  iframes, SW suspension, reveal/re-blur, keyboard, a11y tree checks.
- Bowser: live run of the built exe against the same fixtures.
- Performance: per-mutation cost, CPU/RAM on an image-heavy feed fixture, CPU-throttled (4×) runs.

---

## 10. Revision 2 — after independent review (Fable 5.2, 2026-11-17)

The review was a design read, not a run; each claim below is to be confirmed by tests. Accepted
changes override earlier sections where they conflict.

### Zero-flash
- Blink's `CharacterData::ParserAppendData` is believed to queue `characterData` records. The rAF
  pass is **not** a per-frame document rescan; it only (a) checks `shadowRoot` on elements inserted
  since the last frame (streamed declarative shadow DOM attaches without a record) and (b) re-checks
  units touched since the last frame. A chunked-response fixture confirms parser behaviour.
- Shielded nodes and their children get `transition: none !important; animation: none !important`
  and the cover is near-opaque from frame 0. Blur is decoration; the opaque tint is the secrecy.
- Direct text nodes of a container are hidden with `-webkit-text-fill-color: transparent` plus
  `user-select: none` on the container, independent of overlay geometry.
- **Context first, content second.** Hotness is computed from URL + adapter at document_start. On
  high-risk hosts, units inside a thread whose root is not yet resolved are held `pending`.
- Quarantine CSS covers `img, picture, video[poster], object, embed, svg image, canvas` and
  elements with inline `background-image` on high-risk hosts.
- Frame-exact pixel test uses `HeadlessExperimental.beginFrame` (chrome-headless-shell) on the
  Bowser-style bundle injected with `addInitScript`; extension-level e2e uses DOM invariants.

### Shield mechanics
- Overlay child remains, but layout changes are acknowledged, minimised and measured: no
  `position:relative` when the container already establishes a containing block; `display:contents`
  containers re-target to the first boxed child set's parent unit; table rows shield per cell.
- Hydration/React removal loop is bounded: after 3 removals of our overlay from the same container,
  fall back to attribute + CSS-only shielding (no child): opaque tint via `box-shadow: inset` +
  child blur + text-fill transparent, still click-to-reveal via a capture listener.
- Reads (`getComputedStyle`) batched before writes; `style` removed from `attributeFilter`.
- Window-level capture listeners stop `mouseover/pointerover/mouseenter`-driven hover cards for
  targets inside shielded units.
- More than 20 visible shields → opaque variant (no backdrop-filter) for performance.

### Bowser
- `AddScriptToExecuteOnDocumentCreated`, main world, registered outside the login gate, for every
  tab (normal, private, guest) and popup; script id retained so settings changes replace it.
- Fail closed only on hot or high-risk hosts; elsewhere an engine error shows a small visible
  "Spoiler protection error" notice and reports it (never silent). Native kill switch:
  `spoiler_protection = off` in settings.txt.
- Title: Bowser holds tab-title updates for tabs whose script is active for a short window and
  displays the latest title (the bundle rewrites risky titles), so the raw title never reaches the
  tab strip or history. Chrome cannot do this: documented.
- Verification matrix: cross-origin iframes, srcdoc/about:blank/blob:/data: frames, sandboxed frames,
  popups, bundle size ordering, private and guest tabs.

### Detection
- Compact-view matches may not cross sentence punctuation (`.` `!` `?` `;` `:` newline, CJK
  full stops). Fuzzy matching only for terms ≥7 letters with the same first letter.
- Text extraction joins text nodes with a separator (never raw `textContent` concatenation).
- CJK/Thai terms use substring matching (no token alignment); the lexicon passes through the same
  normalisation pipeline as page text.
- New evasion classes: censor wildcards (`J*son`, `d!es`), vowel dropping (`jsn`, `lc`), ROT13 and
  reversed text views for franchise/entity terms, nickname/phonetic forms ("6ix", "the sixth one",
  "vice city 2", "new gta", "gta sex"), transliterated and inflected names, role references
  ("female protag", "the protagonists"), usernames/flairs included in item text, one-word-per-reply
  (thread warmth).
- Cold-context spoiler-shape rule: `<entity_weak> (dies|is killed|betrays|is the rat/cop/traitor…)
  … (at the end|ending|final mission|credits)` → shield in Strict when Jason+Lucia co-occur, a
  game-register word is present, the item replies to a shielded item, or the launch-window prior is
  active (high-risk host, within 8 weeks of 2026-11-19). Negative disambiguation list (Voorhees,
  Bourne, Statham, Todd, Momoa, Kelce, Derulo, Mraz, St Lucia, Santa Lucia…).
- Noisy-OR is retained as a convenience but the classifier is documented and tuned as a rule
  table. A real negative corpus (attributed Wikipedia/public-domain text about films, other games
  incl. GTA V and Vice City 2002, Rockstar the company, people named Jason/Lucia, St Lucia travel)
  measures false positives; the synthetic corpus measures recall only.
- Anchor prefilter: one combined pass rejects units with no anchor (franchise, entity, cue, marker,
  wildcard-shaped token) before full classification.
- **Lexicon feed** (phase 2): data-only, Ed25519-signed JSON that can only *add* terms or raise
  weights (it can never loosen protection), fetched daily; no browsing data sent. Bundled lexicon
  remains the offline baseline.

### Sequencing
- v1 (ship first): text engine, shields, media quarantine in hot context / flagged units / high-risk
  hosts with metadata signals, X/Reddit/YouTube/Google/Bing adapters first, then the rest.
- v1.1: OCR (Bowser Windows.Media.Ocr first, Chrome Tesseract offscreen), lexicon feed, perceptual-hash
  blocklist, reply/author warmth.
- Chrome Web Store: a text-first build submitted under a descriptive name with no Rockstar assets.

## 11. Everyday-browsing cost (2026-11-18)

Measured with the Bowser bundle in headless Chromium at 4x CPU throttling (a mid-range laptop), against the same page with
protection off.

- **Lexicon snapshot.** `compile()` ran in every document and every frame before the first text could be judged; cold
  (no JIT yet) it was ~30 ms of main-thread work per document at full speed, ~140 ms at 4x, so a page with ten ad or
  embed frames paid it eleven times (load 0.25 s -> 1.8 s). The build now runs `compile()` once and embeds the encoded
  result after `lexicon.js` (`build/build.mjs` `withSnapshot`, `lexicon.encodeSnapshot` / `setSnapshot`). The page revives
  it (same terms, same key order, inserted in the same order into fresh tries, same Sets and RegExp sources): ~10 ms per
  document at full speed, ~40 ms at 4x. It is used only for this exact `VERSION` with no feed terms, and only after the
  page's own normaliser reproduces the snapshot's sample keys (one per non-ASCII Unicode block in the lexicon), so ICU or
  Unicode differences between Node and the browser fall back to compiling. A feed (`extend`) or any revive failure
  compiles from the word lists as before. `test/unit/lexicon-snapshot.test.mjs` asserts the revived lexicon is deeply
  identical to a fresh compile and that both give the same result for every corpus text.
- **Inert elements.** Adding or removing an element with no child nodes, none of the attributes `extract()` reads, no
  shadow root, and that is not media, a frame, a form field, a link, an SVG/MathML element or a custom element, cannot
  change what any unit says. Such records no longer re-queue the parent (`touchStruct`), which re-read the whole
  container. A removal counts only for an element that was inert when it arrived and has had no record since
  (`inertSeen`), because a removed node's present state says nothing about what it held while attached.
- **rAF safety pass window.** §5.1 step 3 limits the re-check of touched units on the next frame to loading and 2 s after
  a large insertion; the implementation ran it after every batch forever, doubling the cost of pages that change every
  frame (animations, tickers, games). It now runs while `readyState !== "complete"` and for 2 s after load or a
  `BIG_INSERT` batch.

Still open: a container whose own text really changes every frame (text pop-ups appearing and disappearing inside one
small block) is re-read in full each time; `extract()` walks every descendant of a deep unit. An incremental extract
would be the next step if that shows up on real sites.

## 12. App-page cost (2026-11-20, Speedometer 3.1)

Speedometer 3.1 runs each suite in an iframe, so the engine judged every DOM change of TodoMVC, the editors and the charts.
With protection on, Bowser scored 6.53 against 15.13 off. Profiles (CPU profile of the injected bundle in Chromium,
headless, interleaved off / on runs) showed where the time went. Every change below is exact: the same units are judged
on the same text, with the same verdicts, at the same moments (no allowlist, no skipped frame, nothing deferred past the
MutationObserver callback or rAF). The corpus, red-team and e2e suites check that.

- **Leftover-work covers (`coverLeft`).** `S.shield.hasEditor()` is a querySelector over the element's subtree. Past
  `MARK_MAX` every mark is redirected to `<html>`; a page with any text field answers "editor" there, so `<html>` was never
  marked and was searched again for every remaining node (one TodoMVC step: 110k whole-document searches, 700M nodes).
  The answers are now kept for one `coverLeft()` call (nothing it does can create or disable a text field), and once
  `<html>` has been tried past `MARK_MAX` the remaining marks of that call are skipped as the no-ops they are. Nodes no
  longer in the document are not marked (re-rendering lists left thousands of them). The "Checking" covers' themes are read
  before any cover is written, so they no longer force a style recalculation each.
- **Verdict memo.** For a text of at most one classifier window (no time budget applies), `classify.text` is a pure
  function of the text, the context fields it reads and the lexicon version; `decide()` remembers those results per
  document (bounded by characters). The canary and the health check still call the classifier itself.
- **Classifier.** The fuzzy-name scan visits token starts only (once per view, not once per fuzzy term) and tests the
  token-end condition a hit needs before the edit distance. 40-45 % less classifier time on Speedometer's texts; identical
  results on the corpora (test/unit/minify.test.mjs, test/unit/lexicon-snapshot.test.mjs, test/corpus).
- **Unit resolution and extraction.** `resolveUnit()` no longer runs `useGenericItem()`'s bounded text walk when the block
  is the generic item itself (the unit is the same either way: every list row and table row). `extract()` tests the five
  text attributes with one selector match first (only after the start-up canary has seen the captured `matches()` answer
  correctly), skips the editability test for nested blocks a shallow unit skips anyway, and tests the generic media
  selector by local name.
- **SPA navigation.** A URL change re-judged the whole document, inside the page's own `pushState`. A unit's verdict depends
  on the URL only through the adapter, its thread rules, the page heat and the live state; the whole-document rescan now
  runs when one of those changed (content changes arrive as records anyway).
- **Shadow hosts.** The `data-sfw-sr` mark the hook sets a microtask after `attachShadow` re-queued its host for a second
  judgement; when the root is already observed (the same-world hook reported it synchronously), that record is skipped.
- **Big insertions.** Only nodes still in the document count towards `BIG_INSERT` (a list rebuilt a hundred times in one
  task is not a big insertion).
- **Per-document start.** The Bowser bundle's engine text is minified by `build/minify.mjs` (comments and indentation;
  every token and every line break between tokens kept, checked by re-tokenizing; markers native code reads are kept
  verbatim): 522 KB -> 401 KB. The snapshot revive builds terms from per-shape templates, and boot no longer runs the
  health check right after the canary in the same synchronous start.

Measured (Playwright Chromium headless on the development laptop, Speedometer 3.1 pinned revision, 5 iterations, 3
interleaved runs per variant): off 13.8, before 6.9, after 8.9 (the engine's share of measured time went from +99 % to
+53 %). The Complex-DOM TodoMVC suites improved most (Svelte 181 -> 69 ms, Preact 210 -> 76 ms, ES6-Webpack 222 -> 102 ms).

Still open: shadow-DOM apps (Web Components, Lit) and the Complex-DOM TodoMVC pages. `textLen()` counts light-DOM text
only, so ladder and deep-unit caps do not bound containers whose content lives in shadow roots, and their whole app is
re-extracted per ladder level; and the parser-inserted complex DOM (750 tree rows, 770 SVG icons) leaves a backlog that
the budgeted slices are still working through when the first measured step starts. Both would need a change to what a
unit reads (shadow-aware caps) or to the rAF safety pass; they are left for a design decision.

## 13. Boot floor (2026-11-21)

What every document pays before there is anything to protect. No benchmark was run for this round: each change is argued
from the code below and checked by the correctness suites (unit, corpus, e2e, red-team reveal / zero-flash / hostile).

- **Shadow-host mark only across worlds.** `SFW.hooks.install(win, { sameWorld: true })` (Bowser, test host) no longer
  queues the `data-sfw-sr` attribute after every `attachShadow` (a microtask, an attribute mutation, a record for every
  observer of the page and a style invalidation per shadow host). The Chrome MAIN-world bundle installs without the option
  and keeps the mark. The same-world readers of the mark read the hook's map instead: `discoverShadowAndTouch` notes a host
  whose root is in the map (it noted hosts carrying the mark, which the hook set on exactly those hosts), and
  `S.shield.deepAll` walks into a closed root of a built-in element found in the map. The record for the mark was a no-op
  for a same-world runtime (the root is observed synchronously from inside `attachShadow`).
- **Descendant rules only once a mark needs them.** The rules whose rightmost compound is universal
  (`[data-sfw~="shield"] *` with its `::before` / `::after` and `:host()` forms, `[data-sfw-defer] *` and its pseudo-elements,
  `html[data-sfw-page~="shield"] > body *`) moved, unchanged, from `PAGE_CSS` to `S.styles.DESC`. They fill a third
  constructable sheet, adopted between the page sheet and the quarantine sheet, that stays empty until `ensureMark(kind, el)`
  runs: synchronously before the first write of a mark of that kind (`S.shield.apply / reblur / repair` before
  `data-sfw="shield"`, `applyPage` before `data-sfw-page`, `markEl` before `data-sfw-defer`), and as soon as the engine meets
  a `data-sfw` that a page wrote itself (expansion walk, attribute records: `extract()` skips what is under such an element,
  so its rules matter for protection). Once in, they stay for the life of the sheet. No style is computed between
  `ensureMark` and the write. Cascade: an automated comparison of every tie (same property, both `!important`, equal
  specificity, same box, different values) between the round-1 sheet and the new layout finds the same winner for all of
  them; the one tie whose order would have flipped (the fallback label of a shield nested under a shielded shadow host)
  is restated after the `:host()` rule in `DESC.shield`.
- **Page sheet in shadow roots at mark time.** No rule of `PAGE_CSS` matches without one of our marks or overlays, so a
  shadow root gets the page and descendant sheets from `ensureMark` (the mark's own tree scope, and for a shield the
  element's own root, where its `:host()` rules apply and its overlay is appended) instead of when it is first observed.
  Sites with a quarantine sheet still adopt all sheets into every root on sight (the quarantine hides media from the
  start). The check runs at every mark, so a root whose page replaced `adoptedStyleSheets` after ours were adopted gets
  them back before its next mark (before, it never did). When one of our sheets is missing from a scope, all of them are
  appended again in order.
- **Overlay sheet on first use.** `SHIELD_CSS` (9 KB) is parsed when the first overlay is built, with the constructor and
  `replaceSync` captured at load, instead of by every document at start.
- **Event guards and play().** The capture listeners for hover / press / click skip the ancestor search while no `data-sfw`
  mark can exist in the document (`shieldMarks`, set by `ensureMark`). The same-world `play()` hook walks the ancestors only
  while a `data-sfw-hold` that `S.shield` wrote is still in place (`SFW.hooks.noteHold`; the Set methods are captured and
  probed at load, and any failure makes it walk every time again). A hold attribute a page writes itself is honoured only
  while one of ours exists; it never guarded anything the engine decided.
- **Left alone.** The start-up canary, the classifier warm-up, the per-root observer and the rAF pass: moving their cost
  into the first batch would change when the first content can be judged.
