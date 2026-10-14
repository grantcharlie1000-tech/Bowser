# Bowser Shield — design

An unpacked Manifest V3 extension that blocks third-party ad and tracker requests and hides ad
slots. It is loaded through Bowser's existing extension installer: the user picks the folder that
holds `manifest.json`, Bowser copies it, shows the declared permissions, and WebView2 loads the
copy.

This document is the whole architecture. An implementer should not have to make further structural
choices.

Scope note: Bowser already has a native request blocker in the Rust code. Bowser Shield is separate
and shares nothing with it — not its rule syntax, not its rule identifiers, not its matching
procedure. Nothing in this design touches Rust, the UI, or the native blocker.

## 0. What fits inside Bowser's installer

From `docs/extensions.md` and the module comment in `src/extensions.rs`:

- Only unpacked folders chosen by the user. No Web Store.
- `manifest.json` at most 1 MiB; manifest version 2 or 3; `name` 1–128 plain-text characters;
  `version` 1–4 dot-separated numbers.
- `permissions`, `host_permissions`, `optional_permissions`, `optional_host_permissions` and the
  combined `content_scripts[].matches` are each at most 256 entries of at most 256 characters, and
  **every entry must be a plain string**. A permission written as an object is refused outright.
- At most 2000 files, 128 MiB total, 12 folder levels below the root. No links or junctions. No
  file name containing `< > : " / \ | ? *` or a control character, and no name ending in `.` or a
  space.
- Bowser shows the declared permissions before install, and does **not** enforce them afterwards.
  WebView2 does whatever WebView2 does.
- The `ms-browser-extension` scheme stays blocked for user-initiated navigation.

Bowser Shield ships 21 files, roughly 120 KiB, two folder levels deep, three permission strings,
two host permissions and two content-script match patterns. Every cap has a wide margin.

The blocked scheme and the missing browser chrome both change the UI design. See section 7.

## 1. File tree

```
extensions/bowser-shield/
  DESIGN.md                    this document; not loaded at runtime
  manifest.json                see section 2
  lib/
    shield-core.js             the pure engine: SRL lexer, parser, compiler, matcher.
                               No chrome.* calls, no DOM, no timers. Loadable three ways
                               (section 1.1) so the Node tests exercise the shipped file.
    shield-dnr.js              pure translation of a compiled list into declarativeNetRequest
                               rules: id bands, priorities, cap handling. No chrome.* calls.
  rules/
    starter.srl                the built-in starter list, in SRL text (section 3.7)
    starter-dnr.json           the same block/allow rules precompiled as a static DNR ruleset,
                               referenced by manifest declarative_net_request.rule_resources
  background/
    service-worker.js          classic service worker. Probes the network layer, owns rule
                               sync, owns counters, answers content-script and UI messages
  content/
    panel-css.js               the overlay panel stylesheet as a JS string constant
                               (avoids needing web_accessible_resources)
    shield-content.js          document_start: cosmetic CSS, DOM scrubber in dom mode,
                               overlay panel host, keyboard chord
  ui/
    panel.js                   one renderer for the panel, used by the popup, the options
                               page and the in-page overlay. Takes a root node, a state
                               object and a send() function; renders and wires events
    popup.html
    popup.js                   loads shield-core + panel.js, fetches state, renders summary view
    options.html
    options.js                 loads shield-core + panel.js, renders the full editor view
    ui.css                     shared styles for popup.html and options.html
  icons/
    icon-16.png
    icon-32.png
    icon-48.png
    icon-128.png
  test/
    run-tests.js               Node test entry point, `node --test` compatible, no dependencies
    reference-matcher.js       a deliberately naive O(N) matcher used only as a test oracle
    cases/
      parser.json              table-driven parser cases
      matcher.json             table-driven matcher cases
      dnr.json                 table-driven translation cases
```

21 files, maximum depth 2.

### 1.1 How `shield-core.js` is loaded three ways

It is a classic script (no `import`/`export`) that builds one object and ends with:

```js
if (typeof module === "object" && module.exports) module.exports = ShieldCore;  // Node tests
if (typeof self === "object") self.ShieldCore = self.ShieldCore || ShieldCore;  // SW + content
if (typeof window === "object") window.ShieldCore = ShieldCore;                 // popup/options
```

The service worker takes it with `importScripts("../lib/shield-core.js")`; the content script lists
it first in `content_scripts[].js`, which Chrome guarantees runs in array order; the popup and
options pages use `<script src>`. `shield-dnr.js` follows the same pattern but is only loaded by
the service worker and the tests.

Deliberate consequence: the service worker is **not** `"type": "module"`. MV3 module service workers
are a newer feature and this design does not want to find out the hard way whether the WebView2
subset includes them. `importScripts` is the oldest, dullest path.

## 2. manifest.json

```json
{
  "manifest_version": 3,
  "name": "Bowser Shield",
  "version": "1.0.0",
  "description": "Blocks third-party ad and tracker requests and hides ad slots, from a rule list you control. No remote lists, no telemetry.",
  "minimum_chrome_version": "120",
  "icons": {
    "16": "icons/icon-16.png",
    "32": "icons/icon-32.png",
    "48": "icons/icon-48.png",
    "128": "icons/icon-128.png"
  },
  "permissions": [
    "declarativeNetRequest",
    "declarativeNetRequestFeedback",
    "storage"
  ],
  "host_permissions": [
    "http://*/*",
    "https://*/*"
  ],
  "background": {
    "service_worker": "background/service-worker.js"
  },
  "action": {
    "default_title": "Bowser Shield",
    "default_popup": "ui/popup.html",
    "default_icon": {
      "16": "icons/icon-16.png",
      "32": "icons/icon-32.png",
      "48": "icons/icon-48.png",
      "128": "icons/icon-128.png"
    }
  },
  "options_ui": {
    "page": "ui/options.html",
    "open_in_tab": true
  },
  "content_scripts": [
    {
      "matches": ["http://*/*", "https://*/*"],
      "js": ["lib/shield-core.js", "content/panel-css.js", "content/shield-content.js"],
      "run_at": "document_start",
      "all_frames": true
    }
  ],
  "declarative_net_request": {
    "rule_resources": [
      {
        "id": "starter",
        "enabled": true,
        "path": "rules/starter-dnr.json"
      }
    ]
  }
}
```

### 2.1 Why each permission

Bowser shows these three strings to the user at install. Each has to earn its place.

- **`declarativeNetRequest`** — the only Manifest V3 way to actually stop a request. It grants
  blocking and upgrading without any host permission, which is why the host permissions below are
  justified on other grounds. Without it there is no network layer at all.
- **`declarativeNetRequestFeedback`** — grants `onRuleMatchedDebug`, the event that reports which
  rule matched which request. It is the only honest source for the blocked count in network mode;
  nothing else tells an MV3 extension what declarativeNetRequest did. Chrome restricts this
  permission to **unpacked** extensions, which is exactly and only how Bowser installs anything, so
  the restriction costs nothing here. It carries no extra data access beyond what the extension
  already blocks. If the runtime refuses it, the extension still blocks; the count shows
  "unavailable" (section 8.4).
