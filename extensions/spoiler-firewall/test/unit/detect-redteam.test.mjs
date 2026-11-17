// Regression tests for the red-team detection findings (test/redteam/README.md H-1, M-2, M-3, M-4).
// A representative sample per fixed class, not the whole corpus: the corpus itself runs under test/redteam/.
import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const SFW = createRequire(import.meta.url)("../../dist/node/engine.cjs");
const { classify: C, policy: P } = SFW;
const flags = (t, ctx) => C.text(t, ctx).flags;
const decide = (t, ctx, mode = "strict") => P.decide(C.text(t, ctx), { mode, hot: (ctx && ctx.hot) || 0, kind: "item" });
const LAUNCH = { launchWindow: true };
// The per-call time budget is wall-clock; on a starved CI machine a mid-sized text can be reported truncated, so the
// "analysed completely" assertions retry a few times before giving up.
function complete(text, ctx) {
  let r;
  for (let i = 0; i < 6; i++) {
    r = C.text(text, ctx);
    if (!r.truncated) break;
  }
  return r;
}

test("H-1 franchise name with separators and number forms (strict shield)", () => {
  for (const t of [
    "just finished G-T-A-Six, the ending made me cry, he dies",
    "G.T.A.Six ending leaked: the main character dies in the last mission",
    "who dies at the end of g.t.a six? the twist is brutal",
    "G_T_A_VI plot twist - the villain is revealed at the end",
    "GTA 06 ending leaked: the main character dies in the last mission",
    "GTA 006 leaked epilogue",
    "GTA no.6 ending leaked: the main character dies in the last mission",
    "GTA no. 6 plot twist - the villain is revealed at the end",
    "GTA N°6 leaked epilogue",
    "GTA #6 leaked epilogue",
    "GTA*6 leaked epilogue and post-credits scene",
    "G*T*A*6 leaked epilogue and post-credits scene",
    "GTA|6 plot twist - the villain is revealed at the end",
    "gee tee ay six plot twist - the villain is revealed at the end",
    "jee tee ay seis plot twist - the villain is revealed at the end",
    "gee-tee-ay-six plot twist - the villain is revealed at the end",
    "spoiler for grand theft auto number 6: she betrays him in the final cutscene",
    "who dies at the end of Grand Theft Auto: Six? brutal",
    "GTA ⓺ ending leak: everyone dies", // double circled digit six
    "GTA 0️⃣ 6️⃣ ending leaked: the main character dies in the last mission", // keycaps 0 6
    "who dies at the end of GTAA 66? the twist is brutal",
    "GGTA 6 plot twist - the villain is revealed at the end",
    "just finished GTAA6, the ending made me cry, he dies",
    "spoiler for 6TA 6: she betrays him in the final cutscene",
    "spoiler for gt@ 6: she betrays him in the final cutscene",
    "just finished 6T4 V1, the ending made me cry, he dies",
    "just finished ԌTA 6, the ending made me cry, he dies", // Komi G
    "GTA бi plot twist - the villain is revealed at the end", // Cyrillic be + i
    "who dies at the end of the 6th GTA? brutal",
    "just finished GTA the 6th, the ending made me cry, he dies",
    "gta VIth leaked epilogue and post-credits scene",
    "just finished #GrandTheftAuto6, the ending made me cry, he dies",
    "#gtaVIending",
    "#GTAVIendingleak",
  ]) {
    assert.equal(decide(t, {}), "shield", t);
    assert.equal(decide(t, {}, "lockdown"), "shield", t);
  }
});

test("H-1 number words in other languages", () => {
  for (const t of [
    "gta шість кінцівка спойлер", // gta шість
    "gta שש סוף ספוילר", // gta שש
    "gta छह अंत स्पॉइलर", // gta छह
    "gta enam spoiler: misi terakhir",
    "gta kuusi spoiler: lopun kuolema",
    "gta seks ending spoiler dor",
    "gta ses ending spoiler",
    "GTA sau spoil ket thuc", // Vietnamese six needs a cue
    "GTA hat leaked ending", // Hungarian six: ambiguous alone, fine with a strong cue
    "GTA陸 剧透 结局",
    "العرب جاتا 6 النهاية سبويلر",
    "حرق قراند 6 النهاية",
    "그랜드 theft auto 6 스포 결말",
    "ग्रैंड थेफ्ट ऑटो 6 स्पॉइलर",
  ]) assert.equal(decide(t, {}), "shield", t);
  // the ambiguous numerals alone are not the franchise
  for (const t of ["GTA hat is on sale", "My GTA sis plays too", "I bought a GTA hat for my brother"]) assert.equal(decide(t, {}), "clear", t);
});

