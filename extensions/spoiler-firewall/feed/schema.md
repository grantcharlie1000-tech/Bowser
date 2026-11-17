# Term-update feed: format 1

A data-only, signed list of extra spoiler terms. Both hosts (the Chrome extension and Bowser) download the same file,
verify it, and hand its `lexicon` to `SFW.lexicon.extend`, which can only **add** terms or **raise** a term's
category. A feed can never remove a term, lower a weight, add an "this is safe" phrase or run code.

## Wire format: one envelope file

`feed.json` (published at `extensions/spoiler-firewall/feed/published/feed.json`):

```json
{ "format": 1, "payload": "<base64>", "sig": "<base64>" }
```

* `payload` is the standard, padded base64 of the **exact UTF-8 bytes** of the payload document below.
* `sig` is the base64 of a 64-byte detached Ed25519 signature (RFC 8032) over those same bytes.
* Clients verify the signature over the decoded payload bytes and only then parse them. Nothing is re-serialised
  before it is checked, so there is no canonicalisation to get wrong.

### Why one envelope and not `feed.json` + `feed.json.sig`

`raw.githubusercontent.com` serves with `Cache-Control: max-age=300` and two files are fetched as two requests. For
up to five minutes after a publish a client can receive the new `feed.json` with the old `feed.json.sig` (or the
reverse) and would reject a perfectly good update, or worse, flip between states. One file is atomic: it is
either the old feed or the new one. It also halves the requests. The cost is that the payload is not human-readable
in the published file; reviewers read `feed/source/lexicon.json` (plain JSON, in git history) and `verify.mjs`
decodes the published file.

## Payload (what is signed)

```json
{
  "format": 1,
  "seq": 7,
  "issued": "2026-11-18",
  "expires": "2026-12-18",
  "lexicon": {
    "version": "f7",
    "add":   { "entityStrong": ["some new place"], "cueStrong": ["some new cue"] },
    "raise": { "some weak term": "entityStrong" }
  }
}
```

| Field | Rule |
|---|---|
| `format` | The number `1`. Anything else is rejected ("format"); a future format 2 gets a new client first. |
| `seq` | Integer 1 to 2147483647, strictly increasing with every publish. |
| `issued`, `expires` | `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM[:SS[.fff]](Z\|+HH:MM)`, valid calendar dates and clock times, `expires >= issued`. A bare `expires` date lasts through that UTC day. |
| `lexicon` | Object with only the keys `version`, `add`, `raise`. |
| `lexicon.version` | Optional, `[A-Za-z0-9_.+-]`, at most 40 characters. `sign.mjs` sets `f<seq>` when absent. Shown by the engine as `<built-in>+<version>`. |
| `lexicon.add` | Object. Keys are categories: `franchise`, `entityStrong`, `entityWeak`, `cueStrong`, `cueWeak`, `marker`, `game`, `shapeVerb`, `shapeEnd`. Each value is an array of strings. Any other key (for example `safe`, `negative`, `anti`) rejects the whole feed. |
| `lexicon.raise` | Object. Keys are terms, values are the new category: `entityWeak`, `entityStrong`, `franchise`, `cueWeak`, `cueStrong`, `marker`. The engine accepts a raise only to a category above the term's current one in the same family, so a "raise" cannot demote. |
| terms | Strings of 2 to 64 characters with no control characters (the engine drops, and `sign.mjs` refuses, anything else). They go through the same normalisation as page text, so case, accents and look-alike letters do not need separate entries. |
| size | At most 5000 terms (adds plus raises) and 200000 bytes of lexicon JSON, both `SFW.lexicon.limits`. The whole envelope is at most 262144 bytes (256 KiB), which in practice limits the payload to about 196 KB. |

`SFW.lexicon.extend` stays the single point where terms enter the engine. `sign.mjs` loads the built engine and
refuses to sign a lexicon that `extend` does not take in full.

## Client rules

Checks run in this order and the first failure stops the pipeline: body size, envelope shape, base64, signature
(any trusted key), payload JSON, schema, rollback. Failures are ignored with a visible status; they never change
what is stored and never turn protection off.

* **Trust.** The public keys in `feed/public-key.txt` (one base64 raw Ed25519 key per line) are compiled into both hosts. A feed is accepted when any one of them verifies it; that is how keys are rotated (see `README.md`).
* **Rollback.** Each client remembers the highest `seq` it accepted. A lower `seq` is rejected ("rollback"). The same `seq` is a no-op. A client that lost its storage starts at 0.
* **Expiry.** An expired feed is **still applied**: it can only add protection, and silently dropping terms because a maintainer was late would be the unsafe direction. The UI says "Term updates out of date." so the maintainer's lapse is visible. Re-publishing with a later `expires` (and a higher `seq`) clears it.
* **Rejected** ("Term update rejected."): wrong key, altered bytes, unsigned, malformed, oversize, rolled back. The last good feed and the built-in list keep working.
* **Unavailable** (offline, 5xx, timeout): "Could not check for term updates." **404** is quiet ("No term update has been published yet."): it is the expected state until the first publish.
* **Off.** The switch ("Download spoiler term updates", Chrome `feedEnabled`, Bowser `spoiler_feed`) stops downloads. A feed already held stays in force; it only adds protection.

## Reference implementations

| | |
|---|---|
| Chrome | `hosts/chrome/feed.js` (verify, status words), `feed-update.js` (fetch, store, schedule), vendored `@noble/ed25519` |
| Bowser | `src/spoiler_feed.rs` (verify with `ed25519-compact`, WinHTTP fetch, store, schedule), `spoiler.rs` (config), `hosts/bowser/bowser-host.js` |
| Tooling | `feed/tools/keygen.mjs`, `sign.mjs`, `verify.mjs` (signing uses `node:crypto`, an independent Ed25519 implementation) |

Test vectors: `feed/sample/feed.json` is signed with the throwaway test key (`feed/test-keys/`); the Rust and Node
tests both verify it.
