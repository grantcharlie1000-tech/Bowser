# Bowser Shield: verification

What was run, what was not, and what is still unknown. Written the way `docs/extensions.md` writes
its own status.

## Result in one paragraph

The engine, the translation, the service worker's pipeline, the content script's logic and the
panel's wiring are covered by `node --test extensions/bowser-shield/test/run-tests.js`: 437 tests,
437 pass, 0 skipped, 0 fail, after the seven fixes listed under "Audit fixes" (the suite was 425
green before them). The icon check passes with the four PNG icons. Nothing was run in a live Bowser
window, in WebView2, or in desktop Chrome. Whether WebView2 enforces declarativeNetRequest rules is still unknown, exactly as section
10 of the design says.

## What was run

```
node --test extensions/bowser-shield/test/run-tests.js
```

Machine for the timings below: Intel Core i5-7300HQ at 2.50 GHz, Windows 10 Home, Node v24.17.0.
`node --check` was also run on every `.js` file, and every `.json` file was parsed.

| area | what the tests do |
| --- | --- |
| 9.1 parser | `cases/parser.json` (67 table cases) plus code cases: every reject code in the section 9.1 table with its line, the 17-entry list, the 513-character line, 2 001 rules, the narrow blast radius (ten lines, line 4 bad, nine rules, one error), 256 KiB by bytes, 100 recorded errors, 501 `off` rules, the example list from 3.8, the starter list |
| 9.2 matcher | `cases/matcher.json` (83 table cases): every bullet of section 9.2, plus ports, trailing dots, user info, uppercase, fragments, `$` and `__proto__` labels, overlapping labels through failure links. Each case is also run through the oracle, so the oracle is checked against the table too |
| 9.3 differential | `test/reference-matcher.js` is a naive array scan. 500 seeded lists of up to 300 rules (seed 20240611 plus the list number), 200 seeded requests each, 100 000 pairs. Core and oracle must agree on verdict, rule index and line. The test also asserts more than 10 000 of the decisions are not `pass`, so agreement is not just both sides saying pass |
| 9.4 translation | `cases/dnr.json` (14 table cases), the schema check, id bands, priorities, the label regex run in Node against hosts that match and hosts that straddle a dot, only/except, party, kind, `off`, determinism |
| 9.5 caps | 6 000 rules give 5 000 and 1 000 dropped ascending; `off` and `allow` last in the file are kept while `block` is dropped; 1 100 labels give 1 000 regexes and 100 `regex-cap`; `starter.srl` translated deep-equals `starter-dnr.json` and is also byte-identical to `JSON.stringify(rules, null, 2) + "\n"` |
| 9.6 budgets | starter plus a 2 000-rule list compiled in under 50 ms; 10 000 `decide()` calls in under 50 ms; a check that cost does not grow with rule count |
| service worker | `background/service-worker.js` loaded unmodified into a fake `chrome`: defaults, the probe (4 ways to land in dom mode), sync, orphan removal, per-rule retry when the browser rejects a batch, caps, site exceptions, `frameContext`, counting, the schema-version guard, message validation |
| content script | `content/shield-content.js` loaded unmodified against a fake DOM, `MutationObserver` and `chrome`: the sheet, per-rule `insertRule`, filtered/unfiltered, storage-driven rebuilds, the no-service-worker fallback, the scrubber, batching and dedupe, the chord, the overlay, Escape, click outside |
| panel | the 7.2 state table row by row (`summaryModel`), the options view and its controls through a fake DOM, the chord parser |
| package | the manifest deep-equals section 2 of the design; installer caps from section 0; HTML pages have no inline script; the overlay's CSS string is the same text as the block in `ui.css`; the token header has four to six named colours; focus outline present; no all-caps labels |

### How the tests were written

Test first, in layers, as the TDD skill asks: parser tests, run, red (module missing), parser; then
the matcher tests and the oracle, red, matcher; then translation, then the starter files, then the
cosmetic helpers, the panel, the service worker, the content script, the package. Each layer was run
red for the expected reason before its code existed.

Failures met on the way, all test mistakes and not code mistakes: a tab-first line is not a comment
under the grammar (my case said it was); a 256 KiB line-number I computed with the wrong first-line
length (2596 vs 2597); an orphan DNR rule that the install-time rebuild correctly removes before the
test looked for it; a full-kind rule has no `resourceTypes`. Each was fixed in the test with the
reason, not by weakening an assertion.