- **`storage`** — `chrome.storage.local` holds the user's rule text, site exceptions and settings.
  No `unlimitedStorage`: the format caps the list at 256 KiB, far under the local quota.

Permissions deliberately **not** requested:

- `webRequest` / `webRequestBlocking` — removed from MV3 for blocking, and it would hand the
  extension the contents of every request.
- `scripting` — the content script injects its own `<style>` element; `insertCSS` is not needed.
- `tabs` — `chrome.tabs.query` works without it, and the tab `url` comes from the host permissions
  below. Asking for `tabs` as well would be redundant noise in Bowser's install dialog.
- `alarms`, `notifications`, `cookies`, `webNavigation` — nothing in this design needs them.

### 2.2 Why these host permissions

`http://*/*` and `https://*/*`, not `<all_urls>`. Two reasons they are needed:

1. The panel must name the current site and offer "never filter this site". That means reading the
   tab's URL, which requires host access to that tab.
2. A content script in a cross-origin subframe cannot see the top-level document's host, so it
   cannot decide first-vs-third party or apply site exceptions on its own. It asks the service
   worker, which answers from `sender.tab.url` — also host-gated.

`<all_urls>` would additionally cover `file:`, `ftp:` and extension pages. The extension has no
business on local files, so the two scheme-scoped patterns are the smaller ask that still works.

### 2.3 Other manifest notes

- No `content_security_policy` entry. The default MV3 policy is already the strictest thing here:
  no remote script, no `eval`, no inline handlers. Nothing in the design needs a relaxation.
- No `web_accessible_resources`. The overlay panel's stylesheet ships as a JS string in
  `content/panel-css.js` precisely so that no extension file has to be exposed to pages.
- `minimum_chrome_version: "120"` records the assumption behind the dynamic-rule budget in
  section 5.4. On an older engine the extension refuses to load rather than silently overflowing.
- `declarative_net_request.rule_resources` loads the starter list as a **static** ruleset. Static
  rules are applied by the browser from the manifest and do not depend on the service worker ever
  running — deliberate insurance against the service-worker risk in section 10.

## 3. SRL — the rule format

**SRL, "Shield Rule Lines", version 1.** Files use the extension `.srl` and are UTF-8 text with
either `\n` or `\r\n` endings. One rule per line. Nothing is inferred from punctuation: every rule
begins with a verb word and names its own kind. This is an original format and is not compatible
with, and does not borrow from, any published filter-list syntax.

### 3.1 Grammar

```ebnf
list        = { line , newline } ;

line        = blank | comment | rule ;
blank       = { " " } ;
comment     = { " " } , "#" , { any-char-except-newline } ;

rule        = verb , sp , target , { sp , qualifier } , { " " } ;

verb        = "block" | "allow" | "hide" | "off" ;
target      = kind , ":" , value ;
kind        = "host" | "label" | "path" | "css" | "site" ;

qualifier   = kind-q | party-q | only-q | except-q ;
kind-q      = "kind"   , sp , typelist ;
party-q     = "party"  , sp , party ;
only-q      = "only"   , sp , sitelist ;
except-q    = "except" , sp , sitelist ;

party       = "first" | "third" | "any" ;
typelist    = typename , { "," , typename } ;
typename    = "script" | "image" | "frame" | "xhr" | "media"
            | "font" | "style" | "ping" | "other" ;
sitelist    = hostvalue , { "," , hostvalue } ;

sp          = " " , { " " } ;
```

Everything outside a `css:` value is lowercase ASCII. An uppercase verb, kind or qualifier is an
error, not a convenience. Tabs are not whitespace here and are an error; a stray tab is almost
always a paste artefact and silently accepting it makes columns in error messages lie. Comments are
whole lines only — there is no trailing `#`, because `#` is legal inside a CSS selector.

### 3.2 Which verb takes which kind

Strict, and the compiler checks it from exactly this table:

| verb    | allowed kinds      | allowed qualifiers            | meaning                                               |
| ------- | ------------------ | ----------------------------- | ----------------------------------------------------- |
| `block` | host, label, path  | kind, party, only, except     | stop the matching request                             |
| `allow` | host, label, path  | kind, party, only, except     | never block the matching request                      |
| `hide`  | css                | only, except                  | hide matching elements with CSS                       |
| `off`   | site               | *(none)*                      | this site is never filtered at all                    |

Any other pairing is an error. A qualifier may appear at most once per rule; repeating one is an
error rather than a last-wins surprise.

### 3.3 Values

| kind    | lexical rule                                                                                                                   |
| ------- | ------------------------------------------------------------------------------------------------------------------------------ |
| `host`  | 4–253 chars. Two or more labels joined by `.`; each label 1–63 chars of `[a-z0-9-]`, not starting or ending with `-`.            |
| `site`  | identical to `host`.                                                                                                             |
| `label` | 4–40 chars of `[a-z0-9-]`, containing at least one letter, not all digits, not starting or ending with `-`.                     |
| `path`  | 3–200 chars, must start with `/`, printable ASCII excluding space, `#`, `"` and `\`. Compared case-insensitively.                |
| `css`   | 3–200 chars, **no spaces and no commas**, and must contain at least one of `#`, `.` or `[`. See 3.5.                             |

`label` has a 4-character floor on purpose. A 2- or 3-character token like `ad` or `ads` matches
inside ordinary words (`downloads.example`, `headers.example`) and would break sites for a user who
has no way to see why.

`path` must begin with `/` so that a rule can never accidentally match inside the host.

### 3.4 Semantics of each kind

- **`host:v`** — matches when the request host is `v`, or ends with `.` + `v`. Label-boundary only:
  `host:ad-delivery.example` matches `ad-delivery.example` and `img.ad-delivery.example`, and does
  **not** match `notad-delivery.example` or `ad-delivery.example.co`.
- **`label:v`** — matches when any single DNS label of the request host contains `v` as a
  substring. Since `v` cannot contain `.`, a match can never straddle two labels.
  `label:click-track` matches `eu.click-tracker.example` and `click-track.example`.
- **`path:v`** — matches when `v` occurs as a substring of the request's path plus query string,
  compared in lowercase. The fragment is never part of a request and is excluded.
- **`css:v`** — a CSS selector for the cosmetic layer. Never consulted for network decisions.
- **`site:v`** — names a first-party site for the `off` verb, matched like `host:`.

### 3.5 CSS selector restrictions

The format forbids spaces inside a `css:` value, which removes all ambiguity about where the
selector ends and `only`/`except` begins — the parser just splits on spaces. It also rules out
descendant combinators. Child, sibling and adjacency still work without spaces:
`div[data-ad-slot]>iframe`, `aside[data-promo]+section[data-promo]`.

Commas are forbidden: one selector per rule, so one line can be reported as failing.

