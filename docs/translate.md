# Page translation

On-device translation of web pages with Mozilla's Bergamot engine (the one inside Firefox Translations). Page text is translated on this machine and never sent anywhere. There is no API, no key and no server of Bowser's.

## What the user sees

- A translate button in the toolbar (HTML chrome) appears when the page says it is in another language and a pinned language pair covers it. The main menu has **Translate page to English** (or **Show original**), and **More tools** has **Always translate this site** and **Never offer to translate this site**. The command palette has **Translate page**.
- The first time a pair is needed its files are downloaded (about 26 to 33 MB per direction), with a toast that says so. In a private tab Bowser asks first.
- The page turns into the target language in place, nearest text first. **Show original** puts every original string back exactly.
- Settings (Appearance > Translation): offer on or off (default on), translate automatically on sites set to "always" (default off), and the target language (default: the Windows display language).

## What it does and does not do

Nothing runs until the user asks, except one pure decision per toolbar refresh and, only for a site set to "always translate" with the automatic setting on **and the files already on disk**, a start after the reader probe. Nothing is downloaded unasked. No script is added at document creation (`AddScriptToExecuteOnDocumentCreated` is unused), there is no `MutationObserver` (text a page adds after translation is not translated; show the original and translate again), and a tab that is not being translated pays one language tag in the reader probe it already runs (`r2|...|lang`).

Pages with no `lang` are never offered, and the command says so (there is no language detector in v1; Bergamot ships none). `<html lang>` or `<meta http-equiv=content-language>` is used, reduced to 2 or 3 letters.

## Design

```
 tab page  <-- ExecuteScript (translate.js: collect / apply / restore) -->  Rust (src/app/page_translate.rs)
                                                                               |  PostWebMessageAsJson / WebMessageReceived
                                                                     hidden worker WebView (worker.html + worker.js + Bergamot wasm)
                                                                               ^ files served from the cache folder at https://bowser-translate.invalid/
```

1. **Files** (`src/translate/store.rs`, `manifest.rs`). The manifest pins every file: URL, exact size and SHA-256. A background thread fetches (`updater::http::get`, HTTPS only, at most the pinned size) to a `.part`, checks size and hash, and only then renames it into place, so a file at its final name is a verified one. A file placed by hand in the cache folder is accepted after the same check. The cache is `%LOCALAPPDATA%\Bowser\translate-v1\` (shared by profiles), or inside the guest profile for a guest session.
2. **Worker page** (`src/engine/translate_worker.rs`). Created only on a translate command, hidden, in its own in-memory profile (`BowserTranslate`), in the tab environment but with no tab blocker and no extension. It loads exactly `https://bowser-translate.invalid/worker.html`, a virtual host mapped to the cache folder (`DENY_CORS`). Its content policy is `default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; base-uri 'none'; form-action 'none'`, so it can read that folder and nothing else. As a second wall, every navigation that is not the worker page, every frame navigation, new window, permission request, download and external launch is refused, and every request not for the virtual host is answered 403. Its only channel is the web-message bridge: messages are accepted only from that host's page, size-capped, and parsed strictly by `translate::parse_worker_message`. The `bowser-ui` policy is unchanged. It is closed after 120 s idle and when the app closes. The engine runs in the page's main thread (a hidden renderer of its own), not in a Worker, because a Worker script does not inherit the page's meta policy.
3. **Page script** (`src/translate/translate.js`). On demand through `run_translate_script` (same http/https gate as reader view, re-read from the live address at the call). `collect` walks text nodes (skipping `script`, `style`, `code`, `pre`, `kbd`, `samp`, `var`, form fields, `svg`, `math`, anything under `translate="no"`, `.notranslate` or `contenteditable`, numbers and URLs) plus `title`, `alt`, `placeholder` and `aria-label`, viewport first, and returns the first batch (at most 300 strings and about 16,000 characters). `apply` writes a translated batch back (leading and trailing space kept), only into nodes the page has not changed since, and returns the next batch. `restore` puts the originals back, again only where the page has not changed the text.
4. **Rust** (`src/app/page_translate.rs`). One job at a time, driven by completions: files, worker ready, models loaded, collect, translate, apply, repeat. Every step checks the tab's translate epoch (bumped on every navigation, like reader view), so a navigation, a reload or a closed tab drops the job silently. A watchdog fails a step that never answers (90 s). Everything that crosses a boundary is validated: `translate::parse_reply` (the page's answer: exact members, token `[a-z0-9]{1,16}`, bounded batch), `parse_batch`, `parse_worker_message`, and the worker's answer must have exactly as many strings as were sent. Strings sent anywhere are escaped with `json_string` (which also escapes `<`, U+2028 and U+2029).

## Privacy