Mutation checks, to show the tests can fail: breaking the failure-link output merge in the automaton
fails the differential test and three AC unit tests (4 failures); making the scrubber stop removing
the blocked attribute, and making a click inside the overlay close it, together fail 12 content-script
tests. Both were reverted and the suite was green again.

### Budgets as measured

| | cold (first run in a fresh process) | warm (runs 2 to 5) | limit |
| --- | --- | --- | --- |
| compile starter + 2 000 rules | 21 to 30 ms | 6 to 9 ms | 50 ms |
| 10 000 `decide()` calls | 42 to 45 ms | 31 to 38 ms | 50 ms |
| one `decide()` | | about 4 µs | |

The cold decide figure is close to the limit on this laptop. The assertions take the best of three
runs so that one scheduler hiccup on a busy machine does not fail a check that exists to catch an
O(N) scan; the bounds themselves are not loosened. `decide` is O(|url| + hits): the code that was
sped up was allocation (host labels were split into arrays, `endsWith("." + v)` built strings), not
the algorithm. The first draft measured 139 ms for the same batch.

## What could not be run

- **No live Bowser window, no WebView2, no desktop Chrome or Edge.** The service worker, content
  script and panel were exercised only against fakes. The fakes encode my reading of the `chrome.*`
  contracts, so they cannot find a place where WebView2 differs.
- **Whether WebView2 enforces `declarativeNetRequest`** (dynamic rules, `allowAllRequests`, the
  static ruleset), and whether `onRuleMatchedDebug` exists there. The probe proves only that the API
  round-trips a rule. The options page says so in the words section 5.6 prescribes.
- **Regex acceptance.** The 26 starter `label:` regexes and any user ones were not given to Chrome's
  RE2 or to `isRegexSupported`. If the browser rejects one, a static rule is skipped by the browser,
  and a dynamic one goes through the per-rule retry in the service worker and is reported as
  `rejected-by-browser`.
- **Page CSP and the overlay.** The panel's styles are adopted as a constructable stylesheet, with a
  `<style>` fallback, and the cosmetic layer uses `sheet.insertRule`, because those paths are not
  blocked by a page's `style-src`. That is from knowledge of the platform, not from a run on a
  strict-CSP site.
- **Closed shadow root focus and the chord inside WebView2.** Untested outside the fake.
- **Screenshots of the UI.** No browser was driven, so the design was not looked at. Colours were
  checked by arithmetic: slate on paper 5.4:1, gate on paper 5.7:1, alert on paper 5.9:1, ink on
  paper about 14:1.
- **Icons.** See below.

### Hand-check list for a real Bowser pass

1. Load the folder. The permission dialog should show exactly three permission strings and two host
   patterns.
2. Press Alt+Shift+S on an http page. The panel appears at the top right, 320 px wide, with the switch
   focused. Escape closes it. A click outside closes it.
3. Walk the 7.2 table: on with counts; switch off ("Shield is off", counts grey); "Don't filter this
   site" (then "This site is never filtered" and "Filter this site"); a non-http tab ("Nothing to
   filter here").
4. Settings: edit rules, introduce a bad line, see `line N: message`; Save; Re-check; add and remove a
   site; toggle the starter list.
5. Load a page that requests from a host with a label such as `adserver`. Look at the blocked count.
   If it stays at 0 while the page shows the ad, network blocking is not enforced in this runtime, and
   the entry for `docs/extensions.md` ("Uncertainty, stated plainly") is "not enforced".
6. Force dom mode (or run where the probe fails) and check the panel says "hidden", never "blocked".

## Icons

`icons/icon-16.png`, `icon-32.png`, `icon-48.png` and `icon-128.png` are in the folder. The text
editor cannot write a PNG (it begins with byte 0x89), so they were drawn afterwards by the same
generator below, then redrawn as a bow in the panel accent, at 16, 32, 48 and 128 pixels. The
browser's own menu glyph is the same bow (`icons/menu/shield.svg`).
The package test checks the PNG signature of each file.

The snippet below is the retired shield generator. Do not run it. The files in `icons/` are the bow.