test("H-1 nicknames: soft forms need no cue, ambiguous forms need a strong cue, date forms need the launch prior", () => {
  for (const t of ["the new Grand Theft Auto ending leaked: the main character dies", "the sixth GTA ending leaked: he dies", "sixth grand theft auto ending leaked", "the Florida GTA: spoiler, he dies at the end"]) {
    assert.equal(decide(t, {}), "shield", t);
  }
  for (const t of ["R* new game ending: the hero dies, spoiler", "Rockstar's new game final mission spoiler: betrayal", "R★'s next game: the ending leaked, he dies", "the latest GTA ending leaked: nobody survives", "that GTA final mission spoiler", "that rockstar game ending spoiler: lucia dies", "6 GTA ending: she betrays him"]) {
    assert.equal(decide(t, {}), "shield", t);
  }
  // without a strong cue the generic nicknames are just words
  for (const t of ["the latest GTA home sales report", "I want that GTA price list", "Rockstar's new game logo", "the upcoming GTA parking rules"]) assert.equal(decide(t, LAUNCH), "clear", t);
  // "the game that comes out on the 19th": launch window, or an explicit leak word
  assert.equal(decide("the game from the 19th: she dies at the end", LAUNCH), "shield");
  assert.equal(decide("the game that comes out on the 19th: the ending, he dies", LAUNCH), "shield");
  assert.equal(decide("the Nov 19 game leaked epilogue and post-credits scene", {}), "shield");
  assert.equal(decide("the game from the 19th: she dies at the end", {}), "clear"); // deliberately not caught cold
  assert.equal(decide("the Nov 19 game against Chicago was great", LAUNCH), "clear");
  // R* / Rockstar + leak
  assert.equal(decide("R* leak: the ending", {}), "shield");
  assert.equal(decide("rockstar leak: ending of the new game, she dies", {}), "shield");
  assert.equal(decide("The new Rockstar album is out, spoilers: no features", LAUNCH), "clear");
  assert.equal(decide("R* new album (the band) leaked on Reddit", LAUNCH), "clear");
});

test("H-1 entity names in other scripts", () => {
  for (const t of [
    "Джейсон Дюваль умирает",
    "Лусия Каминос умирает в финале",
    "ジェイソン・デュバルが死ぬ",
    "ルシア・カミノスの結末",
    "杰森·杜瓦尔的结局",
    "露西亚·卡米诺斯死亡",
    "Лусия умирает в финале Леониды",
    "露西亚在利奥尼达的结局中死亡",
    "Port Gelhorn ending spoiler",
  ]) assert.equal(decide(t, {}), "shield", t);
  // Russian genitive "Leonid" (Леонида) is an ordinary name, not Leonida
  assert.equal(decide("По мнению литературоведа Леонида Гроссмана, роман написан в 1866 году", {}), "clear");
});

test("M-2 typo-tolerant entity names (prefilter anchor + cheap fuzzy probe)", () => {
  for (const t of [
    "Leonda dies in the final mission",
    "leaked cutscene shows Leondia betraying the crew",
    "leaked cutscene shows Lenoida betraying the crew",
    "leaked cutscene shows Leoniad betraying the crew",
    "Cla Hampton is the traitor and betrays everyone in the ending",
    "Ca Hampton death scene leaked",
    "Real Diemz turns out to be the rat",
    "so Real iDmez gets killed at the end, wild twist",
    "full Grasrivers storyline ending explained",
    "Garssrivers death scene leaked",
    "so Mount Klaaga gets killed at the end, wild twist",
    "so Onl Raw Records gets killed at the end, wild twist",
    "so Dre'Qun Priest gets killed at the end, wild twist",
    "full Port Gelhlorn storyline ending explained",
    "Bria Heder is the traitor and betrays everyone in the ending",
    "full Rau Bautista storyline ending explained",
  ]) assert.equal(decide(t, {}), "shield", t);
  // lookalike ordinary words are not names
  for (const t of ["I live in Leonia NJ and love it", "Leonard dies at the end of the film", "Port Elizabeth ferry ending soon", "Real Madrid won the final", "Mount Rushmore tour ends Friday"]) assert.equal(decide(t, LAUNCH), "clear", t);
  // the typo path is a two-stage prefilter: a name with a typo still goes through the full classifier for context
  assert.equal(flags("Leonda is a nice town").entity, true);
});

