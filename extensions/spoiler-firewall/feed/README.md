# Signed term-update feed: maintainer guide

Why this exists: the bundled term list cannot know mission names or late-game characters before GTA VI launches,
and Chrome Web Store review (and a Bowser release) is slower than the news. This folder is the data-only channel
that closes the gap. A feed can only **add** terms or **raise** a term's weight; it is signed, never code, and
clients ignore anything that fails verification. Format: `schema.md`.

```
feed/
  schema.md          the format (read this first)
  public-key.txt     production PUBLIC keys, compiled into the extension and Bowser
  source/lexicon.json  the lexicon you edit (starts empty)
  published/feed.json  the signed file clients download (created by sign.mjs; YOU commit it; see Publishing)
  tools/             keygen.mjs, sign.mjs, verify.mjs (+ lib.mjs, vendor-noble.mjs)
  sample/            a unit-test feed signed with the throwaway test key
  test-keys/         TEST-ONLY key pair. Never trusted by a shipped build.
```

The private signing key is **not in the repository**. `keygen.mjs` wrote it to
`%USERPROFILE%\.spoiler-firewall\feed-signing-key.pem`. Back it up privately. Nothing in this repo ever reads or
copies it except `sign.mjs`, which reads it to sign.

## Who edits terms

One maintainer edits `source/lexicon.json`, reviews the diff, signs, and commits the signed file. The maintainer
decides what counts as a spoiler term. Finding the new names without reading spoilers yourself is the maintainer's
problem: use a trusted second person or a source that lists names only, and enter terms exactly as they would
appear in text. (Nothing in this tooling reads the web or any spoiler source.)

## Editing the lexicon

`source/lexicon.json` is the `lexicon` object of the payload:

```json
{
  "add": {
    "entityStrong": ["a new place that only exists in this game"],
    "cueStrong": ["a phrase that only appears in plot talk"]
  },
  "raise": {
    "a name that was only a weak signal": "entityStrong"
  }
}
```

Categories and limits are in `schema.md`. Rules of thumb:

* `entityStrong` / `franchise`: names that identify the game on their own (a unique place or character). They make
  text about them count as GTA VI. Do **not** use a common word here: a term like "port" would cover half the web.
* `entityWeak`: ambiguous names; they only count next to other GTA signals.
* `cueStrong` / `cueWeak` / `marker`: spoiler-ish wording. Same caution: strong cues cover text on their own.
* `raise` lifts a term the engine already knows to a higher category in the same family
  (`entityWeak` to `entityStrong`, `cueWeak` to `cueStrong`). It cannot lower anything; the engine refuses a demotion,
  and `sign.mjs` refuses to sign a feed containing one.
* Terms are 2 to 64 characters. Case, accents and look-alike letters are normalised by the engine.
* A bad term over-hides (annoying) but can never under-hide. Still, test before publishing.

## Signing

```
node build/build.mjs                       # sign.mjs checks terms with the BUILT engine (dist/node/engine.cjs)
node feed/tools/sign.mjs                   # reads source/lexicon.json, signs with the default key, writes published/feed.json
node feed/tools/verify.mjs feed/published/feed.json   # verifies it the way a client does (production keys)
```

`sign.mjs` options: `--lexicon <file>`, `--key <pem>`, `--out <file>`, `--seq N`, `--issued YYYY-MM-DD`,
`--expires YYYY-MM-DD` or `--expires-days 30` (default 30), `--prev <feed.json>`.

What `sign.mjs` guarantees before it writes anything:

1. the payload satisfies the schema (`hosts/chrome/feed.js`, the code clients run);
2. the **built engine's `SFW.lexicon.extend` accepts every entry** (no rejected term, nothing demoted);
3. `seq` is higher than the one in the file it is about to replace (default: previous + 1; it refuses anything not
   higher, because clients would ignore it);
4. the signed file is under 256 KiB, and verifies with the shipped client verifier and with `node:crypto`.