```js
// node make-icons.js icons   (then delete make-icons.js)
const fs = require("fs"), path = require("path"), zlib = require("zlib");
const GATE = [0x1c, 0x6a, 0x7a], PAPER = [0xf2, 0xf4, 0xf6];
function crc32(buf) { let c, crc = 0xffffffff; for (let n = 0; n < buf.length; n++) { c = (crc ^ buf[n]) & 0xff; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; crc = (crc >>> 8) ^ c; } return (crc ^ 0xffffffff) >>> 0; }
function chunk(type, data) { const out = Buffer.alloc(12 + data.length); out.writeUInt32BE(data.length, 0); out.write(type, 4, "ascii"); data.copy(out, 8); out.writeUInt32BE(crc32(out.subarray(4, 8 + data.length)), 8 + data.length); return out; }
function sample(u, v) { if (v < 0.08 || v > 0.94) return 0; const half = v < 0.52 ? 0.4 : (0.4 * (0.94 - v)) / (0.94 - 0.52); if (Math.abs(u - 0.5) > half) return 0; if (v > 0.22 && v < 0.84 && Math.abs(u - 0.5) < half - 0.07 && Math.floor((u - 0.1) * 12) % 2 === 0) return 2; return 1; }
function png(size) {
  const raw = Buffer.alloc((size * 4 + 1) * size), grid = 4;
  for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
    let a = 0, r = 0, g = 0, b = 0;
    for (let sy = 0; sy < grid; sy++) for (let sx = 0; sx < grid; sx++) { const s = sample((x + (sx + 0.5) / grid) / size, (y + (sy + 0.5) / grid) / size); if (!s) continue; const c = s === 2 ? PAPER : GATE; a++; r += c[0]; g += c[1]; b += c[2]; }
    const o = y * (size * 4 + 1) + 1 + x * 4;
    raw[o] = a ? Math.round(r / a) : 0; raw[o + 1] = a ? Math.round(g / a) : 0; raw[o + 2] = a ? Math.round(b / a) : 0; raw[o + 3] = Math.round((255 * a) / (grid * grid));
  }
  const head = Buffer.alloc(13); head.writeUInt32BE(size, 0); head.writeUInt32BE(size, 4); head[8] = 8; head[9] = 6;
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk("IHDR", head), chunk("IDAT", zlib.deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}
const dir = process.argv[2]; fs.mkdirSync(dir, { recursive: true });
for (const size of [16, 32, 48, 128]) fs.writeFileSync(path.join(dir, "icon-" + size + ".png"), png(size));
```

## Where the design disagrees with itself, and what was done

The task's rule: section 4 decides matching, section 5 decides declarativeNetRequest output, section 7
decides UI. Everything else is judgement, and is listed so it can be overruled.

### Decided by that rule

1. **The overlay's panel code loads on demand (section 7.1).** The first version listed `ui/panel.js`
   and `content/panel-css.js` in the content script entry, so about 34 KiB of panel ran in every frame
   of every page although it is only used on a key press. It is now injected into the top frame by the
   service worker (`loadPanel`, `chrome.scripting.executeScript`) the first time the chord is pressed,
   which costs the `scripting` permission (section 2.1). The chord helpers moved to
   `lib/shield-chord.js`, which every frame still carries. The manifest now deep-equals section 2 of
   the design with no exceptions. **Not verified live:** that this WebView2 runtime provides
   `chrome.scripting`. The Node tests fake the injection; if the API is missing the chord opens
   nothing (the popup and options pages still work).
2. **Section 5.2 vs 3.3 on `path:`.** 5.2 says a path becomes a plain-substring `urlFilter` with "no
   pattern characters", but 3.3 allows `*`, `|` and `^` in a path, and in a `urlFilter` they are
   wildcard, anchor and separator. Section 5 wins for DNR output, so such a path is **not translated**:
   it is reported in `dropped` with reason `path-pattern-char` and the banner says the rule does not
   apply in network mode. The core matcher still treats the characters literally (section 4). The
   starter list has none.
3. **Section 5.2 `label:` regex vs section 4.** The regex ends in `\.`, so a token that sits only in
   the last label (`foo.adserver`) is matched by `decide()` but not by the network rule. Section 5
   wins for DNR output; the gap is asserted in a test and is unlikely to matter, since the last label
   is the TLD.
4. **Section 5.5 regex cap.** The cap of 1 000 is "static and dynamic combined", but 5.5 also says
   "keep the first 1 000". The translator takes a `regexCap` option (default 1 000, so the 9.5 test
   passes as written), and the service worker passes `1000 - 26` while the starter list is enabled
   (the 26 `label:` rules of the static ruleset), or the full 1 000 when it is off.

