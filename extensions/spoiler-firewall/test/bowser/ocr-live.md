# OCR live check (NOT RUN: deferred, the machine was in use)

Needs a free machine, a debug `bowser.exe` and an English (or any) Windows OCR language. It opens a visible
window, so run it only when nothing else is in the foreground.

1. `node extensions/spoiler-firewall/test/bowser/make-ocr-fixture.mjs` (only if the PNGs are missing).
2. `node extensions/spoiler-firewall/test/bowser/ocr-serve.mjs` (serves `127.0.0.1:18083`).
3. Start the debug build with `BOWSER_SPOILER_OCR_TEST_HOST=127.0.0.1` (debug builds only) and a throwaway `--data-dir`.
4. Open `http://127.0.0.1:18083/ocr.html` and expect:
   - `#leak` (image text "GTA 6 ENDING LEAK") covered by a shield within about a second; `#plain` (text "SPOILER TEST 123") not covered;
   - `#data` not covered and no "Couldn't check image" notice (data: answers `unavailable` immediately);
   - switching the tab away and back keeps the answers; no CPU use while the tab is hidden (Task Manager).
5. Settings > Privacy > "Scan images for text (on-device)" off, reload: no OCR (`#leak` is no longer covered by OCR; the page text alone is clean).
6. Repeat step 4 in a private tab and a guest window; close the private tab and confirm nothing is written to the profile
   (`%LOCALAPPDATA%\Bowser` has no OCR text: search it for "ENDING LEAK").
7. Record: first-image latency, whether an image served from the HTTP cache (reload with the server's `cache-control`
   changed to `max-age=600`) still produces an answer (this is the "cache-hit" question in docs/spoiler-firewall.md).
