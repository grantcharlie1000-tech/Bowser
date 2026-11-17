import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const SFW = createRequire(import.meta.url)("../../dist/node/engine.cjs");
const C = SFW.classify;
const f = (t, ctx) => C.text(t, ctx).flags;

test("contract shape: score, level, signals (kind/source only), flags", () => {
  const r = C.text("GTA 6 ending: Jason dies in the final mission", { source: "title" });
  assert.equal(r.level, "spoiler");
  assert.ok(r.score >= 0.9 && r.score <= 1);
  assert.deepEqual(Object.keys(r.flags).sort(), ["cue", "cueWeak", "entity", "entityWeak", "franchise", "marker", "safe"]);
  for (const s of r.signals) {
    assert.deepEqual(Object.keys(s).sort(), ["kind", "source"]);
    assert.equal(s.source, "title");
  }
  // signals never contain matched text
  assert.ok(!JSON.stringify(r).toLowerCase().includes("jason"));
  assert.ok(!JSON.stringify(r).toLowerCase().includes("gta"));
  assert.equal(C.text("", {}).level, "none");
  assert.equal(C.text(null).level, "none");
});

test("ordinary text is rejected and flags nothing", () => {
  for (const t of ["lunch was great today", "the quarterly report shows three shipped features", "anyone know a good pizza place nearby?"]) {
    const r = C.text(t);
    assert.equal(r.level, "none");
    assert.ok(Object.values(r.flags).every((x) => x === false));
  }
});

test("franchise_strong: forms, numerals and languages", () => {
  const yes = [
    "GTA 6", "gta6", "GTA VI", "gtavi", "GTA Six", "Grand Theft Auto VI", "grand theft auto 6", "Grand Theft Auto: VI", "GTA-6", "gta_6", "the new GTA",
    "Vice City 2", "GTA sex", "#gta6", "#GTAVI", "GTA\u00a06", "\u0413\u0422\u0410 6", "\u30b0\u30e9\u30bb\u30d56", "\u30b0\u30e9\u30f3\u30c9\u30fb\u30bb\u30d5\u30c8\u30fb\u30aa\u30fc\u30c8VI",
    "\u4fa0\u76d7\u730e\u8f66\u624b6", "\u4fa0\u76d7\u730e\u8f66\u624b\u516d", "\u4fa0\u76d7\u730e\u8f66\u624b6", "\uc9c0\ud0c06", "\u062c\u064a \u062a\u064a \u0627\u064a 6", "\u0e08\u0e35\u0e17\u0e35\u0e40\u0e2d 6", "GTA seis", "GTA sechs", "gta alti",
    "GTA 6'ya", "grand theft auto six", "Grand Theft Auto VI'yi",
  ];
  for (const t of yes) assert.equal(f(t + " is out").franchise, true, t);
  const no = ["GTA V", "GTA 5", "GTA IV", "Grand Theft Auto V", "GTA San Andreas", "GTA Vice City", "GTA 60", "gta6000", "I love GTA. 6 hours straight", "GTA 6 hours"];
  for (const t of no) assert.equal(f(t).franchise, false, t);
});

test("evasion classes: homoglyph, zero-width, leet, spacing, punctuation, hashtag, emoji, wildcard", () => {
  const cases = {
    homoglyph: "gt\u0430 6 ending", // Cyrillic a
    greek: "g\u03c4\u03b1 6 ending",
    zerowidth: "g\u200bt\u200ba 6 ending",
    softhyphen: "gt\u00ada 6 leak",
    leet: "g7a 6 l34k",
    leetVI: "gta v1 ending",
    spaced: "G T A 6 ending",
    spacedVI: "G T A V I ending",
    dots: "G.T.A 6 ending",
    dotsEllipsis: "G...T...A...6 ending",
    dashes: "G-T-A 6 ending",
    hashtag: "#GtaSixEnding",
    hashtag2: "#gta6leaks",
    camel: "#GTAVIEnding",
    emoji: "\u{1f176}\u{1f183}\u{1f170} 6 ending",
    regional: "\u{1f1ec}\u{1f1f9}\u{1f1e6} 6 ending",
    squash: "gtaaaa 66 ending",
    censor: "g*a 6 ending",
    censor2: "G_A 6 ending",
    mixed: "gTa vI eNdInG",
    fullwidth: "\uff27\uff34\uff21 \uff16 ending",
    keycap: "GTA 6\ufe0f\u20e3 ending",
  };
  for (const [k, t] of Object.entries(cases)) {
    const r = f(t);
    assert.equal(r.franchise, true, k + ": " + JSON.stringify(t));
  }
  // franchise + cue => spoiler
  for (const t of ["g7a 6 l34k", "G T A 6 ending", "#GtaSixEnding", "g*a 6 ending", "gtaaaa 66 ending"]) assert.equal(C.text(t).level, "spoiler", t);
});