### Counts in the design that do not match the files

5. **"41 block rules" (3.7, 5.5) vs 38.** The starter block in section 3.7 has 13 + 8 + 5 label rules
   and 12 path rules: 38 block rules and 10 hide rules. `rules/starter.srl` is the section 3.7 block
   verbatim (a test extracts it from `DESIGN.md` and compares), so `starter-dnr.json` has 38 rules, ids
   1000 to 1037.
6. **"21 files" (section 1) vs 24.** The tree in section 1 lists 24 files. With this file there are 25,
   plus the four icons once they exist.
7. **"Byte for byte" (3.7) vs "deep-equals" (the task).** Both are asserted, and both hold.

### Judgement calls in underspecified places

8. **What a hard limit leaves behind.** "A hard limit stops the compile" is read as: stop reading, keep
   the rules accepted before the limit line, record one error naming the limit and the line.
   `too-many-errors` is recorded as a 101st entry at the line that would have been the 101st.
9. **The 24-label cap** is applied to the request host and to the site host; either one over 24 labels
   is a `pass`, which is the safe direction for the site (nothing gets filtered on a strange host).
10. **Error codes beyond the 9.1 table**: `bad-target`, `bad-site`, `bad-label`, `bad-path`,
    `bad-css`, `css-length`, `qualifier-no-value`, `duplicate-qualifier`, `bad-qualifier`,
    `leading-space`, `list-too-large`, `too-many-off`, `too-many-errors`; warning `duplicate-rule`.
    A rule line must start in column 1 (`leading-space`); the grammar allows leading spaces only on
    blanks and comments. Line length is checked on rule lines only, after comments are skipped, as 4.7
    orders it.