- Page text goes only from the page to Bowser's process to the local worker page and back. The worker page's policy and Bowser's request filter leave it no way to reach the network (tested in real Chromium: `ui/test/translate.mjs`).
- Only the pinned file addresses on `github.com` and `firefox-settings-attachments.cdn.mozilla.net` are ever requested, and only after the user asks. No page address, text or identifier is part of any request.
- Private tabs can translate (the text stays local). The first language-pack download in a private tab asks first. A private tab never stores or reads a per-site "always" or "never" (`translate_set_site` refuses, `should_offer` ignores the stored choice), and the manifest, state and logic hold nothing on disk.
- `translate_auto` is off by default, and an automatic start never downloads.

## Settings

| Key | Default | Meaning |
|---|---|---|
| `translate_offer` | `true` | Show the toolbar button on a page in another language. It only offers. |
| `translate_auto` | `false` | Translate a site automatically when it is set to "always translate" and its files are already on disk. |
| `translate_target` | empty | The language pages are translated into: a primary subtag (`en`, `fr`), or empty for the Windows display language, read once on first use (`GetUserPreferredUILanguages`) and never at startup. |

Per-site choices live in `site-prefs.txt` as `<origin> translate always` or `<origin> translate never`. Older builds ignore the lines.

## Pinned languages

German, Spanish, French, Italian, Dutch, Polish, Portuguese and Turkish, to and from English, and Russian into English (Mozilla's 1.0 release has no English-to-Russian model). Any pair whose two halves exist goes through English (`fr` to `es` runs `fr-en`, then `en-es`). Targets offered in Settings are English and the languages that have an English-to-language model.

## Sources and licences

- **Engine:** BrowserMT `bergamot-translator` v0.4.5, `bergamot-translator-worker.js` and `.wasm` from the GitHub release `v0.4.5`. MPL-2.0 (the repository licence; Mozilla's Remote Settings record for the same release also says `"license": "MPL-2.0"`). The release publishes no checksums, so the pinned hashes are of the files as downloaded on 2026-10-15; the same files translated a sentence in every pinned pair, including a pivot, in a headless check.
- **Models:** Mozilla's Firefox Translations models, release `1.0`, from the Remote Settings collection `translations-models`, served from `firefox-settings-attachments.cdn.mozilla.net`. Each file's SHA-256 is the record's published `attachment.hash`, confirmed by downloading every file again. The `mozilla/translations` README says "The model files are distributed under the MPL 2.0 license" and `mozilla/firefox-translations-models` carries the MPL-2.0 text. Bowser ships and modifies none of these files; it fetches them unchanged on the user's request. MPL-2.0 is not affected by that, and no pair was left out for its licence.
- The full source list, with the record fields used, is the header of `src/translate/manifest.rs`.

**Availability.** The model files are Mozilla's, on Mozilla's CDN. If a file is ever removed, a copy placed by hand in the cache folder works after the same size and hash check, and the manifest's URLs are the one place to repoint (for example at a mirror in a GitHub release of Bowser's own, which is free for a public repository). Nothing in the app needs a key or an account.

## Where the code is

| File | Role |
|---|---|
| `src/translate/mod.rs` | Pure logic: `normalize_lang`, `should_offer`, `pair_route`, `TabState`/epoch, `Pill`, `invocation`, strict parsers, worker messages |
| `src/translate/json.rs` | Strict JSON reader for page and worker answers |
| `src/translate/manifest.rs` | The pinned files, sources and licence record |
| `src/translate/store.rs` | Cache folder, verified download, atomic publish |
| `src/translate/translate.js`, `worker.html`, `worker.js` | Page script, worker page and its policy, worker script (all `include_str!`; no wasm or model is ever embedded) |
| `src/engine/translate_worker.rs`, `webview2.rs` | The hidden worker host and `run_translate_script` |
| `src/app/page_translate.rs` | Job state machine, offer, per-site choice, idle and watchdog |
| `src/win32/window.rs` | Menu rows (ids 5048 to 5050), palette command, toolbar command, toasts, the private-tab prompt |
| `src/settings.rs`, `src/site_prefs.rs` | Settings and per-site choice |
| `ui/src/view.ts`, `protocol.ts`, `settings-page.ts` | Toolbar button, `state.translate`, `open` target `translate`, settings rows |

## Tests

`cargo test --lib -- translate:: app::tests::translate_flow settings:: site_prefs:: reader::` (logic, manifest, store with fake downloads and hashes, the whole job against a scripted engine, stale epochs, private-tab rules). `ui/test/translate.mjs` runs the page script and the reader probe in headless Edge, and, when `BOWSER_TRANSLATE_CACHE` points at a folder laid out like the cache, loads the real worker page with the real engine and models (direct and pivot translation, bad requests, and that no request leaves for another origin). `ui/test/translate-ui.mjs` covers the toolbar and settings page.

## Known limits

- No language detection: a page without `lang` cannot be translated.
- Plain text per node (no HTML-aware translation), so a sentence split across inline tags is translated in pieces.
- Single-page apps that re-render are not followed (no observer).
- The WebAssembly gemm is the portable fallback, so long pages take a while; the throughput on real hardware has not been measured.
- The worker page's virtual host (`bowser-translate.invalid`) and the hidden controller were written to the documented WebView2 behaviour but have not been exercised in a live Bowser window (no GUI test was run).
