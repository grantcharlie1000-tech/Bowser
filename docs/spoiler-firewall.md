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
  stopped and hidden. They have text-only protection.
- Pop-ups open as ordinary Bowser tabs through the same creation path.
- Extensions list (Ctrl+Shift+E): a first entry "Spoiler Firewall for GTA VI - built in (on, strict)" with its
  on/off check; it has no record and no remove entry, and choosing it opens Settings, Privacy and security.
- Debug builds only: `BOWSER_SPOILER_BUNDLE=<file>` swaps the injected bundle (live testing without a rebuild).
  Release builds do not contain this path. The legacy OCR test-host override cannot activate image scanning.

## Text-only post and video detection (21 November 2026)

Bowser's built-in Spoiler Firewall checks **written webpage content**: posts, comments, thread context,
headlines, video titles, descriptions, visible captions/live chat, image alt text and available page
metadata. The shared classifier and signed spoiler term updates are unchanged.

**No image pixels are inspected.** The old on-device OCR pipeline and perceptual matcher against
Rockstar screenshots are retired. Bowser no longer subscribes to image response bodies, decodes them,
calculates visual fingerprints or sends images to OCR workers. The fingerprint catalogue was removed.
Images and videos may still be *covered* when the written text or page/thread context triggers a
spoiler verdict; covering media is not the same as inspecting its pixels.

The legacy `spoiler_ocr` setting is accepted only to migrate old profiles: it is always forced off,
and saving settings writes `spoiler_ocr=off`. The UI no longer exposes image scanning. The per-tab
hook and credentials remain because **signed term updates** use them to update already-open tabs;
their image scanning branch is disabled. Existing Rust image-adapter code is retained for now but
cannot be entered through Bowser's settings or native WebView2 image-response subscriptions.

On YouTube, Bowser classifies titles/descriptions and visible caption or chat text when accessible;
it does not watch video frames, transcribe audio or fetch full video transcripts. Standalone Chrome
extension OCR is a separate, optional feature unaffected by this Bowser-only change. An image-only
spoiler with neutral surrounding text may get through.

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
- **Image documents.** A top-level image on a high-risk host (`https://i.imgur.com/x.png`) uses the body as its
  filtering unit. Bowser checks available title, URL and text context; it does not inspect the image pixels or hold it
  awaiting OCR. An image-only spoiler with neutral metadata may remain visible, including in Strict.
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

Main-world script (a page can observe or tamper with it); image-only spoilers; code words; sandboxed frames
if the runtime withholds the script; other apps; the native
(GDI) settings window has no spoiler controls (it keeps the saved values; use the settings page or settings.txt).
A focused address bar shows the full URL on purpose.