11. **Site exceptions have two sources.** `shield.siteOff` (the panel's list) and `off site:` lines in
    My rules. Both are compiled together (the list becomes a synthetic `off site:` list) and both
    produce rules in the 30 000 band. The band holds 500, so more than 500 in total are dropped with
    reason `band-full`. "Filter this site" removes every list entry that covers the host. A site that
    is covered only by an `off` rule shows "This site is never filtered" with the button disabled and a
    sentence pointing at My rules (`page.siteOffRuleOnly`).
12. **"Adds `location.host`" (7.2).** `host` can carry a port, which the `site` lexical rule rejects, so
    the host name is added. A host that cannot be a site rule (`localhost`, an IPv6 literal) gets a
    disabled button and the reason.
13. **7.2 state precedence** when several rows apply: non-http page, then off, then never-filtered, then
    the mode line. The table does not say; this is the order a user would expect. In dom mode the panel
    shows only "N hidden on this site", as the table says, with no session line.
14. **"Network blocking: active" (7.2) vs "enforcement unverified" (5.6, 7.3).** The summary says
    "active" because the table says so, and the options page carries the caveat. "Active" is stronger
    than what the probe proves; if that is a problem the string is `TEXT.network` in `ui/panel.js`.
15. **Starter source in dom mode.** The content script cannot read `rules/starter.srl` (no
    `web_accessible_resources`), so in dom mode the `frameContext` reply carries `starterSrl`. If the
    service worker is unreachable, the fallback path has no starter text: it scrubs with the user's
    rules only, and applies cosmetic rules from `shield.cosmetic`, which the service worker wrote
    earlier. If the service worker never ran, there is nothing in storage and nothing to apply.
16. **One layer at a time vs the static ruleset (8.3 vs 10.3).** In dom mode the service worker
    disables the static starter ruleset, so the network layer and the scrubber never both act. The
    static ruleset still protects the case where the service worker never runs at all.
17. **`shield.cosmetic` shape.** 6.1 shows `byExcept: [{sel, except}]`. A rule with both `only` and
    `except` carries its `only` list in that entry (`{sel, except, only}`), so except-before-only still
    holds in the content script. `shield.compiled.counts` is the user list; the starter counts are
    sent to the panel separately.
18. **Messages.** A failed mutation (invalid host, text over 256 KiB, a newer schema) replies with the
    current state plus `error`, and the panel shows it as the notice. Mutations run one at a time.
    Messages from any sender other than this extension are refused.
19. **The chord is configurable** (7.1) through a field in the options view, validated in both the
    panel and the service worker. A stored chord that does not parse falls back to Alt+Shift+S.

### Additions to the core API beyond the design's names

`compileAll([{tag, src}])` (several lists, each under its own limits, matched together; needed because
the starter list plus a full user list is over 2 000 rules), `siteIsOff`, `buildCosmetic`,
`cosmeticFor`, `isValidSite`, `hostMatches`, `sameParty`, `normalise`, `SuffixMap` and `AC` as objects,
and on the translator `lineById`, `listById`, `PROBE_RULE`, `isBlockRuleId`. Decisions carry a `list`
tag as well as `line`.

## Audit fixes

Seven findings from the Opus audit, each fixed test first (a failing test, then the change). The
suite went from 425 to 437 tests, all passing. Still only Node against fakes: nothing here was seen in
a live Bowser window or in WebView2.

1. **Chord and outside-click listeners are on `window`, capture phase.** They were on `document`, so a
   page listener on `window` in the capture phase could stop the chord first and hide the only UI in
   WebView2. A test checks both listeners are on `window` with capture `true` and that `document` has
   neither.
2. **The overlay host has no stable hook and cannot be hidden by a page stylesheet.** The
   `data-bowser-shield-panel` attribute is gone. The layout (`all:initial`, `position:fixed`,
   `top:12px`, `right:12px`, `z-index:2147483647`, `display:block`) is set with
   `style.setProperty(name, value, "important")`. The tests find the host as the `div` that owns a
   shadow root, assert it carries no attribute, and assert each property was set `important`.
3. **A hide selector cannot contain `{`, `}` or `;`.** `cssProblem` rejects them with `bad-css` and
   "a selector cannot contain { } or ;", so a hide rule cannot smuggle a declaration. Parser table
   case added. The starter list has none of these characters.
4. **The static starter ruleset follows the switches, not the probe.** It is enabled whenever the
   shield and the starter checkbox are on, including in dom mode. Counting is unchanged: dom mode
   still counts only hidden reports, not network matches. The test that expected it switched off in
   dom mode now expects it on, and off again with the checkbox or the shield.
5. **No frame context still scrubs with the starter list, and no storage key was added.** Section
   7.4's key list stays closed. The content script fetches `rules/starter.srl` through
   `chrome.runtime.getURL`, compiles it, and caches the text for the life of the script. It honours
   `shield.starterEnabled`. If the fetch fails or answers with an error status it scrubs with the
   user's rules only and does not throw (a failure is not cached, so the next refresh tries again). A
   subframe with no worker applies only global selectors and fetches nothing. Tests use a fake
   `fetch`, and also check it is fetched once however many refreshes run and is not used when the
   worker answers.
6. **A `path:` value that starts with `//` is not translated.** As a `urlFilter` it could match the
   host. It is reported in `dropped` with reason `path-scheme-slash`, like the `* | ^` characters, and
   no rule is emitted. The core matcher still reads the path literally; a test shows it still blocks.
7. **The service worker accepts only the chords `parseChord` accepts.** The loose `CHORD_PATTERN`
   let through `Shift+S` and `Alt+Alt+S`. The worker now has `chordAccepted`, which has the same
   rules (one key, a modifier at most once, one of Alt, Ctrl or Meta). One test refuses both
   examples, and another runs 22 candidate strings through the worker and `parseChord` and requires
   the same answer. The worker keeps its own copy because it does not load `ui/panel.js`; the parity
   test is what keeps the two together.

Not changed: the panel's dropped-rules banner explains `path-pattern-char` drops in its own sentence;
a `path-scheme-slash` drop shows only in the general "N rules are not being enforced" line.

## Known limits, stated plainly

- Counters live in service-worker memory, as designed, so they reset whenever Chrome stops the worker.
  The panel will sometimes show a smaller number than it did a minute ago.
- The first request of a page can be sent before the cosmetic sheet is filled (section 6.2 says so).
- `decide()` and the network layer can disagree on the public-suffix edge (section 4.5 says so).
- In dom mode nothing is blocked on the network. The panel says "hidden".
- The regex rules do not match a URL with user info (`https://user@host/`) because of the character
  class in the `label:` regex.
- Non-ASCII host names have to be written as punycode.
- A selector the browser accepts but that matches nothing on a page is not something the panel can
  tell the user about; only selectors the browser refuses are counted.
