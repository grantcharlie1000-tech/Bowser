# Spoiler Firewall for GTA VI in Bowser

Built in, on by default, unofficial. The detection engine is shared with the Chrome extension
(`extensions/spoiler-firewall/DESIGN.md`); this page covers how Bowser hosts it.

## Architecture

- `src/spoiler.rs`: settings model (`spoiler_protection`, `spoiler_mode`, `spoiler_reduced_effects`, `spoiler_ocr`, `spoiler_feed`),
  config JSON, script builder, the process-wide live setting, and the embedded bundle
  (`include_str!` of `extensions/spoiler-firewall/dist/bowser/spoiler-firewall.js`).
- The bundle holds exactly one `/*__SFW_CONFIG__*/null`. Bowser replaces it with
  `{"enabled":true,"mode":"strict","reducedEffects":false,"version":"<bundle version>","private":<bool>}` per tab
  (strings escaped, U+2028/2029 included). A bundle without exactly one placeholder is injected unmodified (the
  engine defaults to enabled and Strict) and a unit test fails the build.
- `src/engine/webview2.rs` `configure_controller` registers the script with
  `AddScriptToExecuteOnDocumentCreated` for every tab (normal, private, guest, pop-ups), outside the login-script
  gate, fire-and-forget (never waits inside the controller creation callback). The returned id is kept per tab
  with an epoch, so a changed setting removes the old script and adds a new one; a registration that completes
  after a newer request is removed when it completes. Setting off registers nothing (the native kill switch).
  A synchronous registration failure fails tab configuration, so no tab is silently unprotected.
- **Asynchronous registration failure.** The completion handler decides with a pure state machine
  (`spoiler::injection_step`): a failed registration is retried once; if that fails too, the tab enters a
  protective state (`protect_tab`): the navigation is stopped and Bowser's own page
  (`spoiler::protective_page_html`: "Spoiler protection could not start in this tab", host only, a Reload link, no
  script) replaces the web page, and `spoiler_protection_failed` is logged. While the tab is in that state every
  navigation (the Reload link, a typed address, the reload button) is cancelled, the registration is retried first and
  the tab goes on to the requested address only once it holds (`spoiler::protective_navigation`). Nothing waits on the
  completion (never on the UI thread). Honest limit: a failure that is reported after the first document already started
  rendering can show that page for the moment it takes the completion to arrive before Bowser stops it; registrations
  are ordered ahead of the first navigation, so in practice the first document is not loaded before the verdict.
- Settings changes (settings page, native settings window, reset) go through `App::apply_live_settings`, which
  publishes the setting and calls `EngineAdapter::refresh_spoiler_scripts` on every live tab. Pages already
  showing keep the script they loaded with until they reload; the settings page says so.
- Not injected: the HTML chrome views (separate chrome-host environment, never `configure_controller`) and
  internal pages (`bowser://settings` and friends have no content WebView2; the engine never sees `bowser:`).
  Pinned web apps (side panels) are injected at creation and re-registered like tabs whenever the setting or the term
  update changes (`engine::web_app::refresh_spoiler_scripts`); a web app view whose registration fails after its retry is
  stopped and hidden. They have no OCR hooks (text-only protection).
- Pop-ups open as ordinary Bowser tabs through the same creation path.
- Extensions list (Ctrl+Shift+E): a first entry "Spoiler Firewall for GTA VI - built in (on, strict)" with its
  on/off check; it has no record and no remove entry, and choosing it opens Settings, Privacy and security.
- Debug builds only: `BOWSER_SPOILER_BUNDLE=<file>` swaps the injected bundle (live testing without a rebuild).
  Release builds do not contain this path. Likewise `BOWSER_SPOILER_OCR_TEST_HOST=<host>` (debug only) makes one
  host count as a listed site for OCR, so local fixtures can exercise it.

## Native OCR (images with text)