test("ciphers: ROT13, reversed, reversed words, upside-down (franchise/entity only)", () => {
  assert.equal(f("tgn 6 raqvat").franchise, true);
  assert.equal(f("6 atg").franchise, true);
  assert.equal(f("atg 6 gnidne").franchise, true);
  assert.equal(f("9 \u0250\u0287\u0183").franchise, true); // "gta 6" rotated 180 degrees
  assert.equal(f("zhfdhr hfr").franchise, false);
  assert.equal(f("wnfba qhiny qvrf").entity, true); // rot13 "jason duval dies"
  assert.equal(f("lavud nosaj").entity, true); // reversed
});

test("entities: strong names, vowel dropping, wildcards, inflected/transliterated forms", () => {
  for (const t of ["Lucia Caminos", "jason duval", "Port Gellhorn", "Leonida", "Leonida Keys", "Mount Kalaga", "Cal Hampton", "Boobie Ike", "Dre'Quan Priest", "Real Dimez", "Raul Bautista", "Brian Heder", "Only Raw Records", "Grassrivers", "jsn dvl", "lc cmns", "L*cia Caminos", "Jas0n Duval", "\u041b\u044e\u0441\u0438\u044f \u041a\u0430\u043c\u0438\u043d\u043e\u0441", "\u30ec\u30aa\u30cb\u30c0", "\u0644\u0648\u0633\u064a\u0627 \u0643\u0627\u0645\u064a\u0646\u0648\u0633"]) {
    assert.equal(f(t + " looks good").entity, true, t);
  }
  assert.equal(f("Jason").entityWeak, true);
  assert.equal(f("Jason").entity, false);
  for (const t of ["\u0414\u0436\u0435\u0439\u0441\u043e\u043d\u0430", "\u30b8\u30a7\u30a4\u30bd\u30f3", "\u6770\u68ee", "\u9732\u897f\u4e9a", "\u30eb\u30b7\u30a2"]) assert.equal(f(t).entityWeak, true, t);
});

test("weak entities need company: Jason/Lucia alone are not spoilers", () => {
  const r = C.text("Lucia is a lovely name");
  assert.equal(r.flags.entityWeak, true);
  assert.notEqual(r.level, "spoiler");
  assert.equal(C.text("Take-Two earnings call tonight").level, "none");
});

test("sentence-boundary rule: compact matches never cross sentence punctuation", () => {
  for (const t of ["GTA V. I loved the ending", "GTA V! I loved it", "I love GTA? 6 hours is long", "GTA V; I think", "GTA V:\nI loved it", "Playing GTA\n6 hours straight", "GTA V\u3002I loved it"]) {
    assert.equal(f(t).franchise, false, JSON.stringify(t));
  }
  assert.equal(f("GTA V I loved it").franchise, false);
  assert.equal(f("bigtavish thing").franchise, false);
  assert.equal(f("I got a vi").franchise, false);
});

test("fuzzy: bounded edit distance, same first letter, >=7 letters only", () => {
  assert.equal(f("Lusia Caminos").entity, true);
  assert.equal(f("Jasan Duval").entity, true);
  assert.equal(f("Grand Thef Auto 6").franchise, true);
  assert.equal(f("Grand Theft Atuo VI").franchise, true);
  assert.equal(f("Grand Theft Auto V").franchise, false);
  // short names get no fuzzy matching
  assert.equal(f("Mason").entityWeak, false);
  assert.equal(f("Lucas").entityWeak, false);
  assert.equal(f("Jasan").entityWeak, false);
  assert.equal(f("Luca").entityWeak, false);
  // first letter must match
  assert.equal(f("Mason Duval").entity, false);
  assert.equal(f("Pucia Caminos").entity, false);
  // stop list
  assert.equal(f("Leonidas of Sparta").entity, false);
  assert.equal(f("Leonid Brezhnev").entity, false);
});

