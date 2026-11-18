# Limitations and security assessment

Spoiler Firewall for GTA VI (unofficial). Status as of 2026-11-17, before launch on 2026-11-19.
This document is kept honest: every number below comes from a test in this repository, and every
"not verified" means exactly that.

## What is measured

| Area | Result | Where |
|---|---|---|
| Synthetic spoiler recall, Strict | 100% (held-out templates 100%) | `test/corpus/run-corpus.mjs` |
| Independent red-team evasions, Strict (1,420 plausible) | 98.8% (17 misses, listed below) | `test/redteam/` text suite |
| False positives on real text (29,107 Wikipedia units, 6 languages), Strict | 0.02% | corpus gate |
| False positives on ordinary pages (1,132 units), Strict / Lockdown | 0.18% / 1.68% | red-team FP storm |
| Unsafe frames (frame-exact `beginFrame` capture), shared fixtures | 0 | `test/e2e/zero-flash.mjs` |
| Unsafe frames, red-team "poster-deliverable" fixtures | 0 of 77 fixtures | red-team zero-flash suite |
| Reveal semantics (one item only, re-blur, keyboard, a11y, find, copy) | 14/14 red-team, 16/16 interaction | `test/e2e/interaction.mjs` |
| Real Chromium with the packaged extension | 23/23 load, 27/27 OCR, 20/20 feed | `test/chrome/` |

Synthetic and red-team corpora were written without knowledge of the real plot (it is not public);
they measure robustness to *forms* of spoilers, not knowledge of story content.

## Known detection gaps

1. **Unknown plot vocabulary.** Mission names, late-game characters and locations revealed at
   launch are not in the built-in list. The signed term feed (`feed/`) exists to add them, but a
   maintainer must publish updates; until then, spoilers that name only unknown entities and no
   GTA/game words in a cold context can pass.
2. **Cold-context spoilers without any GTA signal** ("she dies at the end" in an unrelated thread)
   cannot be attributed to GTA VI. Strict catches spoiler-shaped sentences with game vocabulary
   during the launch window; plain prose without any anchor is missed by design.
3. **Red-team misses kept on purpose** (false-positive cost too high): emoji-only codes with no GTA
   token (`🚗👑6`), mid-word split cues (`end ing sp oiler`), glued strings (`gtavifinalmission`),
   weak-name-only Spanish sentences outside the launch prior, reply-only fragments ("she lives, he
   doesn't").
4. **Ciphers** (ROT13, reversed, Morse, base64): franchise/entity names are decoded; fully ciphered
   messages with ciphered cues are mostly missed.
5. **Images.** Text in images is read by OCR:
   - Chrome: opt-in (needs the `offscreen` permission); English, Spanish, French, German,
     Portuguese, Italian, Russian. Warm latency ~0.4–0.7 s/image on an unloaded machine, more under load.
   - Bowser: on by default via Windows.Media.Ocr; only languages with an installed Windows OCR
     pack; WebP/AVIF need the Windows codecs; animated images are not read (`skipped`): on high-risk hosts
     they stay covered ("Couldn't check image") in Strict and Lockdown, released only in Balanced.
   - **No visual classifier.** A screenshot of the game with no readable text, in a thread with no
     GTA context, is not recognised. Lockdown keeps such media covered on high-risk sites; Strict
     releases text-free images in cold context after inspection. This is the largest residual risk.
6. **Video.** Titles, descriptions, captions, thumbnails (via OCR) and surrounding text are
   inspected; frames and audio are not. Spoiler-shaped videos are paused, muted and covered.
   Local audio transcription was investigated and not shipped (model size and CPU cost).

## Known rendering/exposure limits

- **Content the engine cannot reach:** closed declarative shadow roots in the main world (Bowser);
  form-control values set by script; CSS-generated `content:`; canvas-drawn text; text inside
  sandboxed frames is judged from the parent (srcdoc text, size and context) rather than read.
- **Browser UI outside the page (Chrome):** the tab title can show the original page title for a
  moment before it is rewritten; the address bar shows URL slugs; hover-link status bubbles for
  links outside covered items; web notifications; history and omnibox suggestions. Chrome
  extensions cannot change these.
- **Browser UI (Bowser):** titles are held 150 ms so the rewritten title is what reaches the tab
  strip, window caption and history (an empty or URL-shaped title becomes the host only); the address
  bar shows the host and "· path hidden" for a risky address until it is focused; web notifications are
  suppressed while protection is on; suggestions, History, panels, palette and tab cards are masked
  by the same engine. Not covered: the native (non-HTML) fallback UI, native "recently closed"/jump
  lists, history titles recorded before this feature, bookmark edit forms.
- **Profile pictures are never checked:** on high-risk sites the images each adapter lists as profile pictures (`avatars`
  in engine/adapters.js) are not quarantined or read by OCR, so they show in a GTA context too. A spoiler image used as
  someone's profile picture shows unless the post it sits in is covered for its words or other media.
- **Tabs open before install (Chrome)** are unprotected until reloaded (no `scripting` permission).
- **Bowser, script registration failure:** if the per-tab script cannot be registered (asynchronously reported), it is
  retried once and then the tab shows Bowser's protective page instead of the web page; it is not verified live.
- **Bowser, pages that never get the bundle:** the start page's web page widget is paused until a click while protection
  is on; reader view cannot reveal a shielded unit. Documents open when a term update arrives are updated through the
  per-tab hook (top frame and tracked frames); anything else gets it on its next load.
- **Page scripts** can always tamper with a main-world script (Bowser) or remove the extension's
  covers (both). The threat model treats posters as adversaries, not the platforms themselves.

## Security assessment

- **No browsing data leaves the device.** No telemetry, accounts or cloud inference. The only
  network request is the optional term feed: a fixed URL fetched every 12 h with no cookies and no
  page data; feeds are Ed25519-signed, can only add terms, and are rejected if older than the last
  accepted one. The signing private key lives outside the repository.
- **Chrome permissions:** `storage`, `alarms` (feed schedule); optional `offscreen` (OCR). Content
  scripts on all sites are inherent to the purpose. No remote code; vendored libraries are
  SHA-pinned by `build/validate.mjs`.
- **Bowser:** the engine runs in the page's main world. Native OCR uses a pull model through
  `ExecuteScript` with a per-tab random hook name and key: no WebMessage channel is opened, the
  login handler is untouched, OCR text is served only to the same high-risk site group that served
  the image, and OCR results are memory-only (never written to disk or logged).
- **Fail-safe behaviour:** classification errors and budget overruns end in a covered state;
  engine start failures show a page cover on hot or high-risk pages and a visible notice elsewhere;
  nothing is revealed by a timer.
- **Not verified live:** the Bowser build has not yet been exercised in a visible window (deferred
  at the user's request); see `docs/spoiler-firewall.md` for the verification matrix.
