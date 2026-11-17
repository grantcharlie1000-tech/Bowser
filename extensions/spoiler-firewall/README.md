# Spoiler Firewall for GTA VI (unofficial)

One detection engine, two distributions: a Manifest V3 Chrome extension and Bowser's built-in protection. Not affiliated with Rockstar Games or Take-Two Interactive. Architecture and threat model: [DESIGN.md](DESIGN.md).

## Install

- Development (unpacked): `node build/build.mjs`, open `chrome://extensions`, enable Developer mode, "Load unpacked", choose `dist/chrome`. Tabs open before install are not protected until reloaded.
- From the Chrome Web Store: not yet available. Submission is a manual step (see `store/submission-checklist.md`); approval is not assumed.
- Bowser: built in, on by default. Nothing to install.

## Commands

| Command | What it does |
|---|---|
| `npm run build` | Builds `dist/chrome`, `dist/node/engine.cjs`, `dist/bowser/spoiler-firewall.js` |
| `npm run check` | Fails if the committed Bowser bundle is stale |
| `npm test` | Build, then unit tests (`node --test test/unit/`), including the Chrome packaging and validator tests |
| `npm run e2e` | Playwright fixture tests of the engine |
| `node test/chrome/load-extension.mjs` | Real-Chromium test of the unpacked extension (headless, muted) |
| `npm run package` | Build, validate `dist/chrome`, write `release/spoiler-firewall-chrome-<version>.zip` and `.sha256` |
| `node store/make-graphics.mjs` | Regenerates icons and the store promo tile |

The real-Chrome test needs `npm install --prefix test/chrome` (installs `playwright-core` locally) and a Playwright Chromium build. Branded Chrome ignores `--load-extension`, so a Playwright-managed Chromium is used. Set `SFW_CHROMIUM` to point at a specific binary and `SFW_EXTENSION_DIR` to test a different unpacked directory. Automation runs headless (`--headless=new`) and muted.

## Image text detection (OCR)

`host.ocr` is the engine's optional media-inspection contract (see DESIGN.md §3 and `engine/runtime.js`, "media OCR gate"). When a host provides it, media on high-risk hosts stays quarantined until the host has read the image; the text found is judged together with the unit's own text. Without `host.ocr` nothing changes.

Chrome host: the switch "Scan images for text (local OCR)" in the popup and settings page requests the optional permission (`chrome.permissions.request`, which needs a click) and sets `ocr: true`; switching it off removes the permission, and revoking it in `chrome://extensions` switches the setting off. The content script sends the image URL to the service worker, which asks the offscreen document (`offscreen.js`) to fetch it (no cookies, 8 MB, `image/*`), decode, scale and run Tesseract. Tests:

| Command | What |
|---|---|
| `node test/e2e/ocr-pipeline.mjs` | runtime pipeline against a fake `host.ocr` (delays, errors, cache, priority, unit shielding, zero unsafe frames) |
| `node test/chrome/ocr-extension.mjs` | the real extension: permissions, accuracy and latency on `test/fixtures/ocr-images`, privacy checks |
| `node test/fixtures/make-ocr-fixtures.mjs` | regenerates the OCR fixture images |

The real-extension test runs the unpatched `dist/chrome`: `offscreen` has no install warning, so `chrome.permissions.request` from a real click is granted headless without a prompt; the test checks the grant, the revoke and the whole pipeline.

## Layout

- `engine/` shared detection and shield code (no `chrome.*`).
- `hosts/chrome/` MV3 wrapper: `manifest.json`, `content.js` (host shim, compiled together with the engine), `service-worker.js`, popup, options, icons. Install-time permissions: `storage` only; page access via `<all_urls>` content scripts. Optional (opt-in "Scan images for text"): the `offscreen` permission only (no host permission: the `<all_urls>` content-script patterns already let the offscreen page download images), plus `offscreen.html/js` (Tesseract host).
- `vendor/` bundled Tesseract.js + core + language models (about 21 MB), Chrome host only. See `vendor/README.md` for versions, licences, sizes and how to update; `build/validate.mjs` pins every vendored script.
- `hosts/bowser/` Bowser boot shim.
- `build/` `build.mjs`, `validate.mjs` (release checks), `package.mjs` (deterministic zip + SHA-256).
- `store/` listing text, privacy policy, permission justifications, checklist, graphics.
- `test/` unit, corpus, e2e, and `test/chrome` (real-Chromium run).

## Maintenance

- Lexicon updates: edit `engine/lexicon.js`, rebuild, run unit and corpus tests. A PATCH release is enough; there is no remote lexicon feed in v1.
- Adapter selector drift: sites change markup. Update `engine/adapters.js`; drift degrades to the generic resolver, not to no protection. Re-run the e2e fixtures.
- Keep UI text free of story details; `build/validate.mjs` rejects spoiler-ish words in the shipped UI.

## Release

1. Bump `version` in `package.json` (semver) and the changelog in `store/release.md`.
2. `npm test`, `npm run e2e`, `node test/chrome/load-extension.mjs`.
3. Chrome: `npm run package`, then follow `store/submission-checklist.md`. The zip is at `release/`.
4. Bowser: Bowser embeds `dist/bowser/spoiler-firewall.js` with `include_str!`. Run `node build/build.mjs`, commit the regenerated bundle with the Bowser change, and keep `node build/build.mjs --check` in CI so a stale bundle fails the build.

Full steps: `store/release.md`.