test("CJK / Thai substring matching (no token alignment), Hangul particles", () => {
  assert.equal(f("\u6211\u89c9\u5f97GTA6\u5f88\u597d").franchise, true);
  assert.equal(f("\u4eca\u65e5\u306fGTA6\u304c\u697d\u3057\u307f").franchise, true);
  assert.equal(f("\uc9c0\ud0c06\uac00 \ub098\uc624\ub294\ub0a0").franchise, true);
  assert.equal(f("\u4fa0\u76d7\u730e\u8f66\u624b6\u5267\u900f").cue, true);
  assert.equal(f("\u30cd\u30bf\u30d0\u30ec\u6ce8\u610f GTA6").cue, true);
  assert.equal(f("\u0e08\u0e35\u0e17\u0e35\u0e40\u0e2d 6 \u0e2a\u0e1b\u0e2d\u0e22").franchise, true);
});

test("negatives suppress weak entities but never franchise_strong", () => {
  for (const t of ["Jason Voorhees dies at the end", "Jason Bourne betrays the agency in the ending", "Jason Statham dies in the final scene", "Jason Momoa", "Jason Kelce", "Jason Derulo", "Jason Mraz", "Jason Sudeikis", "Jason Segel", "Jason Isaacs"]) {
    assert.equal(f(t).entityWeak, false, t);
    assert.notEqual(C.text(t).level, "spoiler", t);
  }
  for (const t of ["our trip to St Lucia", "Saint Lucia travel", "Santa Lucia day", "St. Lucia flights", "Lucia di Lammermoor dies at the end"]) assert.equal(f(t).entityWeak, false, t);
  for (const t of ["Rockstar Energy drink", "GTA V ending", "GTA 5 mission", "GTA IV", "GTA San Andreas", "Vice City Stories", "Greater Toronto Area", "GTA housing market", "Toronto GTA traffic", "Mississauga and the GTA"]) assert.equal(f(t).entityWeak, false, t);
  // the negative never hides a real franchise reference
  assert.equal(f("Jason Voorhees fan here, also GTA 6 spoilers").franchise, true);
  assert.equal(f("Toronto GTA 6 ending leak").franchise, true);
  // GTA Online is weak, not negative
  assert.equal(f("GTA Online is fun").entityWeak, true);
});

test("cue handling: spoiler markers, anti-spoiler phrases, reddit/discord syntax, multilingual", () => {
  assert.equal(f("SPOILER ALERT, scroll on").marker, true);
  assert.equal(f("[spoilers] something").marker, true);
  assert.equal(f("(spoilers) something").marker, true);
  assert.equal(f(">!hidden text about GTA!<").marker, true);
  assert.equal(f("||hidden GTA text||").marker, true);
  assert.equal(f("spoiler warning: GTA 6").marker, true);
  assert.equal(f("GTA 6 review, no spoilers").cue, false);
  assert.equal(f("GTA 6 spoiler free review").cue, false);
  for (const t of ["muere", "meurt", "stirbt", "morre", "muore", "\u0443\u043c\u0438\u0440\u0430\u0435\u0442", "\u30cd\u30bf\u30d0\u30ec", "\u5267\u900f", "\u52ab\u900f".replace("\u52ab", "\u5287"), "\uc2a4\ud3ec", "\u0441\u043f\u043e\u0439\u043b\u0435\u0440", "\u043a\u043e\u043d\u0446\u043e\u0432\u043a\u0430", "traici\u00f3n", "trahison", "Verrat", "zdrada", "verraad", "ihanet"]) {
    assert.equal(f("GTA 6 " + t).cue, true, t);
  }
});