test("H-1 cold-context spoiler shapes in the launch window; generic unnamed shape; other-work disambiguation", () => {
  for (const t of [
    "Lucia dying in the final mission is crazy",
    "not Lucia dying in the final mission",
    "Jason doesn't make it out of the final mission",
    "Jason's death in the ending of the game destroyed me",
    "plot twist: Lucia was the rat the whole game",
    "Jason being the mole is such a good twist",
    "they kill off Jason in the last mission",
    "Lucia's betrayal in the final act hits so hard",
    "the twist is that Jason was working with the cops all along (game ending)",
    "user flair: Lucia dies in mission 20",
    "Lucia doesn't survive the final heist, game spoiler",
    "Jason is the rat",
  ]) assert.equal(decide(t, LAUNCH), "shield", t);
  // outside the launch window the shape alone is not enough
  for (const t of ["Lucia dying in the final mission is crazy", "Jason being the mole is such a good twist"]) assert.equal(decide(t, {}), "clear", t);
  // unnamed generic shape: <someone> dies ... final mission + a game-register word, launch window only
  assert.equal(decide("Arthur dies in the final mission", LAUNCH), "shield");
  assert.equal(decide("the old guard betrays him at the end of the final mission", LAUNCH), "shield");
  assert.equal(decide("Arthur dies in the final mission", {}), "clear");
  assert.equal(decide("my phone dies at the end of the day", LAUNCH), "clear");
  // other works / real-life context
  for (const t of [
    "Jason dies at the end of Friday the 13th Part 2, classic horror.",
    "Jason Brody dies at the end, Far Cry 3 spoilers",
    "Jason dies in the last mission of the Resident Evil game",
    "Lucia dies in the final mission of the Silent Hill game",
    "Lucia dies in the final scene of the opera, standing ovation",
    "Jason betrays Medea at the end, the Greek myth",
    "my cousin Jason betrayed me by eating my leftovers at the end of the party",
    "Jason is the traitor in our group chat, he left the Friday dinner early",
    "Lucia muere al final de la telenovela",
    "Jason stirbt im Finale des Horrorfilms",
    "Transfer leak: Jason Kim close to a move",
    "Jason Tran misses the final shot as the season ends in heartbreak",
  ]) assert.equal(decide(t, LAUNCH), "clear", t);
});

test("H-1 usernames, handles and flairs are split into words", () => {
  assert.equal(decide("u/LuciaDiesAtTheEnd: first!", LAUNCH), "shield");
  assert.equal(decide("@JasonDuvalDies: bro", {}), "shield");
  assert.equal(decide("u/JasonIsTheRat said: told you", LAUNCH), "shield");
  assert.equal(flags("u/SomeoneElse: first!").entity, false);
});

test("H-1 line breaks, HTML entities, special separators", () => {
  assert.equal(decide("GTA\n6\nending\nspoiler:\nhe\ndies", {}), "shield");
  assert.equal(decide("GTA\r\n6 ending spoiler: he dies", {}), "shield");
  assert.equal(decide("G\nT\nA\n6\nending spoiler he dies", {}), "shield");
  assert.equal(decide("GTA  6 ending spoiler: he dies", {}), "shield");
  assert.equal(decide("GT&shy;A 6 ending spoiler: he dies", {}), "shield");
  assert.equal(decide("G&zwnj;T&zwnj;A 6 ending leaked, he dies", {}), "shield");
  assert.equal(decide("G&#x54;A 6 ending leaked", {}), "shield"); // numeric entity for T
  // a blank line is still a paragraph break; so are sentence punctuation marks
  assert.equal(flags("Playing GTA\n\n6 hours straight").franchise, false);
  assert.equal(flags("I love GTA? 6 hours is long").franchise, false);
});

test("M-3 false positives that used to shield (Audi A6, GT-A6, Mafia, GTA Online, albums, Leonia)", () => {
  for (const t of ["GT A6 audi", "gt-a6 bolt torque spec", "GT-A6 mobile phone launch", "GT A6 audi for sale, one owner", "The next GTA-like game is Mafia: The Old Country", "Grand Theft Auto Online heists spoilers (Cayo Perico)", "The Warriors (Rockstar) ending explained", "GTA V ou GTA 5 est un jeu"]) {
    assert.equal(decide(t, LAUNCH), "clear", t);
  }
  // ... but "gt a6" with a strong spoiler cue is the evasion, not the car
  assert.equal(decide("GT A6 ENDING SPOILER HE DIES", {}), "shield");
  // a bare number after the word GTA is a number, not a leet letter
  assert.equal(flags("GTA 5 est").franchise, false);
  assert.equal(flags("GTA V I loved it").franchise, false);
  assert.equal(flags("G T A V I ending leaked").franchise, true);
});

