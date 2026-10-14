# Extension probe fixtures

Plain-text unpacked Manifest V3 extensions for Bowser's Stage 1 check: does a local extension
actually execute under WebView2? Nothing here is a real extension, and none of it is a Chrome Web
Store package. Each folder is a complete unpacked extension; load it in place, never edit it.

| Folder | Purpose | `inspect_source` |
|---|---|---|
| `probe-mv3` | Baseline: `storage`, host access to `http://127.0.0.1/*` only, one content script, one service worker, plus `popup.html`, `options.html` and `popup.js` (Stage 2). | accepted |
| `probe-mv3-locales` | Same, with `default_locale`, a `__MSG_extName__` name and `_locales/en/messages.json`. | accepted (`_locales` is the one allowed root underscore name) |
| `probe-mv3-key` | Same, with a manifest `key`. | accepted (unknown keys are ignored) |
| `probe-mv3-metadata` | Same, with a root `_metadata/` folder holding a dummy file. | refused (`ReservedRootName`) |

## What a working extension does

1. The content script runs on `http://127.0.0.1/*` pages and sets `data-bowser-probe="content-script"`
   on `<html>`.
2. It requests `GET <page origin>/__bowser_probe__/beacon?stage=content-script&reply=<JSON>`.
3. It sends `chrome.runtime.sendMessage({ type: 'bowser-probe' })`. The service worker answers with
   `{ ok, from: 'service-worker', id: chrome.runtime.id, stored }`, where `stored` is what the worker
   wrote to `chrome.storage.local` in `onInstalled`.
4. The content script sets `data-bowser-probe-reply` and requests
   `GET <page origin>/__bowser_probe__/beacon?stage=worker-reply&reply=<JSON>`.

A loopback server that logs those two requests is enough to prove both the content script and the
worker ran. The `id` in the reply is the runtime's extension id.

## The key fixture

`probe-mv3-key` carries a throwaway RSA-2048 public key generated for this fixture. The private half
was discarded and belongs to no real extension. Chromium derives an extension id from a `key` as the
first 16 bytes of SHA-256 over the DER public key, written with `0-9a-f` mapped to `a-p`. For this
key that id is:

    ciocoifocbggchcecjgibbaeojegkpen

Microsoft does not document whether WebView2 honours `key`. The live probe observed that it does: the
runtime reported exactly this id in every run (see `docs/extensions.md`, Verification status).

## The underscore rule

WebView2 documents `E_ACCESSDENIED` for a file or directory name starting with `_`. Bowser refuses
any root entry starting with `_` other than `_locales` before it copies or loads anything, and never
deletes files inside a folder the runtime has loaded. `probe-mv3-metadata` exists to test that.
