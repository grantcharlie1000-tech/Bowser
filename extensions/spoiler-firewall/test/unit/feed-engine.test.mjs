// What a verified feed can do to the engine: only ADD protection. SFW.lexicon.extend mutates process state, so
// every scenario loads a fresh engine instance (loadEngine) and nothing here shares state with other test files.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { checkLexiconWithEngine, FEED_DIR, loadClient, loadEngine } from "../../feed/tools/lib.mjs";

const TEXTS = [
  "GTA 6 trailer 3 release date and price",
  "Lucia Caminos dies at the end of the game",
  "who dies in gta 6 ending explained",
  "Saint Lucia beach holiday dies down after the storm",
  "Jason Voorhees returns in a new friday the 13th film",
  "leaked story details for the new rockstar game surfaced overnight",
  "Greater Toronto Area housing market update, GTA condo prices",
  "no spoilers please but I cannot wait for the launch",
  "Brian May plays the final show of the tour",
  "the plot of this movie is a twist",
  "Duval spotted near Leonida Keys, this is the final mission",
  "grand theft auto vi preorder bonus and file size",
  "the cat sat on the mat and nothing happened",
  "sfw sample entity appears in the sample cue",
  "ネタバレ注意 GTA6 の結末",
  "в GTA 6 главный герой погибает в финале",
  "Vice City is back, rockstar confirms the date",
  "download size 150 GB, pre-load starts midnight",
];

const rank = { none: 0, risk: 1, mention: 2, spoiler: 3 };
const snapshot = (SFW) => TEXTS.map((t) => { const r = SFW.classify.text(t); return { score: r.score, level: r.level }; });

function never_lower(before, after, label) {
  before.forEach((b, i) => {
    assert.ok(after[i].score >= b.score - 1e-9, `${label}: "${TEXTS[i]}" score fell ${b.score} -> ${after[i].score}`);
    assert.ok(rank[after[i].level] >= rank[b.level], `${label}: "${TEXTS[i]}" level fell ${b.level} -> ${after[i].level}`);
  });
}

test("the committed sample feed is accepted by the built engine and its terms start matching", () => {
  const payload = JSON.parse(Buffer.from(JSON.parse(readFileSync(join(FEED_DIR, "sample", "feed.json"), "utf8")).payload, "base64").toString());
  const SFW = loadEngine();
  assert.equal(SFW.classify.text("zz sfw sample entity here").flags.entity, false, "unknown before the feed");
  const res = SFW.lexicon.extend(payload.lexicon);
  assert.deepEqual([res.ok, res.added, res.rejected], [true, 4, 0]);
  assert.equal(SFW.classify.text("zz sfw sample entity here").flags.entity, true);
  assert.ok(SFW.lexicon.version.endsWith("+f1"), SFW.lexicon.version);
});

test("applying feeds never lowers any classification (adds, raises, repeats, junk, loosening attempts)", () => {
  const feeds = [
    { add: { cueStrong: ["ending", "the", "cat", "dies", "mat"], cueWeak: ["launch", "final", "plot"] } },
    { add: { entityStrong: ["lucia", "duval", "jason", "brian may", "saint lucia"], entityWeak: ["rockstar", "cal"] } },
    { add: { franchise: ["rockstar confirms", "the new rockstar game"], marker: ["no spoilers please"], game: ["tour", "film"] } },
    { add: { shapeVerb: ["appears", "sat"], shapeEnd: ["of the tour", "the date"] } },
    { raise: { duval: "entityStrong", launch: "cueStrong", "vice city": "franchise", cal: "entityStrong", "release date": "cueStrong" } },
    // attempts to loosen: engine ignores unknown categories, rejects demotions
    { add: { safe: ["release date", "price", "trailer"], negative: ["lucia caminos", "gta 6"], anti: ["ending"], fuzzyStop: ["leonida"], weakOrg: [] } },
    { raise: { "lucia caminos": "entityWeak", "gta 6": "entityWeak", spoiler: "cueWeak", "who dies": "cueWeak" } },
    { remove: ["spoiler", "gta"], add: { cueStrong: [] } },
    { add: { cueStrong: [5, null, {}, "", "x", "bad\u0000term", "y".repeat(100)] } },
    null, "x", [], 5,
  ];
  for (const feed of feeds) {
    const SFW = loadEngine();
    const before = snapshot(SFW);
    SFW.lexicon.extend(feed);
    never_lower(before, snapshot(SFW), JSON.stringify(feed).slice(0, 70));
  }
  // cumulative: all feeds one after another into one engine, then again (repeat feeds are harmless)
  const SFW = loadEngine();
  let prev = snapshot(SFW);
  for (const feed of [...feeds, ...feeds]) {
    SFW.lexicon.extend(feed);
    const now = snapshot(SFW);
    never_lower(prev, now, "cumulative");
    prev = now;
  }
});

test("loosening feeds are inert: nothing from the engine's own safety data changes", () => {
  const SFW = loadEngine();
  const data = JSON.stringify(SFW.lexicon.data);
  SFW.lexicon.extend({ add: { safe: ["lucia caminos"], negative: ["gta 6>gta"], anti: ["spoiler"], fuzzyStop: ["leonida"], unitWords: ["gta"] }, raise: { spoiler: "cueWeak" } });
  assert.equal(JSON.stringify(SFW.lexicon.data), data, "the bundled word lists are not touched by extend");
  assert.equal(SFW.classify.text("Lucia Caminos dies at the end").level, "spoiler");
  assert.equal(SFW.classify.text("GTA 6 spoiler ending").level, "spoiler");
});

test("a feed can make a new name count: a weak mention becomes a shielded one", () => {
  const SFW = loadEngine();
  const text = "everyone is talking about harbor master sloane in the new gta";
  const before = SFW.classify.text(text);
  SFW.lexicon.extend({ add: { entityStrong: ["harbor master sloane"] } });
  const after = SFW.classify.text(text);
  assert.ok(after.score >= before.score);
  assert.equal(after.flags.entity, true);
  assert.equal(before.flags.entity, false);
});

test("the checks sign.mjs relies on: the engine reports what it took and what it refused", () => {
  assert.deepEqual(checkLexiconWithEngine({ add: { cueStrong: ["zz fresh cue"] } }).errors, []);
  assert.equal(checkLexiconWithEngine({ add: { cueStrong: ["zz fresh cue"] } }).added, 1);
  assert.match(checkLexiconWithEngine({ add: { cueStrong: ["x"] } }).errors.join(), /rejected 1 entry/);
  assert.match(checkLexiconWithEngine({ raise: { spoiler: "cueWeak" } }).errors.join(), /rejected/);
  assert.match(checkLexiconWithEngine({ add: {}, raise: {} }).warnings.join(), /added nothing/);
  const tooMany = { add: { cueWeak: Array.from({ length: 5001 }, (_, i) => "zz term " + i) } };
  assert.match(checkLexiconWithEngine(tooMany).errors.join(), /rejected the lexicon/);
});

test("the client schema and the engine agree on limits (a feed the client accepts is one the engine takes)", () => {
  const SFW = loadEngine();
  const { SFWFeed } = loadClient();
  assert.equal(SFWFeed.MAX_LEXICON_JSON, SFW.lexicon.limits.maxJson);
  assert.equal(SFWFeed.MAX_TERMS, SFW.lexicon.limits.maxTerms);
});