At least one of `#`, `.` or `[` must appear, so a rule can never hide every element of a tag.
`hide css:div`, `hide css:*`, `hide css:iframe` and `hide css:body` are all rejected by the
compiler with the message "selector must be narrowed by an id, class or attribute". This is the
rule that keeps the cosmetic layer from eating normal page content.

### 3.6 Defaults, limits and errors

Defaults when a qualifier is absent:

- `party` defaults to `third` for `block` and `allow`. A rule that should also apply to the site's
  own requests must say `party any` or `party first`. Stated once here and once in the options-page
  help, because it is the only non-obvious default in the format.
- `kind` defaults to all nine types.
- `only` and `except` default to empty, meaning every first-party site.
- `only` and `except` on the same rule: `except` is applied first. If a site is in both, the rule
  does not apply there.

Limits, enforced by the compiler:

| limit                               | value       |
| ----------------------------------- | ----------- |
| list text                           | 256 KiB     |
| rules in one list                   | 2 000       |
| characters in one line              | 512         |
| entries in one `only`/`except` list | 16          |
| `off` rules in one list             | 500         |
| labels in a host being matched      | 24          |
| recorded errors before giving up    | 100         |

Error handling is strict about syntax and narrow about blast radius: a bad line is skipped and
recorded as `{line, column, code, message}`; every other line still compiles. The options page
shows the list of errors with line numbers. Two identical rule lines collapse to one, first
occurrence wins, recorded as a warning rather than an error. Exceeding a hard limit stops the
compile and is reported as a single error naming the limit and the line where it was passed.

### 3.7 The starter list

`rules/starter.srl`. These are written for this design from generic third-party ad and tracker URL
shapes. No vendor names appear anywhere in it, deliberately: a list of company domains is both the
thing that looks copied and the thing that goes stale. Generic role words in a host label —
`adserver`, `click-track`, `impression` — have described the same job for twenty years and
are justifiable on their own.

```
# Bowser Shield starter list. SRL version 1.
# Generic third-party ad and tracker URL shapes. Edit or disable this list in Options.

# Host labels naming an ad-serving role.
block label:adserver
block label:adserving
block label:ad-delivery
block label:addelivery
block label:ad-serve
block label:adexchange
block label:ad-exchange
block label:adnetwork
block label:ad-network
block label:adbanner
block label:bannerad
block label:popunder
block label:interstitial-ad

# Host labels naming a click or impression recorder.
block label:click-track
block label:clicktrack
block label:clickserve
block label:imptrack
block label:impression-log
block label:trackpixel
block label:pixel-track
block label:beacon-collect

# Host labels naming a visitor telemetry collector.
block label:telemetry-collect
block label:event-collector
block label:usertrack
block label:visitor-track
block label:session-replay

# Path fragments used by ad slots and measurement endpoints.
block path:/ad-slot/
block path:/adslots/
block path:/sponsored-unit/
block path:/banner-rotate/
block path:/header-bid/
block path:/rtb-bid/
block path:/track/impression
block path:/track/click
block path:/collect/beacon
block path:/pixel.gif kind image
block path:/track.gif kind image
block path:/beacon.png kind image

# Cosmetic. Attribute-qualified only; see section 3.5.
hide css:iframe[src*="/ad-slot/"]
hide css:iframe[src*="/sponsored-unit/"]
hide css:div[data-ad-slot]
hide css:div[data-ad-unit]
hide css:div[data-ad-placeholder]
hide css:div[data-advertisement]
hide css:[data-widget-type="sponsored"]
hide css:aside[aria-label="Advertisement"]
hide css:section[aria-label="Advertisement"]
hide css:img[width="1"][height="1"][src*="/track/"]
```

41 block rules and 10 hide rules. That is enough to be useful on an ordinary news page and small
enough that a user can read all of it before trusting it.

`rules/starter-dnr.json` is the translation of the 41 block rules by `shield-dnr.js`, written into
the repo so the static ruleset is loadable without the service worker. A unit test (section 9.5)
asserts that translating `starter.srl` reproduces `starter-dnr.json` byte for byte, so the two can
never drift.

### 3.8 Example of a user list

```
# My list.

# Block an ad host and everything under it, including its own first-party requests.
block host:ads.shop.example party any

# A measurement endpoint, but only scripts, and not on the site that pays for my work.
block path:/metrics/collect kind script except client.example

# A tracker that one site genuinely needs.
block label:visitor-track
allow label:visitor-track only intranet.example

# Hide a leftover container the block leaves behind.
hide css:div[data-slot-state="empty"] only news.example

# My bank, untouched.
off site:bank.example
```

## 4. The matcher

All of this lives in `lib/shield-core.js` and never touches `chrome.*`.

### 4.1 Compiled shape

```js
Compiled = {
  version: 1,
  sourceHash: "…",            // FNV-1a over the normalised source, for cache validation
  rules: Rule[],              // index === local rule index, in file order
  block:  Index,
  allow:  Index,
  siteOff: SuffixMap,
  cosmetic: {
    global:  number[],            // rule indexes with no only/except
    byOnly:  Map<string, number[]>,
    byExcept: number[]            // rule indexes carrying an except list
  },
  errors: Diag[], warnings: Diag[],
  counts: { block, allow, hide, off }
}

Rule = {
  i, line, verb, kind, value,
  types: number,              // 9-bit mask; 0b111111111 means "all"
  party: 0 | 1 | 2,           // 0 any, 1 first, 2 third
  only: string[] | null,
  except: string[] | null
}

Index = { hosts: SuffixMap, labels: AC, paths: AC }
```

### 4.2 `SuffixMap` — the label trie

A reversed-label trie stored as nested plain objects. Each node maps one DNS label to a child node;
the key `"$"` holds the array of rule indexes that terminate there.

`host:ad-delivery.example` and `host:cdn.ad-delivery.example` compile to:

```js
{ example: { "ad-delivery": { $: [0], cdn: { $: [1] } } } }
```

Lookup for host `img.cdn.ad-delivery.example`: split on `.`, walk from the last label inward —
`example` → `ad-delivery` (collect `[0]`) → `cdn` (collect `[1]`) → `img` (no child, stop). Three
object lookups, early exit, suffix semantics for free because the walk only ever stops on a label
boundary. `notad-delivery.example` fails at step two and collects nothing, which is the behaviour
required in 3.4.

Cost: O(L) where L is the number of labels, capped at 24.

### 4.3 `AC` — the Aho–Corasick automaton

One automaton over all `label:` tokens, one over all `path:` values, built once per compile. Goto
edges are `Map<charCode, nodeId>`; failure links are built by breadth-first walk; output lists are
merged along failure links so a single pass reports every token that ends at each position.

Running it over a subject string of length *m* costs O(*m* + *h*) where *h* is the number of hits —
independent of how many patterns are in the automaton. That is the whole reason it is here: the
cost of a request does not grow with the size of the rule list.

Label tokens cannot contain `.`, so any match found in the host string is necessarily inside one
label. No boundary check is needed, which is a direct consequence of the lexical rule in 3.3.

### 4.4 Request normalisation

```
normalise(url) -> { scheme, host, pathq } | null
```

