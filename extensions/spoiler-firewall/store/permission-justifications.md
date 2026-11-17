# Permission justifications (Chrome Web Store dashboard, "Privacy practices" tab)

## Single purpose
Hide spoilers for the video game Grand Theft Auto VI on web pages the user visits, with a click-to-reveal cover, using on-device text analysis.

## Permissions requested
Install-time manifest permissions: `["storage", "alarms"]`. No `host_permissions`, no `optional_host_permissions`. Broad page access is requested through content scripts matching `<all_urls>`.
Optional (requested at runtime, only if the user switches on "Scan images for text (local OCR)", and removed again when it is switched off): `optional_permissions: ["offscreen"]`. The extension works fully without it.

### storage
Saves the user's settings (protection on/off, caution level, reduced effects, image scan, term updates on/off) and the last verified term update (a word list with its version, no user data) in `chrome.storage.local`, and a numeric per-tab shield counter in `chrome.storage.session` for the toolbar badge and popup. No page content or URLs are stored.

### alarms
Schedules the check for spoiler term updates every 12 hours (`chrome.alarms`; MV3 service workers cannot keep timers alive). The check downloads one small signed static file (see "Term update feed" below). `alarms` produces no install-time warning. Without it the extension would only check when the browser starts.

### Content scripts on all sites (`<all_urls>`, all frames, document_start)
Spoilers can appear on any website (social media, forums, search results, news comments, embedded players, iframes), and the user cannot know in advance where. To stop a spoiler from ever being visible, protection must start before first paint, in every frame, on every site. A narrower host list would leave most of the web unprotected, which defeats the single purpose. A MAIN-world script is needed to observe shadow DOM creation and to refuse autoplay of media held behind a cover; the isolated-world script performs the text analysis. The scripts read page text only to classify it locally. Nothing is transmitted.

### offscreen (optional)
Used only after the user enables image scanning. Tesseract OCR (bundled WebAssembly) needs a document with Web Worker support to run in; the MV3 service worker cannot host it. The extension creates one offscreen document (reason `WORKERS`) on demand and closes it after two idle minutes or when the scan is switched off.

### No host permission is requested for image scanning
Reading an image means downloading the file the page already displays. Content scripts cannot do that themselves (their requests follow the page's CORS rules, and most image CDNs send no CORS headers), but the offscreen document can, because the extension already has the `<all_urls>` content-script match patterns. This was verified on Chromium 151 (test/chrome/ocr-extension.mjs, "EVIDENCE" check): with only `offscreen` granted and no host permission declared anywhere, the offscreen document read a cross-origin image served without CORS headers. So no `host_permissions` / `optional_host_permissions` are declared. Requests are made without cookies or credentials, are limited to 8 MB and to `image/*` responses, and nothing is sent anywhere except to the site that serves the image. The recognised text is returned to the page's content script and is never stored.

### Term update feed: no host permission is requested
The service worker makes one anonymous `fetch()` of `https://raw.githubusercontent.com/grantcharlie1000-tech/Bowser/main/extensions/spoiler-firewall/feed/published/feed.json` (`cache: "no-store"`, `credentials: "omit"`, no referrer, 10 s timeout, 256 KB cap, at startup and every 12 hours; switchable). raw.githubusercontent.com answers with `Access-Control-Allow-Origin: *` (checked with `curl -I`), so a cross-origin read from the extension needs no host permission, and none is declared; test/chrome/feed-extension.mjs proves the fetch works with only `storage` and `alarms` against a server that sends the same CORS header. The response is data only: a JSON envelope carrying a word list and an Ed25519 signature. It is verified in the service worker against a public key packaged in the extension (`feed-config.js`, mirrored in `feed/public-key.txt`) using the vendored @noble/ed25519 3.2.0 (MIT; SHA-256 pinned in `build/validate.mjs`). Unsigned, altered, rolled-back (lower sequence number) or oversize files are ignored. The word list can only add terms or raise a term's category (`SFW.lexicon.extend`); it can never remove or weaken anything, and it is never executed. So this is not remote code: the extension's behaviour does not change except that more text matches the same built-in rules. This closes the gap between a game launch and Web Store review latency for new mission and character names. No user data is sent.

### Permissions deliberately not requested
`tabs`, `scripting`, `activeTab`, `webNavigation`, `declarativeNetRequest`, `history`, `cookies`, install-time `host_permissions`. As a result, tabs open before installation are not protected until reloaded (documented in the options page and popup).

## Remote code
No. The one thing downloaded after install is the signed, data-only term list described above; it is parsed as JSON, verified, and passed to the extension's own matcher as plain strings. All JavaScript and WebAssembly is packaged in the extension (Tesseract.js 7.0.0 + tesseract.js-core 7.0.0, Apache-2.0, with seven language models, all under `vendor/`; every file is SHA-256 pinned by `build/validate.mjs`). Extension pages use `script-src 'self' 'wasm-unsafe-eval'`, which allows packaged WebAssembly only; `unsafe-eval` is not granted. Nothing is fetched as code from a network (`corePath`, `workerPath` and `langPath` point at `chrome.runtime.getURL(...)`). Our own code has no eval / new Function; the vendored bundles contain two dead-code `Function(...)` fallbacks that are documented in vendor/README.md.

## Data usage disclosures
See submission-checklist.md.
