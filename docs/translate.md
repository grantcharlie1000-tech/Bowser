# Page translation

Translation of web pages by Google Translate. There is nothing to download, no key and no account: Bowser sends the page's text to Google's public translation endpoint and writes the answer back into the page.

**This sends the page's text to Google.** That is the trade for working at once on 63 languages without a download. Bowser says so where it matters (the menu toast, the Settings rows, and a question before a private tab sends anything) and it only happens on the user's own command.

## What the user sees

- A translate button in the toolbar (HTML chrome) appears when the page says it is in a language other than the target. The main menu has **Translate page to English** (or **Show original**), and **More tools** has **Always translate this site** and **Never offer to translate this site**. The command palette has **Translate page**.
- Clicking it shows "Translating with Google Translate…" and the page turns into the target language in place, nearest text first. **Show original** puts every original string back exactly.
- A page that does not say what language it is in is not offered, but the menu command still works: Google detects the language.
- In a private tab Bowser asks first ("Send this page's text to Google Translate"), every time.
- Settings (Appearance > Translation): offer on or off (default on), translate automatically on sites set to "always" (default off), and the target language (default: the Windows display language, or English when that is not a language in the list).

## What it does and does not do

Nothing runs until the user asks, except one pure decision per toolbar refresh and, only for a site set to "always translate" with the automatic setting on, a start after the reader probe. No script is added at document creation (`AddScriptToExecuteOnDocumentCreated` is unused), there is no `MutationObserver` (text a page adds after translation is not translated; show the original and translate again), and a tab that is not being translated pays one language tag in the reader probe it already runs (`r2|...|lang`).

## Design

```
 tab page  <-- ExecuteScript (translate.js: collect / apply / restore) -->  Rust (src/app/page_translate.rs)
                                                                               |  background thread, one POST per batch
                                                                               v
                                          https://translate.googleapis.com/translate_a/t?client=gtx   (src/translate/google.rs, net.rs)
```

1. **Page script** (`src/translate/translate.js`). On demand through `run_translate_script` (same http/https gate as reader view, re-read from the live address at the call). `collect` walks text nodes (skipping `script`, `style`, `code`, `pre`, `kbd`, `samp`, `var`, form fields, `svg`, `math`, anything under `translate="no"`, `.notranslate` or `contenteditable`, numbers and URLs) plus `title`, `alt`, `placeholder` and `aria-label`, viewport first, and returns the first batch (at most 300 strings and about 16,000 characters). `apply` writes a translated batch back (leading and trailing space kept), only into nodes the page has not changed since, and returns the next batch. `restore` puts the originals back, again only where the page has not changed the text.
2. **Google** (`src/translate/google.rs`, `net.rs`). One form POST per batch: `q=` once per string, `sl` the page's language when it is one in the table (otherwise `auto`), `tl` the target. The answer is a JSON array of strings (a named source) or of `[text, detected language]` pairs (`auto`), in the order sent; anything else, or a different count, is refused. `net.rs` is WinHTTP: HTTPS only, redirects refused, 10 s to connect, 20 s to receive, answers capped at 1 MB. A network failure is tried once more; HTTP 429 is reported as "limiting requests" and not retried. The host `translate.googleapis.com` is the only address translation contacts.
3. **Rust** (`src/app/page_translate.rs`). One job at a time, driven by completions: collect, send, apply, repeat. The request runs on a thread and posts `WM_TRANSLATE_NET` (job id and request id) back; the result waits in a process-wide list keyed by request id. Every step checks the tab's translate epoch (bumped on every navigation, like reader view), so a navigation, a reload or a closed tab drops the job silently. A watchdog (120 s) fails a step that never answers. Everything that crosses a boundary is validated: `translate::parse_reply` (the page's answer: exact members, token `[a-z0-9]{1,16}`, bounded batch), `google::parse_answer`, and the answer must have exactly as many strings as were sent. Strings sent into the page are escaped with `json_string` (which also escapes `<`, U+2028 and U+2029).

## Privacy