Lowercase the scheme and host. Strip a trailing `.` from the host and strip any `:port`. Leave
internationalised hosts in the punycode form the URL parser produces — rules are ASCII, so an IDN
rule is written in punycode and that is documented in the options help. `pathq` is
`pathname + search`, lowercased, fragment discarded. Return `null` for anything that is not
`http:` or `https:`; a `null` is an immediate pass.

### 4.5 Party

```
party(requestHost, siteHost) = sameParty ? first : third
sameParty(a, b) = a === b || a.endsWith("." + b) || b.endsWith("." + a)
```

No public suffix list. Shipping one would add hundreds of kilobytes and fetching one is forbidden,
so this design uses a label-boundary suffix test instead and states the cost openly:

- `cdn.example.com` vs `example.com` → same party. Correct.
- `tracker.example.net` vs `example.com` → third party. Correct.
- `a.example.github.io` vs `b.example.github.io` → third party. Correct.
- `evil.github.io` vs `github.io`, where the page genuinely is `github.io` → same party. Wrong,
  and the rare case this heuristic gets wrong.

Also honest: declarativeNetRequest's own `domainType: "thirdParty"` uses Chrome's real public
suffix list, so the network layer and the DOM fallback can disagree on this one edge. They are
never both live at once (section 8.3), so no request is decided by both.

### 4.6 The decision procedure

```
decide({ url, siteHost, type }) -> Decision
```

`Decision` is `{ verdict: "off" | "allow" | "block" | "pass", ruleIndex, line }`.

1. `n = normalise(url)`. If `null` → `pass`.
2. If `siteOff.walk(siteHost)` collects anything → `off`. Nothing else runs.
3. `p = party(n.host, siteHost)`.
4. Query the **allow** index. First candidate that survives the qualifier check → `allow`.
5. Query the **block** index. First candidate that survives the qualifier check → `block`.
6. Otherwise → `pass`.

Querying one index, in this order because it is cheapest first:

```
a. hosts.walk(n.host)        -> candidate indexes   O(L)
b. labels.run(n.host)        -> candidate indexes   O(|host| + hits)
c. paths.run(n.pathq)        -> candidate indexes   O(|pathq| + hits)
```

Each candidate is checked against its `Rule` record:

```
types & (1 << type)                          O(1)
rule.party === 0 || rule.party === p         O(1)
rule.except: none of them matches siteHost   O(16) worst case
rule.only:   one of them matches siteHost    O(16) worst case
```

`except` is tested before `only`, per 3.6. Host comparisons in `only`/`except` use the same
label-boundary suffix test as `host:`.

When more than one rule in a set matches, the one with the **lowest line number** wins. That is
deterministic, stable across recompiles, and gives the panel a line to show the user.

**Exception precedence is: `off` beats `allow` beats `block`.** It is positional in the algorithm,
not a scoring system — step 2 returns before step 4 can run, and step 4 returns before step 5. A
specific `block` can never override a broad `allow`; the user who wants that removes the allow.

Total cost per request: O(|url| + hits), with no term in *N*, the number of rules. Compile is
O(total pattern bytes) in time and memory. 2 000 rules compile to roughly 250–400 KB of automaton
in practice, held once in the service worker and once per content-script world in dom mode.

### 4.7 Compile steps

1. Split source into lines, tracking 1-based line numbers.
2. Per line: strip the trailing `\r`, skip blanks and `#` comments, reject tabs and any line over
   512 characters.
3. Split on runs of spaces into tokens. First token is the verb; second must contain `:`.
4. Validate verb/kind against the table in 3.2, then the value against the lexical rules in 3.3.
5. Parse remaining tokens in qualifier pairs; reject duplicates and unknown words.
6. Append a `Rule`; drop exact duplicates of an earlier line with a warning.
7. After all lines: bucket rules into the block, allow, cosmetic and siteOff structures; build the
   two suffix maps and the four automata; compute `sourceHash`.

Compiling the starter list and a 2 000-rule user list together must stay under 50 ms on the target
machine; the tests assert it (section 9.6).

## 5. Network layer — translating SRL to declarativeNetRequest

`lib/shield-dnr.js`, pure, tested in Node.

### 5.1 Which rules cross over

Only `block` and `allow` rules with kind `host`, `label` or `path`, plus `off` rules. `hide` rules
never become network rules.

### 5.2 Shape of each generated rule

| SRL                  | declarativeNetRequest condition                                                     |
| -------------------- | ----------------------------------------------------------------------------------- |
| `host:v`             | `requestDomains: ["v"]` — Chrome matches the domain and its subdomains, which is exactly the label-boundary suffix semantics of 3.4 |
| `path:v`             | `urlFilter: "v"`, `isUrlFilterCaseSensitive: false` — a plain substring, no pattern characters |
| `label:v`            | `regexFilter: "^https?://([a-z0-9-]+\\.)*[a-z0-9-]*" + esc(v) + "[a-z0-9-]*\\."`, `isUrlFilterCaseSensitive: false` |
| `kind a,b`           | `resourceTypes: ["…"]` using the map in 5.3                                          |
| `party third`        | `domainType: "thirdParty"`                                                           |
| `party first`        | `domainType: "firstParty"`                                                           |
| `party any`          | *(omit `domainType`)*                                                                |
| `only a,b`           | `initiatorDomains: ["a","b"]`                                                        |
| `except a,b`         | `excludedInitiatorDomains: ["a","b"]`                                                |
| `off site:v`         | `action: {type:"allowAllRequests"}`, `condition: {requestDomains:["v"], resourceTypes:["main_frame"]}` |

Note what is deliberately **not** used: no `||` or `^` anchoring syntax in `urlFilter`. Host rules
go through `requestDomains`, which is both exact and structured, and path rules are plain
substrings. The generated rules contain no pattern language beyond a RE2 regex for `label:`.

`allowAllRequests` on a `main_frame` request exempts that document and everything it loads, which
is precisely what "this site is never filtered" means.

### 5.3 Resource type map

| SRL      | declarativeNetRequest                        |
| -------- | -------------------------------------------- |
| `script` | `script`                                     |
| `image`  | `image`                                      |
| `frame`  | `sub_frame`                                  |
| `xhr`    | `xmlhttprequest`                             |
| `media`  | `media`                                      |
| `font`   | `font`                                       |
| `style`  | `stylesheet`                                 |
| `ping`   | `ping`, `csp_report`                         |
| `other`  | `object`, `websocket`, `webtransport`, `other` |

`main_frame` is never generated for a `block` rule. Bowser Shield does not block top-level
navigation; that is the native blocker's business and a surprising thing for an ad blocker to do.

### 5.4 Id allocation

Fixed bands, assigned by position so a rebuild is idempotent and so an id alone says what kind of
rule matched (section 8.2 depends on this):