test("M-3 Lockdown: maximally protective for GTA, quiet on ordinary pages", () => {
  // strong GTA signals: every mode
  for (const t of ["GTA 6 release date and price?", "GTA 6", "Lucia Caminos in the trailer", "the sixth GTA", "Leonda"]) assert.equal(decide(t, LAUNCH, "lockdown"), "shield", t);
  // bare weak names / orgs / places / markers on a cold page: clear even in Lockdown
  for (const t of [
    "Jason Reyes wins the lottery, says the ending of the story is a new house",
    "Police say Lucia Santos betrayed the trust of investors in a plot to defraud pensioners",
    "Local woman Lucia Miller turns 100; says secret is daily walks",
    "Lucia and I are going to the lake this weekend",
    "Take-Two Interactive shares rise after earnings call",
    "Vice City, a documentary about 1980s Miami, premieres on PBS",
    "Spoiler alert: it will be sunny all week in Toronto",
    "GTA home prices fall for third straight month",
    "Jason, I'm outside",
    "Cal Poly wins the final",
    "Brian May plays at the end of the show",
  ]) assert.equal(decide(t, LAUNCH, "lockdown"), "clear", t);
  // with a game signal or a hot context Lockdown shields again
  assert.equal(decide("Jason dies in the last mission", LAUNCH, "lockdown"), "shield");
  assert.equal(decide("Jason is great", { hot: 1 }, "lockdown"), "shield");
  assert.equal(decide("Spoiler alert: the ending", { hot: 1 }, "lockdown"), "shield");
  assert.equal(decide("good morning everyone", { hot: 2 }, "lockdown"), "shield");
  // Lockdown never shields less than Strict (and Strict never less than Balanced)
  const sample = [
    "Lucia dying in the final mission is crazy", "Jason is the rat", "that GTA ending leaked", "GTA 6 release date?", "Lucia Caminos", "GTA 6 trailer 3 price",
    "R* leak: the ending", "the new Grand Theft Auto", "he dies at the end", "Jason Brody dies at the end, Far Cry 3 spoilers", "[Spoilers] Elden Ring ending thread",
  ];
  for (const t of sample) for (const ctx of [{}, LAUNCH, { hot: 1 }, { hot: 2 }]) {
    const d = P.decideAll(C.text(t, ctx), { hot: ctx.hot || 0, kind: "item" });
    if (d.strict === "shield") assert.equal(d.lockdown, "shield", t + " " + JSON.stringify(ctx));
    if (d.balanced === "shield") assert.equal(d.strict, "shield", t + " " + JSON.stringify(ctx));
  }
});

test("M-4 per-call cap and budget: huge text is bounded, windowed, and reported as truncated (fail closed)", () => {
  const big = "g t a ".repeat(60000); // 360k chars of look-alike noise
  C.text("warm up the lexicon compile GTA 6", { hot: 0 }); // keep the one-off lexicon compile out of the timing
  const t0 = Date.now();
  const r = C.text(big, { hot: 0 });
  const dt = Date.now() - t0;
  assert.equal(r.truncated, true);
  assert.equal(r.len, big.length);
  assert.equal(P.decide(r, { mode: "balanced", hot: 0 }), "shield"); // budget overrun fails closed in every mode
  assert.ok(dt < 1500, "one call must be bounded (took " + dt + " ms)"); // generous: shared CI machines
  // a spoiler near the tail of an over-cap text is still found (head + tail are scanned)
  const tail = "lorem ipsum ".repeat(30000) + " GTA 6 ending leaked";
  const r2 = C.text(tail, { hot: 0 });
  assert.equal(r2.flags.franchise, true);
  assert.equal(r2.truncated, true);
  // ordinary long text within the cap is analysed completely
  const ok = ("The committee met on Tuesday to review the budget. ".repeat(30) + "GTA 6 ending leaked. ").repeat(4);
  const rok = complete(ok, { hot: 0 });
  assert.equal(rok.truncated, undefined);
  assert.equal(rok.flags.franchise, true);
});

test("windows: a spoiler in the second window of mid-sized text is still found", () => {
  const filler = "Nothing to see here, just a perfectly ordinary sentence about the weather. ".repeat(80);
  assert.ok(filler.length > 4000);
  const r = complete(filler + "GTA 6 ending leaked", { hot: 0 });
  assert.equal(r.flags.franchise, true);
  assert.equal(r.truncated, undefined);
});

test("all-digit spans are numbers: leet folding needs at least one real letter in the matched span", () => {
  for (const t of ["6746", "item 6746", "6 7 4 6", "foo 6746", "0161 674 6746", "£67.46", "v6.7.4.6", "ref 6.7.4.6", "In 1337 and 2026", "5318008", "call 07 7 3 4 6746 now", "order #6746 shipped"]) {
    for (const ctx of [{}, LAUNCH, { hot: 1 }]) {
      const r = C.text(t, ctx);
      assert.equal(r.flags.franchise, false, t);
      assert.equal(r.flags.entity, false, t);
      assert.equal(r.flags.entityWeak, false, t);
    }
    assert.equal(decide(t, LAUNCH, "lockdown"), "clear", t);
  }
  // evasions that contain a real letter still work
  for (const t of ["gt4 6 ending", "6t4 6 ending leaked", "g74 6 ending leaked"]) assert.equal(decide(t, {}), "shield", t);
});
