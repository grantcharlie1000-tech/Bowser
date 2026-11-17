// Deterministic synthetic corpus generator. All plot events are INVENTED; nothing here is real
// GTA VI plot knowledge. Example: { text, expect:{lockdown,strict,balanced}, category, transform,
// group:"pos"|"neg", tid (template id), heldOut, ctx }.
// Held-out: ~20% of templates (by hash of template id) are never inspected while tuning.
//   node test/corpus/generate.mjs [--out file.json] [--seed N]
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

function mulberry32(a) {
  return function () {
    a |= 0; a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
function hash(s) { let h = 2166136261; for (let i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = Math.imul(h, 16777619); } return h >>> 0; }
export const isHeldOut = (tid) => hash("ho:" + tid) % 5 === 0;

export function generate(seed = 20261117) {
  const rnd = mulberry32(seed);
  const pick = (a) => a[Math.floor(rnd() * a.length)];
  const chance = (p) => rnd() < p;
  const out = [];
  const seen = new Set();
  function add(ex) {
    if (seen.has(ex.text)) return false;
    seen.add(ex.text);
    ex.heldOut = isHeldOut(ex.tid);
    out.push(ex);
    return true;
  }

  // ---------------- transforms (applied to a span; Latin text)
  const HOMO = { a: "а", e: "е", o: "о", p: "р", c: "с", i: "і", x: "х", y: "у", s: "ѕ", j: "ј", k: "к", n: "η", t: "τ", u: "υ", v: "ν" };
  const LEET = { a: "4", e: "3", i: "1", o: "0", s: "5", t: "7", l: "1" };
  const nonSpace = (s) => [...s].map((ch, i) => [ch, i]).filter(([ch]) => /[A-Za-z0-9]/.test(ch));
  function homoglyph(s) {
    const idx = nonSpace(s).filter(([ch]) => HOMO[ch.toLowerCase()]);
    if (!idx.length) return null;
    const force = pick(idx)[1];
    return [...s].map((ch, i) => (HOMO[ch.toLowerCase()] && (i === force || chance(0.5)) ? HOMO[ch.toLowerCase()] : ch)).join("");
  }
  const zerowidth = (s) => [...s].join(pick(["​", "‌", "⁠", "­", "﻿"]));
  function leet(s) {
    const idx = nonSpace(s).filter(([ch]) => LEET[ch.toLowerCase()]);
    if (!idx.length) return null;
    const force = pick(idx)[1];
    return [...s].map((ch, i) => (LEET[ch.toLowerCase()] && (i === force || chance(0.5)) ? LEET[ch.toLowerCase()] : ch)).join("");
  }
  const spacing = (s) => s.replace(/\s+/g, "").split("").join(" ");
  const punct = (s) => s.replace(/\s+/g, "").split("").join(pick([".", "-", ".", "_"]));
  function emoji(s) {
    const r = [...s].map((ch) => (/[a-z]/i.test(ch) ? String.fromCodePoint(0x1f170 + ch.toLowerCase().charCodeAt(0) - 97) : ch)).join("");
    return r === s ? null : r;
  }
  function regional(s) {
    return [...s].map((ch) => (/[a-z]/i.test(ch) ? String.fromCodePoint(0x1f1e6 + ch.toLowerCase().charCodeAt(0) - 97) : ch)).join("");
  }
  function censor(s) {
    const cs = [...s];
    const inner = cs.map((ch, i) => [ch, i]).filter(([ch, i]) => /[a-z]/i.test(ch) && i > 0 && i < cs.length - 1 && /[a-z]/i.test(cs[i - 1]) && /[a-z]/i.test(cs[i + 1]));
    if (!inner.length) return null;
    cs[pick(inner)[1]] = pick(["*", "*", "_"]);
    return cs.join("");
  }
  const mixed = (s) => [...s].map((ch) => (chance(0.5) ? ch.toUpperCase() : ch.toLowerCase())).join("");
  const rot13 = (s) => s.replace(/[a-z]/gi, (c) => { const b = c <= "Z" ? 65 : 97; return String.fromCharCode(((c.charCodeAt(0) - b + 13) % 26) + b); });
  const rev = (s) => [...s].reverse().join("");
  const revWords = (s) => s.replace(/\S+/g, (w) => [...w].reverse().join(""));
  function vowel(s) { if (s.indexOf(" ") < 0 && s.length < 8) return null; const r = s.toLowerCase().replace(/'/g, "").replace(/(^|\s)(\S)(\S*)/g, (m, sp, a, rest) => sp + a + rest.replace(/[aeiou]/g, "")); return r === s.toLowerCase() || r.split(" ").join("").length < 5 ? null : r; }
  function typo(s) {
    const cs = [...s];
    const pos = cs.map((ch, i) => i).filter((i) => i > 1 && /[a-z]/i.test(cs[i]) && /[a-z]/i.test(cs[i - 1]) && cs[i] !== cs[i - 1]);
    if (!pos.length) return null;
    const i = pick(pos);
    if (chance(0.5)) cs.splice(i, 1);
    else { const t = cs[i]; cs[i] = cs[i - 1]; cs[i - 1] = t; }
    return cs.join("");
  }
  const hashtag = (s) => "#" + (chance(0.5) ? s.replace(/[^A-Za-z0-9]/g, "") : s.split(/\s+/).map((w) => w[0].toUpperCase() + w.slice(1)).join(""));
  const camelHashtag = (s) => "#" + s.split(/\s+/).map((w) => w[0].toUpperCase() + w.slice(1)).join("");

  const SPAN_TRANSFORMS = { homoglyph, zerowidth, leet, spacing, punct, emoji, regional, censor, mixed, hashtag, camelHashtag, none: (s) => s };
  const ENTITY_TRANSFORMS = { homoglyph, zerowidth, spacing, punct, vowel, censor, mixed, typo, none: (s) => s, leet };

  // ---------------- vocabulary
  const F_EN = ["GTA 6", "GTA VI", "GTA6", "GTAVI", "Grand Theft Auto VI", "Grand Theft Auto 6", "GTA Six", "gta vi", "gta 6", "GTA-6", "G.T.A. 6", "the new GTA", "Grand Theft Auto Six", "Gta 6"];
  const NAMES = ["Lucia", "Jason"];
  const E_FULL = ["Lucia Caminos", "Jason Duval", "Port Gellhorn", "Leonida", "Leonida Keys", "Mount Kalaga", "Cal Hampton", "Boobie Ike", "Dre'Quan Priest", "Real Dimez", "Raul Bautista", "Brian Heder", "Only Raw Records", "Grassrivers"];
  const EN_TEMPLATES = [
    "{F} ending is insane, {A} dies in the last mission",
    "just finished {F}, the twist at the end ruined me",
    "leaked: {F} plot - {B} betrays {A}",
    "{F} spoilers: who dies?",
    "[spoilers] {F} final boss and epilogue",
    "ok so in {F} it turns out {A} was the rat all along",
    "the post credits scene of {F} is wild",
    "{F} story leak: the villain is revealed to be the mayor",
    "{F} full playthrough, all cutscenes and the true ending",
    "anyone else shocked that {A} survives the finale in {F}?",
    ">!{F} {A} dies in mission 12!<",
    "SPOILER ALERT {F}: {A} and {B} get killed",
    "yo {F} ending explained",
    "{F} leaked mission list and plot twist, {A} betrayed everyone",
    "I cried when {A} died in {F}",
    "{F}: the epilogue reveals {B} was dead the whole time",
    "so the final cutscene in {F} shows {A} sacrificing herself",
    "watch out for {F} spoilers, the ending is a massive twist",
    "{F} who dies in the end? thread",
    "plot details of {F} leaked on a forum, {A} is killed in act 3",
  ];
  const ML = [
    { l: "es", F: ["GTA 6", "Grand Theft Auto VI", "GTA VI", "nuevo GTA", "GTA6", "GTA seis"], A: ["Lucia", "Jason"], T: ["{F}: el final es una locura, {A} muere en la ultima mision", "spoiler {F}: {A} traiciona a {B} al final", "filtracion de la trama de {F}, {A} muere", "{F} spoilers del desenlace: {A} muere en el epilogo"] },
    { l: "fr", F: ["GTA 6", "Grand Theft Auto VI", "GTA VI", "nouveau GTA", "GTA6"], A: ["Lucia", "Jason"], T: ["spoiler {F} : {A} meurt a la fin", "{F} fuite de l'histoire, {A} trahit {B}", "la fin de {F} est incroyable, trahison de {A}", "{F} spoiler: la mort de {A} dans la derniere mission"] },
    { l: "de", F: ["GTA 6", "Grand Theft Auto VI", "GTA VI", "neues GTA", "GTA6", "GTA sechs"], A: ["Lucia", "Jason"], T: ["{F} Ende Spoiler: {A} stirbt im Finale", "{F} Handlung geleakt, {A} stirbt am Ende", "Spoiler zu {F}: {A} begeht Verrat an {B}", "{F} Abspann Szene: Tod von {A}"] },
    { l: "it", F: ["GTA 6", "Grand Theft Auto VI", "GTA VI", "nuovo GTA", "GTA6"], A: ["Lucia", "Jason"], T: ["{F} spoiler: il finale, {A} muore", "trama di {F} trapelata, {A} tradisce {B}", "{F} fine del gioco: morte di {A}, colpo di scena"] },
    { l: "pt", F: ["GTA 6", "Grand Theft Auto VI", "GTA VI", "novo GTA", "GTA6", "GTA seis"], A: ["Lucia", "Jason"], T: ["{F} spoiler: o final, {A} morre", "enredo de {F} vazou, {A} trai {B}", "{F} reviravolta no final: morte de {A}"] },
    { l: "ru", F: ["ГТА 6", "GTA 6", "ГТА6", "ГТА VI", "Grand Theft Auto VI", "новая ГТА"], A: ["Люсия", "Джейсон"], T: ["{F} спойлер: концовка, {A} умирает", "утечка сюжета {F}: {A} предает {B}", "{F} финал игры, смерть {A} в последней миссии"] },
    { l: "ja", F: ["グラセフ6", "GTA6", "グランド・セフト・オートVI", "GTA VI", "ジーティーエー6"], A: ["ルシア", "ジェイソン"], T: ["{F} ネタバレ: 結末で{A}が死ぬ", "{F} のエンディングが衝撃的、{A}の裏切り", "{F} リーク: 最終ミッションで{A}が死亡"] },
    { l: "zh", F: ["侠盗猎车手6", "俠盜獵車手6", "GTA6", "GTA 6", "侠盗飞车6"], A: ["露西亚", "杰森"], T: ["{F}剧透：结局{A}死亡", "{F}泄露剧情，{A}背叛", "{F}大结局反转，{A}死去"] },
    { l: "ko", F: ["지타6", "GTA6", "GTA 6", "그랜드테프트오토6"], A: ["루시아", "제이슨"], T: ["{F} 스포: 결말에서 {A} 죽는다", "{F} 엔딩 유출, {A} 배신", "{F} 스포일러 반전 {A} 죽음"] },
    { l: "ar", F: ["جي تي اي 6", "GTA 6", "GTA6"], A: ["لوسيا", "جيسون"], T: ["{F} سبويلر: النهاية يموت {A}", "تسريب قصة {F}: خيانة {A}"] },
    { l: "th", F: ["จีทีเอ 6", "GTA 6", "GTA6"], A: ["ลูเซีย", "เจสัน"], T: ["{F} สปอย ตอนจบ {A} ตาย", "{F} หลุด พล็อต {A} หักหลัง"] },
    { l: "tr", F: ["GTA 6", "GTA6", "GTA VI", "Grand Theft Auto 6"], A: ["Lucia", "Jason"], T: ["{F} spoiler: sonunda {A} ihanet eder", "{F} sizinti hikaye {A} ihanet"] },
    { l: "pl", F: ["GTA 6", "GTA6", "GTA VI", "Grand Theft Auto VI"], A: ["Lucia", "Jason"], T: ["{F} spoiler: koniec, {A} umiera", "{F} przeciek fabuly, zdrada {A}"] },
    { l: "nl", F: ["GTA 6", "GTA6", "GTA VI", "Grand Theft Auto VI"], A: ["Lucia", "Jason"], T: ["{F} spoiler: einde, {A} sterft", "{F} verhaal uitgelekt, verraad van {A}"] },
  ];
  const E_TEMPLATES = [
    "{E} dies in the final mission",
    "{E} is the traitor and betrays everyone in the ending",
    "so {E} gets killed at the end, wild twist",
    "spoiler: {E} survives the finale",
    "leaked cutscene shows {E} betraying the crew",
    "{E} turns out to be the rat",
    "full {E} storyline ending explained",
    "{E} death scene leaked",
  ];
  const E_MENTION = ["{E} looks great", "can't wait to see {E}", "new screenshot of {E}", "{E} trailer reaction"];

  const ex = (group, category, transform, tid, text, expect, ctx) => add({ text, expect, category, transform, group, tid, ctx: ctx || null });
  const ALLS = { lockdown: "shield", strict: "shield", balanced: "shield" };
  const CLEAR = { lockdown: "clear", strict: "clear", balanced: "clear" };

  // ---------------- positives
  const fTransforms = Object.keys(SPAN_TRANSFORMS);
  const ctxChoices = [null, null, null, { hot: 1 }, { hot: 2 }];
  let guard = 0;
  while (out.length < 2600 && guard++ < 200000) {
    const ti = Math.floor(rnd() * EN_TEMPLATES.length);
    const tr = pick(fTransforms);
    const span = SPAN_TRANSFORMS[tr](pick(F_EN));
    if (span == null) continue;
    const A = pick(NAMES), B = A === "Lucia" ? "Jason" : "Lucia";
    const text = EN_TEMPLATES[ti].replace(/\{F\}/g, span).replace(/\{A\}/g, A).replace(/\{B\}/g, B);
    ex("pos", "franchise-en", tr, "fen" + ti, text, ALLS, pick(ctxChoices));
  }
  guard = 0;
  while (out.length < 3000 && guard++ < 100000) {
    const ti = Math.floor(rnd() * EN_TEMPLATES.length);
    const tr = pick(["rot13", "reversed", "reversedWords"]);
    const base = EN_TEMPLATES[ti].replace(/\{F\}/g, pick(["GTA 6", "GTA VI", "Grand Theft Auto VI", "GTA6"])).replace(/\{A\}/g, "Lucia").replace(/\{B\}/g, "Jason");
    const text = tr === "rot13" ? rot13(base) : tr === "reversed" ? rev(base) : revWords(base);
    ex("pos", "cipher", tr, "cip" + ti, text, ALLS);
  }
  guard = 0;
  while (out.length < 3900 && guard++ < 100000) {
    const m = pick(ML);
    const ti = Math.floor(rnd() * m.T.length);
    const F0 = pick(m.F);
    const latin = /^[\x00-\x7f ]+$/.test(F0);
    const tr = latin && chance(0.45) ? pick(["homoglyph", "zerowidth", "mixed", "leet", "spacing", "punct", "hashtag"]) : pick(["none", "none", "zerowidth"]);
    const span = SPAN_TRANSFORMS[tr](F0);
    if (span == null) continue;
    const A = pick(m.A), B = m.A.find((x) => x !== A);
    const text = m.T[ti].replace(/\{F\}/g, span).replace(/\{A\}/g, A).replace(/\{B\}/g, B);
    ex("pos", "multilingual-" + m.l, tr, "ml" + m.l + ti, text, ALLS);
  }
  guard = 0;
  const eTrs = Object.keys(ENTITY_TRANSFORMS);
  while (out.length < 4900 && guard++ < 200000) {
    const ti = Math.floor(rnd() * E_TEMPLATES.length);
    const tr = pick(eTrs);
    const span = ENTITY_TRANSFORMS[tr](pick(E_FULL));
    if (span == null) continue;
    ex("pos", "entity-cue", tr, "ec" + ti, E_TEMPLATES[ti].replace(/\{E\}/g, span), ALLS, pick([null, null, { hot: 1 }]));
  }
  guard = 0;
  while (out.length < 5100 && guard++ < 20000) {
    const ti = Math.floor(rnd() * E_MENTION.length);
    const tr = pick(["none", "none", "homoglyph", "zerowidth", "mixed", "spacing"]);
    const span = ENTITY_TRANSFORMS[tr](pick(E_FULL));
    if (span == null) continue;
    ex("pos", "entity-mention", tr, "em" + ti, E_MENTION[ti].replace(/\{E\}/g, span), { lockdown: "shield", strict: "shield", balanced: "clear" });
  }
  const SHAPE = [
    "{A} dies at the end", "{A} betrays {B} in the final mission", "{A} is the rat and gets killed in the ending", "{A} survives the credits scene",
    "{A} was working for the cops all along, ending twist", "{A} is killed at the end of the last mission", "{B} gets killed in the credits",
  ];
  guard = 0;
  while (out.length < 5500 && guard++ < 20000) {
    const ti = Math.floor(rnd() * SHAPE.length);
    const kind = pick(["both", "game", "reply", "launch", "role"]);
    const A = pick(NAMES), B = A === "Lucia" ? "Jason" : "Lucia";
    let text = SHAPE[ti].replace(/\{A\}/g, A).replace(/\{B\}/g, B);
    let ctx = null;
    if (kind === "both" && !/\{B\}/.test(SHAPE[ti]) && !text.includes(B)) text += " and " + B + " too";
    if (kind === "game") text = "that cutscene: " + text.replace(/^./, (c) => c.toLowerCase()) + " in the playthrough";
    if (kind === "reply") ctx = { reply: true };
    if (kind === "launch") ctx = { launchWindow: true };
    if (kind === "role") {
      text = text.replace(/Lucia|Jason/, "the female protag").replace(/Lucia|Jason/g, "he");
      if (!/female protag/.test(text)) continue;
    }
    ex("pos", "cold-shape-" + kind, "none", "sh" + ti + kind, text, ALLS, ctx);
  }
  guard = 0;
  const HOT_T = ["he dies at the end", "the ending is a huge twist, the villain betrays everyone", "leaked story details, full plot", "who dies in the finale?", "turns out the rat was there all along"];
  while (out.length < 5900 && guard++ < 20000) {
    const ti = Math.floor(rnd() * HOT_T.length);
    const mut = pick([(s) => s, (s) => s.toUpperCase(), (s) => zerowidth(s), (s) => s + " lol", (s) => "omg " + s, (s) => s + " " + Math.floor(rnd() * 100)]);
    ex("pos", "hot2-cue-only", "none", "h2" + ti, mut(HOT_T[ti]), ALLS, { hot: 2 });
  }

  // ---------------- negatives
  const OTHER = ["Elden Ring", "Cyberpunk 2077", "The Last of Us Part II", "Red Dead Redemption 2", "Mass Effect 3", "Metal Gear Solid 2", "Hades", "Baldur's Gate 3", "Halo Infinite", "Fallout 4", "Final Fantasy VII", "Zelda: Tears of the Kingdom", "God of War", "Disco Elysium", "Hollow Knight", "Oppenheimer", "Titanic", "Inception", "The Sixth Sense", "Fight Club", "Se7en", "Avengers: Endgame", "The Godfather", "Parasite", "Interstellar", "Gone Girl", "Psycho"];
  const GEN_T = [
    "SPOILER ALERT: the ending of {X} made me cry, the main character dies in the last scene",
    "{X} twist at the end is wild, the villain betrays everyone",
    "just finished {X}, the final mission is brutal and the epilogue is great",
    "who dies in {X}? ending explained, post credits scene included",
    "leaked plot details of {X} are all over the forum",
    "[spoilers] {X} final boss and ending",
    ">!the killer was his brother in {X}!<",
  ];
  for (let i = 0; i < 700; i++) {
    const ti = Math.floor(rnd() * GEN_T.length);
    const text = GEN_T[ti].replace(/\{X\}/g, pick(OTHER)) + (chance(0.5) ? " " + Math.floor(rnd() * 1000) : "");
    ex("neg", "other-media-spoilers", "none", "og" + ti, text, { lockdown: "clear", strict: "clear", balanced: "clear" }); // M-3: a bare spoiler marker on a cold page is not GTA evidence, even in Lockdown
  }
  const V_T = [
    "{X} was a masterpiece, the ending where Tommy dies still gets me", "nostalgia for {X}: the final mission and the credits are iconic",
    "{X} betrayal in the last mission was a great twist", "does anyone remember the plot twist in {X}? he dies at the end",
    "{X} leaked cutscene from the beta was wild", "replaying {X} full playthrough, all cutscenes",
    "my {X} ending explained: Claude dies and Carl betrays", "{X} story mode spoilers, the villain survives",
  ];
  const OLD = ["GTA V", "GTA 5", "GTA IV", "GTA 4", "GTA III", "GTA San Andreas", "Grand Theft Auto V", "Grand Theft Auto IV", "GTA Vice City", "Grand Theft Auto: San Andreas", "Vice City Stories", "GTA Vice City 2002", "Grand Theft Auto III", "GTA Liberty City Stories"];
  for (let i = 0; i < 700; i++) {
    const ti = Math.floor(rnd() * V_T.length);
    ex("neg", "older-gta-games", "none", "old" + ti, V_T[ti].replace(/\{X\}/g, pick(OLD)) + (chance(0.5) ? " " + Math.floor(rnd() * 1000) : ""), CLEAR);
  }
  const mk = (arr, n, cat, pre, expect) => {
    for (let i = 0; i < n; i++) {
      const ti = i % arr.length;
      const text = i < arr.length ? arr[ti] : (i % 2 ? mixed(arr[ti]) : arr[ti]) + " " + i;
      ex("neg", cat, i < arr.length ? "none" : "mixed", pre + ti, text, typeof expect === "function" ? expect(text) : expect);
    }
  };
  mk(["Jason Voorhees finally dies at the end of the movie, great ending", "Jason Bourne betrays the agency in the final act, twist ending", "watching Jason Statham films, the villain dies at the end",
    "Jason Momoa was great, his character dies in the last scene", "Jason Kelce retires, the ending of his career", "Friday the 13th: Jason is killed in the final scene",
    "Jason and the Argonauts, the ending of the myth", "Jason Derulo new single dropped", "Jason Todd betrays Batman in the comic ending"], 360, "jason-other", "jas", CLEAR);
  mk(["our trip to St Lucia was amazing, the beaches and the zipline", "Saint Lucia travel guide: best resorts and the Pitons", "Santa Lucia festival in Sweden, the ending procession",
    "Lucia di Lammermoor dies at the end of the opera, the mad scene is famous", "flights to St. Lucia from London are cheaper in spring", "visiting Saint Lucia in December, spoiler: it is hot"], 300, "lucia-other", "luc", CLEAR);
  mk(["GTA housing market update: Toronto condo prices", "GTA traffic on the 401 is a nightmare again", "Greater Toronto Area real estate, Mississauga and Brampton rentals", "GTA rental prices keep rising in Durham region",
    "moving to the GTA, Toronto homes for sale", "GTA transit delays on the Gardiner"], 300, "toronto-gta", "tor", CLEAR);
  mk(["Rockstar Energy drink flavors ranked", "bought a Rockstar Energy at the gas station", "rockstar energy sponsorship at the motocross race", "feeling like a rockstar today after the promotion"], 200, "rockstar-other", "rk", CLEAR);
  const LOG_T = [
    "GTA 6 release date confirmed, can't wait", "{F} price and preorder details", "{F} download size on PS5 and Xbox", "when does {F} preload start?", "{F} trailer 3 countdown",
    "{F} PC port when? metacritic score?", "so hyped for {F}, midnight launch time", "{F} collectors edition preorder price",
  ];
  for (let i = 0; i < 400; i++) {
    const ti = Math.floor(rnd() * LOG_T.length);
    let text = LOG_T[ti].replace(/\{F\}/g, pick(["GTA 6", "GTA VI", "Grand Theft Auto VI", "GTA6"]));
    const long = chance(0.3);
    if (long) text += " we have been waiting for this release for so many years and the whole community is counting every single day until launch, so excited " + Math.floor(rnd() * 1000);
    else text += " " + Math.floor(rnd() * 100);
    ex("neg", long ? "logistics-long" : "logistics-short", "none", "lg" + ti + (long ? "L" : "S"), text, { lockdown: "shield", strict: text.length <= 140 ? "clear" : "shield", balanced: "clear" });
  }
  mk(["Jason dies at the end", "Lucia betrays him in the ending", "Lucia is the rat at the end", "Jason survives the final scene", "Lucia is a great name for a character"], 100, "cold-ambiguous", "amb", { lockdown: "clear", strict: "clear", balanced: "clear" }); // M-3: bare weak names + story words, no game/GTA signal
  mk(["spoiler de la pelicula, el final es increible", "fin du film: il meurt a la fin, quelle trahison", "Spoiler zum Film: der Held stirbt am Ende", "ネタバレ注意 映画の結末で主人公が死ぬ", "спойлер: в конце фильма герой умирает", "剧透预警 这部电影的结局很震撼", "스포 주의 영화 결말 반전"], 200, "other-media-ml", "mln", { lockdown: null, strict: "clear", balanced: "clear" });
  mk(["lunch was great today", "the weather is lovely this weekend", "anyone know a good pizza place nearby?", "my team lost in overtime, brutal", "new phone arrived, the camera is amazing", "reading a book about medieval history", "can't believe it's already Friday", "the new update fixed my battery issue", "who is watching the match tonight?", "traffic was awful this morning"], 400, "chatter", "ch", CLEAR);
  return out;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const a = process.argv;
  const si = a.indexOf("--seed");
  const data = generate(si > 0 ? Number(a[si + 1]) : undefined);
  const oi = a.indexOf("--out");
  const pos = data.filter((d) => d.group === "pos").length;
  console.log(`generated ${data.length} examples (${pos} positives, ${data.length - pos} negatives)`);
  if (oi > 0) writeFileSync(a[oi + 1], JSON.stringify(data));
}