| band            | contents                                   | source   | priority |
| --------------- | ------------------------------------------ | -------- | -------- |
| 999             | the engine probe rule (section 5.6)         | dynamic  | 1        |
| 1 000 – 1 999   | starter list **block** rules, file order    | static   | 1        |
| 2 000 – 2 999   | starter list **allow** rules, file order    | static   | 2        |
| 10 000 – 19 999 | user list **block** rules, file order       | dynamic  | 1        |
| 20 000 – 29 999 | user list **allow** rules, file order       | dynamic  | 2        |
| 30 000 – 30 499 | site exceptions (`off`), list order         | dynamic  | 3        |

`id = bandStart + indexWithinBand`. Nothing hashes, nothing increments across runs, so the same
list text always produces the same ids and a diff of two builds is readable.

Explicit priorities, even though declarativeNetRequest would already rank `allowAllRequests` above
`allow` above `block` at equal priority. Writing them out means the generated rules state the
precedence of section 4.6 rather than relying on it being inherited.

### 5.5 Applying rules, and the caps

Dynamic rules are written in one atomic call:

```js
const existing = await chrome.declarativeNetRequest.getDynamicRules();
await chrome.declarativeNetRequest.updateDynamicRules({
  removeRuleIds: existing.map(r => r.id),   // every id, not a remembered list
  addRules: next
});
```

Removing by what is actually there, rather than by what the extension thinks it wrote, means a
half-finished previous run cannot leave orphan rules behind.

The caps that matter, and what the design does about each:

| cap                                             | Chrome value        | this design                                                  |
| ----------------------------------------------- | ------------------- | ------------------------------------------------------------ |
| dynamic rules                                   | 30 000 from Chrome 120 (5 000 before) | budget fixed at **5 000**, the older and smaller number |
| "unsafe" dynamic rules                          | 5 000               | not reached — `block`, `allow` and `allowAllRequests` are all safe actions |
| regex rules (static and dynamic combined)       | 1 000               | tracked separately, see below                                 |
| enabled static rulesets                         | 50                  | one                                                           |
| rules in the static ruleset                     | 30 000 guaranteed   | 41                                                            |

The SRL format caps a list at 2 000 rules and 500 `off` entries, so a valid list can produce at
most 2 500 dynamic rules against a 5 000 budget. The cap is therefore unreachable through the
format — but the translator still enforces it, because a cap that is only honoured by arithmetic
in a different file is a cap waiting to be broken.

**When the budget is exceeded**, the translator keeps rules in file order until the budget is full,
drops the rest, and returns `{ rules, dropped: [{line, reason}] }`. `off` rules are allocated
first, then `allow`, then `block`, because dropping an exception is worse than dropping a block —
a dropped exception breaks a site, a dropped block shows an ad. The service worker stores the
dropped list in `shield.compiled.dropped` and the options page shows a banner: *"N rules are not
being enforced, starting at line L."* Nothing is silently discarded.

**The regex cap is separate and lower.** `label:` rules are the only ones that compile to regexes.
If a list contains more than 1 000 of them, the translator keeps the first 1 000 in file order and
reports the rest through the same `dropped` channel with reason `regex-cap`. There is no graceful
degradation for a dropped `label:` rule in network mode — the DOM scrubber is not running
(section 8.3) — so it simply does not apply, and the options page says so in those words.

### 5.6 Probing the engine

Microsoft documents that WebView2 loads unpacked extensions and that extensions using browser-UI
entry points load with those entry points missing. It does not publish a list of which `chrome.*`
APIs the WebView2 subset includes. Whether `declarativeNetRequest` dynamic and session rules work
in the current Edge 154-class runtime is therefore **not established by this design**. See
section 10.

So the service worker finds out at startup rather than assuming:

```js
async function probeEngine() {
  const dnr = chrome.declarativeNetRequest;
  if (!dnr || !dnr.updateDynamicRules || !dnr.getDynamicRules) return "dom";
  try {
    await dnr.updateDynamicRules({ removeRuleIds: [999], addRules: [PROBE_RULE] });
    const live = await dnr.getDynamicRules();
    await dnr.updateDynamicRules({ removeRuleIds: [999] });
    return live.some(r => r.id === 999) ? "network" : "dom";
  } catch {
    return "dom";
  }
}
```

`PROBE_RULE` is id 999, `action: {type: "block"}`, `condition: {requestDomains:
["probe.bowser-shield.invalid"], resourceTypes: ["image"]}`. `.invalid` is reserved by RFC 2606 and
never resolves, so the rule can never affect a real request even if the removal fails.

The probe runs on `runtime.onInstalled` and on each service-worker start where
`shield.engineMode` is absent, and the result is written to `shield.engineMode`. The options page
has a "Re-check" button that forces it again.

What the probe proves and what it does not: it proves the API exists and round-trips a rule. It
does **not** prove the runtime enforces the rule on a real request — an API that accepts and
returns rules while ignoring them is indistinguishable from a working one without issuing a
request, and this extension issues none. The options page states "Network blocking: API present,
enforcement unverified in this runtime" and gives the user a one-line manual check (open a page
with a known ad host, look at the count). Claiming more than that would be dishonest.

## 6. Cosmetic layer

Content script, both modes, every frame, `document_start`.

### 6.1 Building the CSS

The service worker compiles the user list and the starter list once per change and stores the
cosmetic result as plain data under `shield.cosmetic`:

```js
{ global: ["div[data-ad-slot]", …],
  byOnly: { "news.example": ["div[data-slot-state=\"empty\"]", …] },
  byExcept: [{ sel: "…", except: ["client.example"] }, …] }
```

Each selector becomes one CSS rule, never a comma-joined group, so a selector the engine rejects
takes only itself down:

```css
div[data-ad-slot]{display:none!important}
```

`display:none !important` and nothing else. No `visibility`, no collapsing of parents, no layout
rewriting — this layer hides, it does not redesign the page.

### 6.2 Injecting it

At `document_start` the content script creates `<style id="bowser-shield">`, appends it to
`document.documentElement` (which exists before `<head>` does) and leaves it empty. It then reads
`chrome.storage.local`, and when the data arrives adds each applicable rule with
`sheet.insertRule`. A selector the engine refuses throws; the script catches per rule, skips it,
and counts it in a `badSelectors` tally the options page shows.

The storage read is asynchronous, so there is a window — usually a few milliseconds, before first
paint on most loads — in which an ad slot is not yet hidden. On a slow start the user may see a
flash. There is no synchronous storage API available to a content script, so this design accepts
the flash and says so rather than pretending otherwise.

The script subscribes to `chrome.storage.onChanged` and rebuilds the sheet in place when the rules
change, so editing rules in Options updates open tabs without a reload.

### 6.3 Site exceptions and the top-level host

A content script in a cross-origin subframe cannot read the top document's host, and site
exceptions are defined against the **top-level** site. So the content script asks:

```js
const ctx = await chrome.runtime.sendMessage({ type: "frameContext" });
// -> { siteHost, enabled, mode, filtered }
```

The service worker answers from `sender.tab.url`, which it can read because of the host
permissions in 2.2. If `filtered` is false — the extension is off, or the top-level site matches an
`off` rule — the content script removes its `<style>` element, does not install the mutation
observer, and does nothing further on that page. The same message doubles as the signal that wakes
the service worker, which matters in section 10.