Set `expires` to a date you will actually refresh: clients still apply an expired feed, but they show "Term updates
out of date." Re-run `sign.mjs` (it bumps `seq`) to refresh without changing terms.

## Publishing (deliberately a manual, reviewed step)

Nothing here publishes. To publish: commit `feed/published/feed.json` (and `source/lexicon.json`) to `main` of
`grantcharlie1000-tech/Bowser`. Clients fetch
`https://raw.githubusercontent.com/grantcharlie1000-tech/Bowser/main/extensions/spoiler-firewall/feed/published/feed.json`
at startup and every 12 hours. GitHub's CDN caches that file for about 5 minutes, so expect up to 12 h 5 min before a
given client sees it (and an immediate refresh on browser or Bowser restart). Until the file exists the URL answers
404 and clients say "No term update has been published yet." and carry on.

Dry run without publishing: `node feed/tools/sign.mjs --out %TEMP%\\feed.json`, then `verify.mjs` on that file.

Rolling back a bad term: edit `source/lexicon.json`, sign again (higher `seq`), commit. Do **not** commit an older
signed file: clients reject a lower `seq` as a rollback attack. A corrected feed replaces the old term set completely
(the stored feed is replaced, not merged) for every page a client loads after it downloads it; pages that were
already open keep the terms they started with.

## Key rotation

Clients trust every key in `public-key.txt` (one base64 key per line), so rotation needs no flag day:

1. `node feed/tools/keygen.mjs --out <new path outside the repo>` and add the printed public key as a second line of
   `public-key.txt`. Update `hosts/chrome/feed-config.js` `PUBLIC_KEYS` to match (a unit test fails if they differ).
2. Ship a Chrome extension release and a Bowser release trusting old + new.
3. Once most users have updated, sign with the new key (`sign.mjs --key <new>`).
4. In a later release, delete the old key line from both.

Until step 2 has reached users, a feed signed with only the new key is rejected as "Term update rejected." by old
clients, which is safe (they keep the built-in list and their last feed).

## Emergency procedure

* **Bad terms published (over-hiding).** Fix `source/lexicon.json`, sign (seq bumps), commit. Clients replace the feed
  within 12 hours. Users can switch updates off and keep protection (their stored feed stays; it can only add).
* **Signing key leaked or lost.** A leaked key lets its holder add terms for every user (they cannot remove anything or
  run code, so the worst case is over-hiding, or a huge `seq` that blocks future honest feeds). Rotate: generate a new
  key, ship a client release that trusts it and drops the old one (see Key rotation), then sign with it. A leaked key
  that also signed a very large `seq` needs a client release that raises the minimum accepted `seq`
  (`MAX_SEQ`/stored seq are the only state); until then the old holder cannot be outvoted by a lower `seq`.
  Keep `seq` modest and sequential so this stays recoverable.
* **Kill switch.** There is no remote "turn off protection" and no remote code, by design. The only server-side
  action is publishing a new honest feed.
* **GitHub unreachable or the file deleted.** Clients show a quiet status and keep working with the built-in list plus
  the last feed they held.

## Test-only pieces

`test-keys/` holds a throwaway key committed on purpose so tests can sign. It is never listed in `public-key.txt`
and never in a build. The Chrome extension honours a `feedDev` override (URL + key) **only** when it is an unpacked
build (its manifest has no `update_url`, which every Web Store install has) and only for an `http://127.0.0.1` or
`http://localhost` URL; it lives in `chrome.storage.local`, which web pages cannot write, and the content scripts
never write it. `test/chrome/feed-extension.mjs` uses it against a local server.

## Tests

```
node --test test/unit/feed-verify.test.mjs test/unit/feed-engine.test.mjs test/unit/feed-update.test.mjs test/unit/feed-config.test.mjs
node test/chrome/feed-extension.mjs        # real Chromium, headless, muted, local server only
cargo test -j 2 spoiler                    # Bowser side (a BelowNormal process on a busy machine)
```