Screenshots of spoilers carry their text in pixels. With `spoiler_ocr=on` (default; settings page "Scan images
for text (on-device)") Bowser reads text in images of high-risk sites with the Windows on-device OCR engine and
hands it to the engine, which classifies it like any other text. Code: `src/spoiler_ocr.rs` (policy, parsing,
cache, worker, WinRT) and `src/engine/webview2/ocr.rs` (per-tab events and pump).

**There is no web message channel** (`IsWebMessageEnabled` stays off for this feature and the login handler is
untouched). Native *pulls* with `ExecuteScript`:

1. The config JSON carries `"ocr":<bool>,"ocrHook":"__sfw_<128-bit hex>","ocrKey":"<128-bit hex>"` (the hook and key are present whenever protection is on and the hooks were installed, even with OCR off, because live term updates use the same hook; per tab; the
   hook is created before the script is registered and kept for the tab's life, so re-registration after a
   settings change keeps pages loaded earlier answerable). `bowser-host.js` defines that one property on
   `window` (non-enumerable, non-writable, non-configurable, frozen `{drain, deliver, feed}`) and implements the shared
   host contract `host.ocr = {request(req) -> Promise<{status:"ok",text}|{status:"unavailable"|"error"|"skipped"}>}`.
   Requests are absolute http(s) only (data:/blob: answer `unavailable` at once), capped at 256 pending, and
   settle `error` after 30 s (the runtime shows "Couldn't check image", never auto-reveals). Both hook calls take
   the key, so a page that lists its own property names still cannot drain requests or forge answers (a
   hardening over the first design, which had no key).
2. A tab with protection and OCR on subscribes `WebResourceResponseReceived`. An image response (content type
   `image/*` except SVG, status 200, GET, Content-Length 256 bytes to 5 MB, or unknown and capped while reading)
   whose host is on the allowlist while the tab's top-level site is on it too is read with `GetContent` (the
   IStream is copied on the UI thread) and parked in memory, unscanned (`ocr::Parked`: 600 bodies / 64 MB per
   cache, oldest out first). It is queued for a worker only when the page asks about that URL (a drain names it,
   or a held request is already waiting for it): the page asks only for media near the viewport whose post is not
   already decided by its words, so most images a feed loads are never scanned. Scanning every image response
   kept a core busy for as long as a feed was scrolled. Known-small URLs (X avatars in their `_normal` / `_mini` /
   `_bigger` variants, emoji, hashflags) are not read at all and answer `skipped`. The response event needs no resource filter and never
   holds a request, so the tab adds none: an image filter makes every matching request wait for Bowser's UI thread
   (the blocker's `WebResourceRequested` handler), and with filters for YouTube, ytimg, twimg and the other listed hosts
   that made the whole browser crawl. Only if a navigation to a listed site completes without one response event (a
   runtime that raises it for filtered requests only) does the tab fall back to image filters for the listed hosts.
   Native never fetches anything.
3. The workers (one thread, two on a machine with 8 or more hardware threads, below-normal priority, sharing one queue; MTA, queue of 64 jobs / 48 MB with the oldest dropped first,
   jobs of a previous document dropped by a per-tab navigation epoch) decodes with `BitmapDecoder`, skips images
   under 96x64 and animated or multi-frame images (`skipped`), scales a short edge under 300 px up 2x, scales a
   width above the engine limit down, cuts very tall images into overlapping 4096 px bands (at most 8) and runs
   `Windows.Media.Ocr.OcrEngine` (user-profile languages, falling back to en-US). Where a small image (at most 2
   bands, 2.5 megapixels) gives fewer than 12 letters or digits, up to two more installed engines from ja, zh, ko,
   ru are tried and the richer text is kept. No engine at all: OCR is unavailable for the session and every
   request answers `unavailable`.
4. The pump drains the top frame and every tracked child frame (`FrameCreated`, nested `FrameCreated`, frame
   `ExecuteScript`) only while the tab is visible: on DOMContentLoaded and NavigationCompleted, 150 ms after image
   responses (debounced), on becoming visible, then every 300 ms for two empty rounds or while a frame still
   returned requests. No timer runs for a hidden tab or a page off the allowlist. The drain result is hostile: it
   is parsed strictly (one level of string wrapping, depth 32, at most 64 items, ids of at most 64 safe
   characters, URLs of at most 2048 characters, http(s) only, no credentials) and anything else is dropped.
5. A request is served only when the asking frame's site group and the image host's group are the same platform
   (x/twitter/twimg, reddit/redditmedia/redd.it, instagram/facebook/fbcdn/threads, tiktok, bsky.app,
   youtube/ytimg, twitch/jtvnw, discord/discordapp, imgur, 4chan/4cdn, gtaforums, resetera, telegram, kick, tumblr,
   steamcommunity + steamuserimages, rockstargames.com, games press (IGN, GameSpot, Kotaku, Polygon, Eurogamer, GamesRadar,
   PC Gamer, VG247, Push Square, Pure Xbox, GameFAQs and their CDNs) and fandom/wikia), so Bowser is never a cross-origin image reader elsewhere.
   The frame's group comes from native navigation events, never from what the page says. A cached answer is
   delivered at once; an image still being scanned is delivered when done; an image not seen yet is held 2 s (its
   response normally precedes the request) and then answered `unavailable`. Deliveries are JSON inside a
   JavaScript string literal with U+2028/2029 and `</` escaped, at most 192 KB per call, text at most 4096
   UTF-16 units.

**Cache and privacy.** An in-memory LRU of 2000 entries per cache, keyed by (top-level site group, URL); private
tabs have their own, cleared when the last private tab closes. OCR text is never written to disk and never
logged (the policy log carries event names only). Everything stays on the machine.

**Limits (honest).**
- Bowser checks X photos and eligible YouTube thumbnails against a local catalogue of Rockstar screenshot
  and video thumbnail fingerprints. An X image that does not match continues through OCR when available.
  YouTube's per-card metadata remains the fallback when its visual check is unavailable or takes longer
  than 1.5 seconds after dispatch. The catalogue detects close copies of known images. Original player
  captures, heavy crops and novel gameplay scenes can pass when their surrounding text is clear.
- OCR needs a Windows OCR language. English is installed with most Windows editions; others are optional
  (Settings, Time and language, Language and region). Without the pack for the text's language, recognition of
  that text is poor or empty.
- WebP and AVIF decode only if Windows has the codec (AVIF needs the AV1 extension; WebP support depends on the
  Windows build). A failed decode is cached as `unavailable` for that URL. No heavier decoder is bundled.
- Animated images are not scanned (`skipped`). On a high-risk host in Strict and Lockdown a `skipped` image stays held
  with the "Couldn't check image · Click to reveal" cover (only Balanced releases it; before, Strict released it).
  Local `data:`/`blob:` images the engine never sends are the exception and keep the older rule. Video frames are not scanned.
- A redirected image is cached under the URL the response event reports, which may differ from the `src` the page
  asks about; that request answers `unavailable` after the grace. Whether an image served from the browser's HTTP
  cache raises the response event is unverified (live check pending); if it does not, such an image answers
  `unavailable` until it is fetched again.
- Only sites on the allowlist are scanned; text in images on other sites is not read.
- Stylised or low-contrast text, heavily compressed screenshots and handwriting may not be read.

## Signed term updates

The bundled term list cannot know mission names or late-game characters before the game is out, and a Bowser
release is slower than the news. `src/spoiler_feed.rs` closes that gap with a data-only, signed feed that can only
**add** terms or **raise** a term's weight; it is never code and can never loosen protection. Format, rules and
the maintainer workflow: `extensions/spoiler-firewall/feed/schema.md` and `feed/README.md`. The Chrome extension
downloads the same file (`hosts/chrome/feed*.js`).

- **Fetch.** One anonymous HTTPS GET of `spoiler_feed::FEED_URL` through the updater's WinHTTP code
  (`updater::http`: no cookies, no referrer, HTTPS-only redirects, 10 s timeouts, 256 KiB cap) on a
  `bowser-spoiler-feed` thread: about 5 s after startup, then every 12 hours while running, and immediately when
  `spoiler_feed` is switched on. Never on the UI thread, never in benchmark launches. The request's User-Agent is
  `Bowser-Updater/<version>`; the host also sees the IP address, as with any request. 404 is quiet (nothing is
  published until the maintainer commits `feed/published/feed.json`).
- **Verify.** Ed25519 with `ed25519-compact` (pure Rust, no dependencies of its own; Windows CNG has no Ed25519
  before Windows 11, and the updater's P-256 does not match the extension's WebCrypto/noble choice) against
  `feed/public-key.txt` (compiled in; any listed key may have signed, which is how keys rotate). Order: size, envelope,
  signature, payload, schema, rollback. A strict JSON reader parses the payload only after the signature verified.
  Unsigned, altered, rolled-back, malformed or oversize files are ignored with the status "Term update rejected".
- **Policy.** Expired feeds are still applied (they only add protection) and flagged "Term updates out of date.". A
  lower `seq` than the stored one is a rollback and is refused; the same `seq` is a no-op.
- **Store.** The last good envelope is kept verbatim beside settings.txt as `spoiler-feed.json` (no browsing data) and
  re-verified on every load, so a tampered file is ignored. Loaded synchronously at startup, before the first tab.
- **Apply.** `spoiler::script_for` embeds the verified `lexicon` object as the last config member,
  `"feed":{...}` (re-serialised with the same U+2028/2029 escaping as every other string, size-checked, and dropped
  if it is not a balanced object). `bowser-host.js` calls `SFW.lexicon.extend(config.feed)` before
  `SFW.runtime.start`; `extend` only adds or raises, validates every term and ignores junk. A newly verified feed
  posts `WM_SPOILER_FEED` to the main window, which re-registers the script in every live tab and pinned web app, sends the
  same lexicon to every chrome view (`docs/spoiler-firewall-ui.md`) and pushes it into documents that are already open:
  `ExecuteScript` calls the per-tab hook's `feed(key, json)` in the top frame and every tracked child frame
  (`TabOcr::push_feed`); the bundle runs `SFW.lexicon.extend` and the runtime's config-change path, which rescans when the
  lexicon version changed. The same feed is never applied twice in one document. Pages that cannot be reached (no hooks,
  a frame that is not tracked yet) pick the feed up on their next load. A typical feed adds a few hundred bytes to the per-tab script;
  the hard cap is 5000 terms (about 200 KB of JSON), only reachable by an unusually large feed.
- **Setting.** `spoiler_feed=on|off` (default on; only off, false or 0 turn it off), settings page "Download spoiler
  term updates" under Privacy and security, with a read-only status line ("Term list: built-in 2026.11.17-1 +
  update #3 (2026-11-18). ..."). Off stops downloads only: an update already held stays in force. Offline or with no
  published feed, the bundled lexicon works exactly as before.
- **Tests.** `cargo test spoiler_feed` (verification incl. a feed signed by the Node tooling, rollback, caps,
  expiry, strict JSON, store reload and tamper, config embedding) and the Node suites `feed-*.test.mjs`.

## Title handling

Bowser reads `DocumentTitleChanged` and feeds the tab strip, window caption and (through the tab title) history.
While protection is on, a title change starts a 150 ms hold (`TITLE_HOLD_MS`, one `SetTimer` per tab, not reset by
further changes so a title that keeps changing is not starved); when it fires Bowser reads the title again and
delivers that. The bundle rewrites a risky `document.title` to "Spoiler-protected page" in the page, so the
rewritten title is what reaches the strip and history. Best effort: a page whose engine has not rewritten the
title within 150 ms shows its own title, and history first records the URL as the title for a page whose title
has not arrived (the existing placeholder rule) and then the held title. An empty title, one equal to the page URL, or one that looks like a URL (`spoiler::protected_title`) is delivered as the
host only (`example.com`), also in the window caption and taskbar text, so a slug never reaches them. The address bar is
covered separately (`docs/spoiler-firewall-ui.md`). The only cost is titles appearing 150 ms later while protection is on.
Chrome cannot do this.

## Web notifications

A page's notification shows its title and body outside the page, where the engine cannot reach them, and Bowser has no
Rust classifier (the engine is not ported). While protection is on, every tab handles `NotificationReceived`
(`ICoreWebView2_24`, available in webview2-com 0.39.1), marks it Handled and shows nothing (`spoiler_notification_suppressed`
in the policy log); Bowser has no toast surface that could show a host-only line, so suppression is the whole behaviour. With
protection off the default WebView2 notification UI is untouched. Notification permission prompts and stored allows
are unchanged. Cost: while protection is on, web notifications from sites never appear.

## Other surfaces closed in the audit

- **Cards with pictures.** A small card (article, li, figure, blockquote, listitem: at most 8 element children and 1500
  characters of text) that holds a shielded block is covered whole in the same evaluation pass, so a spoiler's picture
  (an `img` or a CSS `background-image`), links and captions are covered with it; a generic card with an inline or
  computed `background-image` counts as media. A long article of many paragraphs is still shielded per paragraph.
- **Image documents.** A top-level image on a high-risk host (`https://i.imgur.com/x.png`) is held and read like any
  other media of that host: the body is the unit, spoiler text covers it, a clean read releases it, an unreadable one stays
  covered in Strict.
- **More high-risk adapters:** imgur, 4chan, GTAForums, ResetEra, Telegram Web, Kick, Tumblr, Steam Community,
  rockstargames.com, games press and guides (IGN, GameSpot, Kotaku, Polygon, Eurogamer, GamesRadar, PC Gamer, VG247,
  Push Square, Pure Xbox, GameFAQs and similar) and Fandom wikis (`gta.fandom.com` is hot for pages naming GTA VI and
  at least warm for every other page; Wikipedia's `/wiki/Grand_Theft_Auto_VI` is hot by the URL rules).
- **Cover theme.** The cover follows the page: light on a light background, dark on a dark one (relative luminance of the
  first non-transparent background behind the unit, else a single-scheme `color-scheme`, else the OS scheme), through
  `data-sfw-theme` set while the cover is built, so there is no flip after the first paint.
- **Reader view** no longer makes a shielded unit visible (`reader.css` does not apply its visibility rule to shielded
  nodes, and `reader.js` ignores paragraphs inside shielded units when it picks the article).
- **Start page web page widget** is paused behind "Web page widget paused by spoiler protection · Load" while protection
  is on (see `docs/spoiler-firewall-ui.md`).

## Checks

- `cargo test spoiler`: the settings model (defaults on and Strict, missing keys, garbage never turns it off), the
  substitution, and that the embedded bundle is present with exactly one placeholder.
- `settings`/`chrome_host::settings` tests cover the keys and the optional, exact `save` payload.
- Before a release: `node extensions/spoiler-firewall/build/build.mjs --check`.

## Verification matrix (live)

Live runs are scripted in `extensions/spoiler-firewall/test/bowser/` (`live-run.mjs`, `serve.mjs`,
`probe-bundle.js`, `win.ps1`, fixtures). **None has been run yet (deferred: the machine was in use).** Bowser
cannot be driven over CDP (it refuses to start when `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` is set), so the run
uses beacons from a probe bundle plus window screenshots. The probe proves the injection mechanism, config
substitution and frame coverage; the real engine still needs the same fixtures run with `--bundle real`.

| Case | Status |
|---|---|
| top document shielded | not run yet (deferred: user busy) |
| cross-origin iframe (localhost vs 127.0.0.1, out of process) | not run yet (deferred: user busy) |
| srcdoc iframe | not run yet (deferred: user busy) |
| sandboxed iframe without allow-scripts | not run yet (deferred: user busy); expected to be NOT covered if Chromium withholds script there |
| about:blank iframe written by the parent | not run yet (deferred: user busy) |
| blob: and data: iframes | not run yet (deferred: user busy) |
| private tab (`"private":true` in config) | not run yet (deferred: user busy) |
| guest window | not run yet (deferred: user busy) |
| pop-up tab (target=_blank) | not run yet (deferred: user busy) |
| title held and rewritten in tab strip and caption | not run yet (deferred: user busy) |
| settings change reaching a reloaded page | not run yet (deferred: user busy) |
| bundle size and script-ordering behaviour with the full runtime | not run yet; needs the runtime bundle |

## Known limits

Main-world script (a page can observe or tamper with it); images with no text; code words; sandboxed frames
if the runtime withholds the script; other apps; the native
(GDI) settings window has no spoiler controls (it keeps the saved values; use the settings page or settings.txt).
A focused address bar shows the full URL on purpose.