If the message fails entirely (no service worker), the content script falls back to reading
`chrome.storage.local` directly and uses `location.hostname` as the site host. In a subframe it
then declines to apply site exceptions, because it cannot verify them, and applies only the global
cosmetic rules. Degraded and documented, not silently wrong.

### 6.4 The dom-mode scrubber — the fallback for a missing network layer

**Only when `mode === "dom"`.** This is the fallback for a runtime where
`declarativeNetRequest` is absent or does not round-trip a rule.

The content script compiles the same SRL list with the same `shield-core.js` matcher, then:

1. Installs a `MutationObserver` on `document.documentElement` with `childList: true,
   subtree: true, attributes: true, attributeFilter: ["src", "href", "data-src"]`.
2. For each added element and each attribute change, resolves the URL-bearing attribute to an
   absolute URL — `img[src]`, `script[src]`, `iframe[src]`, `source[src]`, `embed[src]`,
   `object[data]`, `video[src]`, `audio[src]`, `track[src]`, and `link[href]` where `rel` is
   `preload`, `prefetch`, `preconnect` or `stylesheet`.
3. Infers the request type from the tag, calls `decide()`, and on a `block` verdict removes the
   attribute, sets `hidden`, and sets `style.display = "none"` inline.
4. Sweeps the existing tree once on `DOMContentLoaded` for anything inserted before the observer
   caught up.

**What this is, stated plainly.** It is suppression in the DOM, not blocking on the network. For an
element inserted by the HTML parser, the observer usually fires before the resource loader starts,
so the request often never happens. Often is not always: the request may already be in flight, and
for anything loaded by page script through `fetch`, `XMLHttpRequest`, `sendBeacon` or
`new Image()` this layer sees nothing at all, because a content script runs in an isolated world
and cannot intercept the page's own calls. Patching those APIs from a `world: "MAIN"` script was
considered and rejected: it would still not stop the request, it breaks sites, and it is trivially
detectable.

So the fallback's honest claim is: **third-party ad and tracker resources are stripped from the
page and hidden; some of them are prevented from loading as a side effect; none of it is network
blocking.** The panel labels these "hidden", never "blocked", and the options page says in one
sentence why the two words differ.

### 6.5 Which layer does what

| feature                                    | network mode              | dom mode                        |
| ------------------------------------------ | ------------------------- | ------------------------------- |
| stopping `block` requests                  | declarativeNetRequest     | not available                   |
| stripping `block` URLs from elements       | not needed                | content script (`shield-content.js`) |
| `allow` exceptions                         | declarativeNetRequest     | content script matcher          |
| `off` site exceptions                      | `allowAllRequests` rule **and** content script skip | content script skip |
| `hide` cosmetic rules                      | content script CSS        | content script CSS              |
| counting                                   | `onRuleMatchedDebug`      | content script reports          |
| the panel                                  | content script overlay    | content script overlay          |

## 7. The UI

### 7.1 The problem the UI has to solve

Two facts from section 0 collide with the normal shape of an extension UI:

- Microsoft documents that an extension with UI entry points — toolbar icon, badge, popup — loads
  in WebView2 **with those entry points missing**, because WebView2 has no browser chrome to host
  them. So `action.default_popup` may never be openable, and `action.setBadgeText` may never be
  visible.
- Bowser blocks user-initiated navigation to the `ms-browser-extension` scheme, so the user cannot
  type the options page URL either.

If the design relied on either, the extension would install and then be unreachable.

So the panel is implemented **once** in `ui/panel.js` and hosted **three** ways:

1. `ui/popup.html` — the normal `action` popup, for any host that does give it a button.
2. `ui/options.html` — the normal options page, via `options_ui` with `open_in_tab: true`.
3. **An in-page overlay**, which is the one that always works. The content script listens for
   **Alt+Shift+S** on `keydown` (its own listener, no `commands` permission, checked against
   `event.code === "KeyS" && event.altKey && event.shiftKey && !event.ctrlKey && !event.metaKey`,
   and ignored while focus is in an editable element). On trigger it attaches a closed shadow root
   to a fresh `<div>` on `document.documentElement`, injects `PANEL_CSS` from
   `content/panel-css.js`, and renders the same panel into it. Escape or a click outside closes it.
   The chord is configurable; the options view shows it.

A closed shadow root with its own stylesheet means page CSS cannot reach the panel and the panel's
CSS cannot reach the page.

### 7.2 Panel — summary view (what the popup shows)

```
Bowser Shield                                    [ On  ●——— ]
news.example

   37 blocked on this site
  124 blocked this session

  Network blocking: active
  [ Don't filter this site ]
  [ Settings… ]
```

States:

| state                          | shown as                                                                  |
| ------------------------------ | ------------------------------------------------------------------------- |
| on, network mode               | counts, "Network blocking: active"                                        |
| on, network mode, no feedback  | "Blocked count unavailable in this runtime" in place of the two counts     |
| on, dom mode                   | "N hidden on this site", "Network blocking unavailable — hiding in the page only" |
| off (`shield.enabled` false)   | toggle off, counts greyed, "Shield is off"                                 |
| site in `shield.siteOff`       | "This site is never filtered", button reads "Filter this site"             |
| non-http page                  | "Nothing to filter here", toggle still works                               |

The toggle writes `shield.enabled`. "Don't filter this site" adds `location.host` to
`shield.siteOff` and takes effect on the next load, with the panel saying so.

### 7.3 Panel — options view

Four sections, in this order:

1. **Status** — engine mode, rule counts, the "Re-check" button for the probe (5.6), and the
   enforcement caveat sentence.
2. **My rules** — a `<textarea>` with the raw SRL text, a character counter against the 256 KiB
   cap, and **Save**. On save the text is compiled in the page by `shield-core.js` for instant
   feedback; errors render underneath as `line N: message`, each clickable to put the cursor on
   that line. Saving with errors is allowed — the bad lines are skipped, exactly as section 3.6
   describes — but the error list stays visible. A **Format help** disclosure holds a compact copy
   of the tables in 3.2 and 3.3.
3. **Sites never filtered** — the `shield.siteOff` list with an add field and a remove button per
   row, capped at 500 and validated against the `site` lexical rule.
4. **Built-in starter list** — a checkbox for `shield.starterEnabled` and a read-only view of
   `rules/starter.srl` so the user can see every rule they are trusting.

Any save triggers the same pipeline: compile → translate → `updateDynamicRules` →
`updateEnabledRulesets` for the starter toggle → write `shield.cosmetic` → broadcast to content
scripts. A dropped-rules banner (5.5) appears above section 2 when `shield.compiled.dropped` is
non-empty.

### 7.4 Storage keys

All in `chrome.storage.local`, all prefixed `shield.`:

| key                   | type                                                   | default |
| --------------------- | ------------------------------------------------------ | ------- |
| `shield.version`      | number — schema version, currently 1                   | `1`     |
| `shield.enabled`      | boolean                                                | `true`  |
| `shield.starterEnabled` | boolean                                              | `true`  |
| `shield.userRules`    | string — raw SRL text, ≤ 256 KiB                       | `""`    |
| `shield.siteOff`      | string[] — lowercase hosts, ≤ 500                      | `[]`    |
| `shield.cosmetic`     | object — `{global, byOnly, byExcept}` (6.1)            | `{}`    |
| `shield.compiled`     | object — `{sourceHash, counts, errors, warnings, dropped}` | `{}` |
| `shield.engineMode`   | `"network" | "dom" | "unknown"`                        | `"unknown"` |
| `shield.panelChord`   | string — e.g. `"Alt+Shift+S"`                          | `"Alt+Shift+S"` |

On startup the service worker reads `shield.version`; a missing key means a first run and it writes
every default. A higher number than it knows means a downgrade, and it refuses to write rather than
corrupting a newer schema.

Nothing per-site and nothing per-URL is ever persisted. Counters live in service-worker memory
only, so the extension never builds a storage record shaped like browsing history.

## 8. Counting without double-counting

### 8.1 What is counted

Per tab: `{ siteHost, blocked, hidden }`, in a `Map<tabId, …>` in service-worker memory. Plus one
session total. Both reset to zero for a tab when `chrome.tabs.onUpdated` reports a committed
top-level navigation to a different host, and the entry is deleted on `chrome.tabs.onRemoved`.

### 8.2 Network mode

`chrome.declarativeNetRequest.onRuleMatchedDebug` fires once per request that matched a rule and
carries `{ rule: { ruleId, rulesetId }, request: { requestId, tabId, url, type } }`.

A matched rule is not always a *blocking* rule — an `allow` or `allowAllRequests` match fires the
same event. The id bands of 5.4 make the filter a range check:

```js
const isBlockRule = id => (id >= 1000 && id <= 1999) || (id >= 10000 && id <= 19999);
```

Only those increment. This is the payoff for allocating ids by band rather than by counter.

No deduplication is needed on top of that: a request has one `requestId` and declarativeNetRequest
reports one outcome for it. The handler still guards against a repeated `requestId` with a bounded
FIFO `Set` of the last 4 096 per tab, because an unverified runtime is exactly the place where a
"can't happen" turns into a doubled number.

### 8.3 Dom mode, and why the two layers never both count

The content script reports to the service worker in batches (coalesced on
`requestAnimationFrame`, flushed at most every 250 ms):

```js
chrome.runtime.sendMessage({ type: "hidden", urls: [...] });
```

Within a frame it keeps a `Set` of absolute URLs it has already acted on, capped at 2 048 entries
with oldest-first eviction, so the same ad URL appearing on two elements — or an attribute the page
rewrites repeatedly — counts once. The service worker adds the batch length to that tab's `hidden`.

The rule that makes the whole scheme safe: **exactly one layer is live at a time.** The service
worker probes once (5.6), writes `shield.engineMode`, and tells every content script which mode it
is in through the `frameContext` reply. In `network` mode the content script does cosmetic CSS and
nothing else — no observer, no attribute stripping, no reports. In `dom` mode no dynamic rules are
written, so `onRuleMatchedDebug` cannot fire. There is no configuration in which a single request
is counted by both layers, because there is no configuration in which both layers run.

### 8.4 When counting is not possible

If `onRuleMatchedDebug` is undefined — the permission was refused, or the WebView2 subset omits it
— the service worker sets `countsAvailable: false` and the panel shows "Blocked count unavailable
in this runtime" where the numbers go. It does not estimate, and it does not substitute the
cosmetic layer's numbers for the network layer's. A made-up count is worse than no count, because
the count is the only thing telling the user whether the extension works at all.

`chrome.action.setBadgeText` is called with the per-tab blocked count on every change, so the
number is right where a host does show a toolbar button. The design does not depend on it being
visible.

## 9. Test plan

`node --test extensions/bowser-shield/test/run-tests.js`. Node 18 or newer, `node:test` and
`node:assert` only, zero dependencies, no browser, no network, no adblock library of any kind. The
tests `require()` the shipped `lib/shield-core.js` and `lib/shield-dnr.js` — not a copy — so they
test what ships.

The implementer must satisfy all of the following before the extension is considered done.

### 9.1 Parser — `cases/parser.json`

Each case is `{ src, expect: { rules?, errors? } }`.

Must accept: every line of `rules/starter.srl`; every line of the example in 3.8; qualifiers in any
order; multiple spaces between tokens; `\r\n` endings; a trailing newline or none.

Must reject, with the right line number and a stable error code:

| input                                   | code                  |
| --------------------------------------- | --------------------- |
| `Block host:a.example`                  | `bad-verb`            |
| `block domain:a.example`                | `bad-kind`            |
| `block host:example`                    | `bad-host` (one label) |
| `block host:-a.example`                 | `bad-host`            |
| `block label:ad`                        | `label-too-short`     |
| `block label:1234`                      | `label-all-digits`    |
| `block path:ad-slot`                    | `path-no-slash`       |
| `hide css:div`                          | `css-too-broad`       |
| `hide css:*`                            | `css-too-broad`       |
| `hide css:div[a],span[b]`               | `css-comma`           |
| `hide css:div[a] > iframe`              | `css-space`           |
| `block css:div[a]`                      | `kind-not-allowed`    |
| `hide css:div[a] kind script`           | `qualifier-not-allowed` |
| `off site:a.example only b.example`     | `qualifier-not-allowed` |
| `block host:a.example party both`       | `bad-party`           |
| `block host:a.example kind script,bogus` | `bad-type`           |
| `block host:a.example only a,b,…(17)`   | `list-too-long`       |
| `block\thost:a.example`                 | `tab`                 |
| a 513-character line                     | `line-too-long`       |
| 2 001 rules                              | `too-many-rules`      |

And must prove the blast radius is narrow: a source of ten lines where line 4 is invalid compiles
nine rules and reports exactly one error.

### 9.2 Matcher — `cases/matcher.json`

Each case is `{ rules, url, site, type, expect }`.

The ones that must be present, because each is a place this design could be got wrong:

- `host:ad-delivery.example` matches `https://ad-delivery.example/x` and
  `https://img.ad-delivery.example/x`; does **not** match `https://notad-delivery.example/x`,
  `https://ad-delivery.example.co/x`, or `https://addelivery.example/x`.
- `label:click-track` matches `https://eu.click-tracker.example/a`; does not match
  `https://click.track.example/a` (the token straddles a dot and therefore cannot match).
- `path:/ad-slot/` matches `https://x.example/AD-SLOT/1` (case-insensitive) and
  `https://x.example/a?u=/ad-slot/`; does not match `https://x.example/a#/ad-slot/` (fragment).
- Party: default `third` means `block host:a.example` does not fire on a page at `a.example`;
  `party any` means it does.
- `kind image` does not fire for a `script` request.
- `only` and `except` honour label-boundary suffixes: `only shop.example` fires on
  `www.shop.example`, not on `shop.example.net`.