test("rule table: hot context, weak compound, cold spoiler-shape rule", () => {
  // hot=2 + strong cue => spoiler; hot=2 + weak name => at least mention
  assert.equal(C.text("he dies at the end", { hot: 2 }).level, "spoiler");
  assert.notEqual(C.text("Lucia is great", { hot: 2 }).level, "none");
  // cold, cue alone does not count
  assert.equal(C.text("he dies at the end", { hot: 0 }).level, "none");
  // weak compound: name + weak cue + hot
  assert.equal(C.text("Jason mission", { hot: 1 }).level, "spoiler");
  // cold shape rule needs a qualifier
  assert.notEqual(C.text("Jason dies at the end").level, "spoiler");
  assert.equal(C.text("Jason betrays Lucia at the end").level, "spoiler"); // both names
  assert.equal(C.text("Lucia dies in the final mission cutscene").level, "spoiler"); // game register word
  assert.equal(C.text("Lucia dies at the end", { reply: true }).level, "spoiler");
  assert.equal(C.text("Lucia is killed in the ending", { launchWindow: true }).level, "spoiler");
  assert.notEqual(C.text("Lucia is killed in the ending", { launchWindow: false }).level, "spoiler");
  assert.equal(C.text("the female protag dies at the end", { launchWindow: true }).level, "spoiler");
  // Jason Voorhees shape sentence is suppressed
  assert.notEqual(C.text("Jason Voorhees dies at the end", { reply: true }).level, "spoiler");
  // entity_strong + cue
  assert.equal(C.text("Leonida ending explained").level, "spoiler");
  // logistics only
  const r = C.text("GTA 6 release date and price?");
  assert.equal(r.flags.safe, true);
  assert.equal(r.level, "mention");
});

test("windows: a spoiler beyond the first 4000 chars is still found; windows overlap", () => {
  const filler = "the weather report for the coastal region predicts mild conditions. ".repeat(120);
  const text = filler + "GTA 6 ending leaked." + filler;
  assert.ok(text.length > 8000);
  assert.equal(C.text(text).level, "spoiler");
  // straddling a window boundary: whole phrase inside the 500-char overlap
  const pad = "x".repeat(3700);
  assert.equal(C.text(pad + " GTA 6 ending " + "y".repeat(3000)).level, "spoiler");
});

test("prefilter: >=99% of ordinary text is rejected", () => {
  const words = "the quick brown fox jumps over lazy dog while people discuss weather sports lunch music travel books garden coffee market train office meeting project budget holiday photo recipe weekend neighbour football concert museum bicycle kitchen window river mountain library school teacher doctor engineer painter student summer winter".split(" ");
  let seed = 7;
  const rnd = () => ((seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff);
  const before = { ...C.stats };
  let n = 0;
  for (let i = 0; i < 1000; i++) {
    const len = 12 + Math.floor(rnd() * 20);
    const s = Array.from({ length: len }, () => words[Math.floor(rnd() * words.length)]).join(" ");
    C.text(s);
    n++;
  }
  const rejected = C.stats.rejected - before.rejected;
  assert.ok(rejected / n >= 0.99, "rejected " + rejected + "/" + n);
});

test("url(): slugs and query params", () => {
  assert.equal(C.url("https://example.com/news/gta-6-ending-leaked").level, "spoiler");
  assert.equal(C.url("https://www.google.com/search?q=gta+6+who+dies").flags.franchise, true);
  assert.equal(C.url("https://www.youtube.com/results?search_query=GTA%20VI%20spoilers").flags.franchise, true);
  assert.equal(C.url("https://example.com/recipes/pasta").level, "none");
  assert.doesNotThrow(() => C.url("not a url"));
  assert.doesNotThrow(() => C.url(null));
  assert.equal(C.url("https://example.com/%E0%A4%A").level, "none");
});

test("isHotText and explain", () => {
  assert.equal(C.isHotText("anything about GTA 6"), true);
  assert.equal(C.isHotText("Lucia Caminos"), true);
  assert.equal(C.isHotText("Jason"), false);
  assert.equal(C.isHotText("lunch"), false);
  const r = C.text("GTA 6 ending: the twist");
  assert.equal(C.explain(r), "GTA VI reference + story cue");
  assert.equal(C.explain(C.text("hello world")), "No spoiler signals");
  assert.ok(!/jason|duval|dies/i.test(C.explain(C.text("Jason Duval dies at the end"))));
});

test("hostile input never throws and stays bounded", () => {
  const nasty = ["\u0000".repeat(1000), "(".repeat(50000), "a*".repeat(20000), "*".repeat(100000), "g*".repeat(30000), "GTA ".repeat(20000), "\u202e\u200b".repeat(40000), "!!||".repeat(20000)];
  for (const s of nasty) {
    const t0 = Date.now();
    assert.doesNotThrow(() => C.text(s, { hot: 2 }));
    assert.ok(Date.now() - t0 < 3000, "slow on " + s.slice(0, 8));
  }
});
