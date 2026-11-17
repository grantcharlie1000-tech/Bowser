// Downloads plain-text extracts from Wikipedia as a REAL negative corpus (text that is not a
// GTA VI spoiler). Run once; the JSON output is committed as a test fixture.
//   node test/corpus/real-negatives/fetch-real-negatives.mjs
// Text is CC BY-SA 4.0 (see README.md). Generic User-Agent, no personal data.
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "data");
const UA = "SpoilerFirewallTest/1.0 (test corpus)";
const MAX_CHARS = 45000;

const TITLES = {
  en: [
    "Friday the 13th (franchise)", "Friday the 13th (1980 film)", "Jason Bourne (film)", "The Bourne Identity (film)", "Saint Lucia", "Greater Toronto Area",
    "Grand Theft Auto V", "Grand Theft Auto: Vice City", "Grand Theft Auto: San Andreas", "Grand Theft Auto IV", "Grand Theft Auto: Vice City Stories",
    "Grand Theft Auto: Liberty City Stories", "Grand Theft Auto Online", "Rockstar Games", "Rockstar Energy", "Lucia di Lammermoor", "The Last of Us Part II",
    "Red Dead Redemption 2", "Breaking Bad", "The Sopranos", "Psycho (1960 film)", "The Sixth Sense", "Fight Club (film)", "Gone Girl (novel)", "Hamlet",
    "Romeo and Juliet", "Macbeth", "Titanic (1997 film)", "Avengers: Endgame", "The Godfather", "Casablanca (film)", "Jason Statham", "Jason Momoa",
    "Jason Kelce", "Jason Derulo", "Jason and the Argonauts", "Take-Two Interactive", "Miami Vice", "Scarface (1983 film)", "Bonnie and Clyde (film)",
    "Bonnie and Clyde", "Leonidas I", "Toronto", "Mississauga", "Brampton", "Super Bowl LVIII", "2022 FIFA World Cup", "Brian May", "Raúl (footballer)",
    "Cal Ripken Jr.", "Duval County, Florida", "Ambrosia", "Spoiler (media)", "Mass Effect 3", "Metal Gear Solid 2: Sons of Liberty", "Cyberpunk 2077",
    "Elden Ring", "Se7en", "The Usual Suspects", "Star Wars: The Empire Strikes Back", "Game of Thrones", "Oppenheimer (film)", "Barbie (film)",
    "Brian Cox (actor)", "Lucia Joyce", "Jason Todd", "Voorhees"
  ].filter(Boolean),
  es: ["Grand Theft Auto V", "Rockstar Games", "Santa Lucía", "Breaking Bad", "Real Madrid Club de Fútbol", "Don Quijote de la Mancha"],
  fr: ["Grand Theft Auto V", "Sainte-Lucie", "Vendredi 13 (série de films)", "Les Misérables", "Paris Saint-Germain Football Club"],
  de: ["Grand Theft Auto V", "Rockstar Games", "St. Lucia", "Faust. Der Tragödie erster Teil", "FC Bayern München"],
  ja: ["グランド・セフト・オート V", "ロックスター・ゲームス", "ジェイソン・ステイサム", "ファイナルファンタジーVII", "進撃の巨人"],
  ru: ["Grand Theft Auto V", "Rockstar Games", "Стэйтем, Джейсон", "Сент-Люсия", "Преступление и наказание"],
  zh: ["侠盗猎车手V", "洛克之星", "杰森·斯坦森", "三体 (小说)"],
  ko: ["그랜드 theft auto V", "록스타 게임스", "기생충 (영화)"],
};

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function extract(lang, title) {
  const u = new URL(`https://${lang}.wikipedia.org/w/api.php`);
  u.search = new URLSearchParams({ action: "query", prop: "extracts", explaintext: "1", format: "json", redirects: "1", titles: title }).toString();
  let res;
  for (let attempt = 0; attempt < 6; attempt++) {
    res = await fetch(u, { headers: { "User-Agent": UA, "Accept-Encoding": "gzip" } });
    if (res.status !== 429 && res.status < 500) break;
    const ra = Number(res.headers.get("retry-after")) || 0;
    await sleep(Math.max(ra * 1000, 3000 * (attempt + 1)));
  }
  if (!res.ok) throw new Error("HTTP " + res.status);
  const j = await res.json();
  const pages = j.query && j.query.pages ? Object.values(j.query.pages) : [];
  const p = pages[0];
  if (!p || p.missing !== undefined || !p.extract) return null;
  return { title: p.title, text: p.extract.slice(0, MAX_CHARS) };
}

function paragraphs(text) {
  const out = [];
  for (const line of text.split(/\n+/)) {
    const t = line.trim();
    if (t.length < 40 || /^=+\s.*\s=+$/.test(t)) continue;
    if (t.length <= 1000) { out.push(t); continue; }
    // long paragraph: chunk on sentence ends, <= ~800 chars
    let cur = "";
    for (const s of t.split(/(?<=[.!?。！？])\s+/)) {
      if ((cur + " " + s).length > 800 && cur) { out.push(cur.trim()); cur = s; } else cur += " " + s;
    }
    if (cur.trim().length >= 40) out.push(cur.trim());
  }
  return out;
}

mkdirSync(OUT, { recursive: true });
const summary = {};
for (const [lang, titles] of Object.entries(TITLES)) {
  const articles = [];
  for (const title of titles) {
    try {
      const a = await extract(lang, title);
      if (a) articles.push({ title: a.title, requested: title, paragraphs: paragraphs(a.text) });
      else console.log(`skip (missing) ${lang}: ${title}`);
    } catch (e) {
      console.log(`fail ${lang}: ${title}: ${e.message}`);
    }
    await sleep(1200);
  }
  const n = articles.reduce((s, a) => s + a.paragraphs.length, 0);
  summary[lang] = { articles: articles.length, paragraphs: n };
  writeFileSync(join(OUT, `${lang}.json`), JSON.stringify({ lang, license: "CC BY-SA 4.0", source: `https://${lang}.wikipedia.org`, fetched: new Date().toISOString().slice(0, 10), articles }));
}
console.log(JSON.stringify(summary, null, 1));