- Precedence: a list with `off site:s.example`, `allow …`, `block …` all matching returns `off`;
  remove the `off` and it returns `allow`; remove the `allow` and it returns `block`.
- Lowest line number wins among several matching rules in the same set.
- Non-http schemes (`data:`, `blob:`, `about:`, `ms-browser-extension:`) return `pass`.
- A host with 25 labels returns `pass` rather than walking.

### 9.3 Differential test against a naive oracle

`test/reference-matcher.js` implements the same semantics in the dumbest possible way: keep the
rules in an array, and for each request loop over every rule in order doing string comparisons. It
is obviously correct and obviously slow, and it exists only to be disagreed with.

The test generates 500 pseudo-random rule lists (seeded, so a failure reproduces) of up to 300
rules drawn from the full grammar, and 200 pseudo-random URLs per list, and asserts that
`ShieldCore.decide()` and the reference return the **same verdict and the same rule line** for
every pair. This is the test that catches a bad failure link in the automaton or an off-by-one in
the suffix walk, which no hand-written case table reliably finds.

### 9.4 Translation — `cases/dnr.json`

- Every generated rule validates against a schema check written in the test: integer `id`, integer
  `priority`, an `action.type` from the allowed set, a `condition` with at least one of
  `requestDomains`, `urlFilter`, `regexFilter`.
- Ids land in the band from 5.4 that matches the rule's verb and source, and ids are unique.
- The generated `regexFilter` for `label:v` matches a host containing `v` in a label and not one
  where `v` straddles a dot — asserted by running the regex in Node against both, which is a real
  check because RE2 and JavaScript agree on this subset.
- `only`/`except` land in `initiatorDomains`/`excludedInitiatorDomains`; `party` maps to
  `domainType`; `kind` maps through the table in 5.3.
- `off site:v` produces exactly one `allowAllRequests` rule with `resourceTypes: ["main_frame"]`.
- Determinism: translating the same source twice is deep-equal.

### 9.5 Cap behaviour

- A synthetic list of 6 000 translatable rules (built in the test, bypassing the format cap, to
  exercise the budget directly) produces exactly 5 000 rules and a `dropped` array of 1 000 with
  ascending line numbers.
- Allocation order under pressure: a list whose `off` and `allow` rules come last in the file still
  keeps all of them and drops `block` rules.
- 1 100 `label:` rules produce 1 000 regex rules and 100 entries in `dropped` with reason
  `regex-cap`.
- Translating `rules/starter.srl` deep-equals the committed `rules/starter-dnr.json`. This is the
  test that stops the static ruleset drifting from the text.

### 9.6 Compile budget

Compile the starter list plus a generated 2 000-rule list and assert it completes in under 50 ms,
and that 10 000 `decide()` calls over a fixed URL set complete in under 50 ms. These are loose
bounds; they exist to catch an accidental O(N) scan per request, which is the specific failure this
design's data structures exist to prevent.

### 9.7 Not tested here

The service worker, the content script and the UI are not covered by these tests — they need a
browser. Their behaviour is verified by hand against the checklist in 7.2's state table, in Bowser
and in desktop Chrome, and the result is written up the way `docs/extensions.md` writes up its own
verification status: what was checked, what was not, and what is still unknown.

## 10. WebView2 risk, stated plainly

What is established: Microsoft documents that WebView2 loads unpacked extension folders through
`ICoreWebView2Profile7::AddBrowserExtension`, and documents that an extension with UI entry points
— icon, badge, popup — loads with **those entry points missing**, because WebView2 has no browser
UI to host them.

What is not established: Microsoft does not publish a list of which `chrome.*` APIs the WebView2
subset includes. This design did not find official documentation either confirming or denying that
`declarativeNetRequest` works in WebView2, and specifically found nothing about dynamic rules
(`updateDynamicRules`) or session rules. The 154-class runtime on the development machine was not
exercised. **Treat network-level blocking in WebView2 as unverified.**

The design's answers, in order of how much they matter:

1. **The popup and options page may be unreachable.** No toolbar button means no popup; the blocked
   `ms-browser-extension` scheme means the user cannot navigate to the options page. Answer: the
   in-page overlay of 7.1, reached by a keyboard chord from the content script, is the primary UI,
   and the popup and options page are the secondary ones. The extension is fully usable with
   neither.
2. **Dynamic rules may not work.** Answer: the runtime probe of 5.6, and the dom-mode fallback of
   6.4 — honestly labelled, since it hides and strips rather than blocks.
3. **The static ruleset may still work when the service worker does not.** Static rules are applied
   from the manifest by the browser. Shipping the starter list as a static ruleset means a runtime
   with no working service worker still blocks the starter set. That is why the starter list exists
   in both `.srl` and `-dnr.json` form.
4. **The service worker may not start on its own.** Answer: the content script's `frameContext`
   message on every page load is also the wake-up call, and if messaging fails entirely the content
   script reads `chrome.storage.local` directly and runs the dom-mode path by itself. Nothing in
   the critical path requires the service worker to have run first.
5. **`onRuleMatchedDebug` may be absent.** Answer: section 8.4 — show "unavailable", never a guess.
6. **Session rules are not used at all.** Not because they are known to be missing, but because
   nothing here needs them: the rules should survive a restart, which is what dynamic rules are
   for. One fewer unverified API in the critical path.

An implementer who finds that WebView2 does enforce dynamic rules should record it in
`docs/extensions.md` under the existing "Uncertainty, stated plainly" heading, with the runtime
version it was checked against.

## 11. Non-goals

- **Not a replacement for Bowser's native blocker.** Different layer, different rules, different
  code. It does not read, write, extend or coordinate with it, and the two may both be on.
- **No remote filter lists.** No fetch, no subscription, no update check, no "import from URL". The
  user pastes rules or uses the starter file. This is a deliberate limit on how much of the web an
  extension with host permissions is allowed to talk to.
- **No compatibility with any published filter syntax.** SRL does not parse, import or convert
  rules from any other blocker, and there is no plan to add it.
- **No telemetry, no analytics, no crash reporting, no remote anything.** The only storage is
  `chrome.storage.local`, and nothing per-site or per-URL is persisted to it.
- **No scriptlet injection, no page-script patching, no anti-adblock circumvention.** The dom-mode
  fallback is a DOM scrubber and nothing more.
- **No cookie, header, redirect or CSP modification.** Only `block`, `allow` and `allowAllRequests`
  actions are ever generated, which also keeps every rule inside the "safe" dynamic-rule class.
- **No top-level navigation blocking.** `main_frame` never appears in a generated block rule.
- **No element picker, no per-element "block this" UI.** Rules are typed; the surface stays small
  enough to read.
- **No regex, wildcard or pattern language in SRL.** Host, label, path, selector, site — five
  kinds, no pattern characters. The RE2 regexes generated for `label:` are an implementation detail
  of the translation and are never written by a user.
- **No sync storage, no profiles, no rule-list sharing.**
- **No Manifest V2 variant.**