- Page text goes from the page to Bowser's process to `translate.googleapis.com` over HTTPS and back. Google can see it, and the connection's address. No page address, cookie or identifier is part of any request, and Bowser sends no key or account.
- It is only ever sent on the user's command, or for a site they chose "always translate" for with the automatic setting on (both off by default). A private tab asks every time and never stores or reads a per-site "always" or "never" (`translate_set_site` refuses, `should_offer` ignores the stored choice).
- Form fields, code blocks, numbers, URLs and anything marked `translate="no"` are never collected.

## Settings

| Key | Default | Meaning |
|---|---|---|
| `translate_offer` | `true` | Show the toolbar button on a page in another language. It only offers. |
| `translate_auto` | `false` | Translate a site automatically when it is set to "always translate". |
| `translate_target` | empty | The language pages are translated into: a primary subtag (`en`, `fr`), or empty for the Windows display language, read once on first use (`GetUserPreferredUILanguages`) and never at startup. A value that is not in the table is replaced by the display language, then English. |

Per-site choices live in `site-prefs.txt` as `<origin> translate always` or `<origin> translate never`. Older builds ignore the lines.

## Languages

The table is `translate::LANGUAGES` (code and English name), 63 languages from English, Spanish and French through Arabic, Hindi, Chinese, Japanese and Korean to Kazakh; it is both the list Settings offers as a target and the set of codes named as a source. A page in any other language is sent with the source left to Google (`auto`). The Settings page keeps its own name map (`LANGUAGE_NAMES` in `ui/src/settings-page-logic.ts`); `ui/test/translate-ui.mjs` fails if a code in the list has no name there.

## Limits and risks

- **The endpoint is not a documented API.** `translate_a/t?client=gtx` is what Google's own web widgets use; it needs no key but carries no promise. It can change, be throttled for a connection that sends a lot (HTTP 429), or be switched off. Bowser then says "Google Translate could not be reached" or "is limiting requests", puts the page back and leaves everything else alone. The official Cloud Translation API needs a key and billing and cannot ship in a public browser without a server of our own.
- Plain text per node (no HTML-aware translation), so a sentence split across inline tags is translated in pieces.
- Single-page apps that re-render are not followed (no observer).
- The source is the page's declared language, or detected per request; a mislabelled page can come back untouched.
- A long page is many sequential requests (a batch is up to about 16,000 characters, one request each, at most 80 batches).

## Where the code is

| File | Role |
|---|---|
| `src/translate/mod.rs` | Pure logic: `normalize_lang`, `LANGUAGES`, `should_offer`, `target_language`, `source_for`, `TabState`/epoch, `Pill`, `invocation`, strict parsers |
| `src/translate/google.rs` | The request (path, form body) and the strict reading of the answer, errors, retry; the live test |
| `src/translate/net.rs` | The one WinHTTP POST |
| `src/translate/json.rs` | Strict JSON reader for the page's and Google's answers |
| `src/translate/translate.js` | Page script (`include_str!`) |
| `src/engine/webview2.rs` | `run_translate_script` |
| `src/app/page_translate.rs` | Job state machine, offer, per-site choice, watchdog |
| `src/win32/window.rs` | Menu rows (ids 5048 to 5050), palette command, toolbar command, toasts, the private-tab prompt |
| `src/settings.rs`, `src/site_prefs.rs` | Settings and per-site choice |
| `ui/src/view.ts`, `protocol.ts`, `settings-page.ts` | Toolbar button, `state.translate`, `open` target `translate`, settings rows |

## Tests

`cargo test --lib -- translate:: app::tests::translate_flow settings:: site_prefs:: chrome_host::settings` covers the logic, the request and answer parsing (with a fake connection), and the whole job against a scripted engine and a scripted Google (stale epochs, watchdog, each failure, private-tab rules, auto start). `cargo test --lib -- --ignored google::tests::live` makes one real request to Google. `ui/test/translate.mjs` runs the page script and the reader probe in headless Edge; `ui/test/translate-ui.mjs` and `ui/test/settings-page.mjs` cover the toolbar and settings page.

## History

The first version translated on the device with Mozilla's Bergamot engine in a hidden worker page, from model files downloaded on first use (about 30 MB per direction). It was removed: the download was slow and silent (the tab looked stuck, and a second click said translation was already working), and it covered nine languages. The worker, the file manifest and the download store are gone; nothing is cached on disk any more.
