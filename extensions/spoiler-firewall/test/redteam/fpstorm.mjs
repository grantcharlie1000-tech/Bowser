// False-positive storm: realistic ORDINARY pages (no GTA VI content at all) generated from phrase banks that deliberately
// reuse vocabulary the detector is sensitive to (dies/ending/final mission/leak/twist/betrays, people named Jason/Lucia,
// Rockstar/Take-Two, Toronto GTA, other games' spoilers). Measures shield rate per page type in Strict/Lockdown, then a
// browser pass checks the "warm feed" cascade (>=3 false shields make the whole feed warm and shield ordinary posts).
import { createRequire } from "node:module";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import http from "node:http";

const HERE = dirname(fileURLToPath(import.meta.url));
const SFW = createRequire(import.meta.url)(join(HERE, "..", "..", "dist", "node", "engine.cjs"));

function rng(seed) { let a = seed >>> 0; return () => { a = (a + 0x6d2b79f5) | 0; let t = Math.imul(a ^ (a >>> 15), 1 | a); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; }; }

const FIRST = ["Jason", "Lucia", "Maria", "Tom", "Priya", "Chen", "Aisha", "Dmitri", "Sofia", "Kwame", "Hannah", "Liam", "Jason", "Lucia"];
const LAST = ["Miller", "Santos", "Okafor", "Kim", "Novak", "Reyes", "Tran", "Lee", "Brown", "Singh", "Garcia", "Walsh"];
const CITY = ["Toronto", "Mississauga", "Brampton", "Markham", "Miami", "Tampa", "Orlando", "Leeds", "Austin", "Denver", "Lagos", "Pune", "Porto"];
const TEAM = ["Raptors", "Maple Leafs", "Blue Jays", "Heat", "Celtics", "Lakers", "Bucks", "Rangers", "Sharks"];
const GAME = ["Elden Ring", "Baldur's Gate 3", "Cyberpunk 2077", "Red Dead Redemption 2", "The Last of Us", "Hades", "Zelda", "Fallout 4", "Mass Effect 3", "GTA V", "GTA IV", "Vice City", "San Andreas", "Skyrim", "Starfield", "Hollow Knight"];
const SHOW = ["Succession", "Breaking Bad", "The Bear", "Severance", "Stranger Things", "Dune", "Oppenheimer", "Wicked", "Squid Game", "Andor", "Yellowjackets"];
const pick = (r, a) => a[Math.floor(r() * a.length)];
const name = (r) => pick(r, FIRST) + " " + pick(r, LAST);

const BANK = {
  news: [
    (r) => `${name(r)}, 62, dies after long illness; ${pick(r, CITY)} mourns`, (r) => `Council vote ends the ${pick(r, CITY)} parking dispute after final meeting`,
    (r) => `Police say ${name(r)} betrayed the trust of investors in a plot to defraud pensioners`, (r) => `Water main leak floods ${pick(r, CITY)} downtown; repairs expected to end Friday`,
    (r) => `The final mission of the ${pick(r, ["Artemis", "Voyager", "Hubble servicing", "ISS crew"])} programme has been extended`, (r) => `Leaked memo shows ${pick(r, ["the ministry", "the airline", "the bank"])} planned layoffs`,
    (r) => `Opinion: the twist in the budget is that nobody wins`, (r) => `Epilogue to a long strike: workers return as the contract ending is agreed`, (r) => `${pick(r, ["Mayor", "Governor", "Senator"])} ${name(r)} betrays party line in final vote`,
    (r) => `Weather: freezing rain tonight, spoiler for tomorrow's commute`, (r) => `${name(r)} wins the lottery, says the ending of the story is a new house`, (r) => `Port authority says ${pick(r, ["Port Elizabeth", "Port Said", "Port Hope"])} ferry service ending soon`,
    (r) => `Rockstar chef opens ${pick(r, CITY)} restaurant; the menu ends with a twist on dessert`, (r) => `Take-Two Interactive shares rise after earnings call`, (r) => `Vice City, a documentary about 1980s Miami, premieres on PBS`,
    (r) => `Report: ${pick(r, CITY)} housing market plot twist as prices fall`, (r) => `Obituary: ${name(r)} passed away peacefully; services Saturday`, (r) => `Heist at ${pick(r, CITY)} jeweller: suspect dies in crash after police chase`,
    (r) => `Markets close higher; the dollar ends the week at a two-month high`, (r) => `Storm makes landfall in Florida Keys; hurricane warning ends Sunday`, (r) => `${name(r)} named new CEO as predecessor leaves at the end of the quarter`,
    (r) => `Spoiler alert: it will be sunny all week in ${pick(r, CITY)}`, (r) => `Film critic ${name(r)} on why the ending of ${pick(r, SHOW)} works`,
    (r) => `Jason ${pick(r, LAST)} announced as new coach of the ${pick(r, TEAM)}`, (r) => `Lucia ${pick(r, LAST)} elected chair of the ${pick(r, CITY)} school board`, (r) => `Gas leak forces evacuation of ${pick(r, CITY)} office tower`,
    (r) => `Study: sleep, diet and the final stretch of a marathon`, (r) => `Local woman Lucia ${pick(r, LAST)} turns 100; says secret is daily walks`, (r) => `Jason ${pick(r, LAST)}, 34, charged in connection with ${pick(r, CITY)} robbery`,
  ],
  toronto: [
    (r) => `GTA home prices fall for third straight month`, (r) => `TTC fare changes coming to the GTA in the new year`, (r) => `GTA man dies in Highway 401 crash; OPP investigate`, (r) => `Raptors final score: ${60 + Math.floor(r() * 60)}-${60 + Math.floor(r() * 60)}`,
    (r) => `${name(r)} of Brampton wins big at the ${pick(r, ["OLG", "casino", "raffle"])}`, (r) => `Drake teases new album, says the 6ix will love the ending`, (r) => `Toronto snow: GTA bracing for the final storm of the season`,
    (r) => `GTA zone 6 bus route cancelled; final day is Friday`, (r) => `Jason ${pick(r, LAST)} launches ${pick(r, ["bakery", "tutoring", "plumbing"])} business in ${pick(r, CITY)}`, (r) => `Lucia ${pick(r, LAST)} on the ending of her 30-year teaching career in ${pick(r, CITY)}`,
    (r) => `Blue Jays trade rumours: a twist at the deadline`, (r) => `GTA school boards: end of strike nears as final offer is tabled`, (r) => `Leaf Island ferry final run of the season`, (r) => `Police plot: GTA thieves targeted keyless cars`,
  ],
  reddit_games: [
    (r) => `[Spoilers] ${pick(r, GAME)} ending discussion: who else cried?`, (r) => `${pick(r, GAME)} final mission was incredible (spoilers inside)`, (r) => `Theory: the ${pick(r, GAME)} twist was foreshadowed in chapter 2`,
    (r) => `Did anyone else think ${pick(r, ["the villain", "the sidekick", "the narrator"])} betrays the party in ${pick(r, GAME)}?`, (r) => `Leaked ${pick(r, ["Switch 2", "PS6", "Xbox"])} specs, what do you think?`,
    (r) => `${pick(r, GAME)} all endings explained`, (r) => `Rockstar patch notes for Red Dead Online: bug fixes and balance changes`, (r) => `Just finished ${pick(r, GAME)}. Jason's story hit me hard`, (r) => `Lucia is my favourite character in ${pick(r, GAME)}`,
    (r) => `PSA: ${pick(r, GAME)} final boss strategy, spoilers`, (r) => `Take-Two earnings: what it means for the industry`, (r) => `Best open-world endings: Red Dead 2, Witcher 3, Elden Ring`, (r) => `GTA V is 12 years old and still sells; why?`, (r) => `Anyone replaying Vice City? The ending still holds up`,
    (r) => `Full playthrough of ${pick(r, GAME)}, all cutscenes, no commentary`, (r) => `${pick(r, GAME)} post-credits scene explained`, (r) => `Character death in ${pick(r, GAME)} hit harder than expected`, (r) => `Unpopular opinion: the ${pick(r, GAME)} epilogue is the best part`,
    (r) => `My dad Jason finally beat ${pick(r, GAME)} at age 58`, (r) => `Lucia from ${pick(r, ["Persona", "Fire Emblem", "Octopath"])} deserves more love`, (r) => `Mission 3 of ${pick(r, GAME)} is a difficulty spike`,
  ],
  sports: [
    (r) => `${pick(r, TEAM)} beat ${pick(r, TEAM)} ${90 + Math.floor(r() * 30)}-${80 + Math.floor(r() * 30)}; Jason ${pick(r, LAST)} scores ${20 + Math.floor(r() * 20)}`, (r) => `Jason ${pick(r, LAST)} injury update: out for the final stretch`,
    (r) => `${pick(r, TEAM)} season ends in heartbreak as Jason ${pick(r, LAST)} misses the final shot`, (r) => `Coach ${name(r)} betrays no emotion after the loss`, (r) => `Lucia ${pick(r, LAST)} wins gold in the final`, (r) => `Jason ${pick(r, LAST)} retires at the end of the season`,
    (r) => `Final mission for the rookie: stop ${name(r)} in the playoff opener`, (r) => `Rockstar rookie ${name(r)} impresses in preseason`, (r) => `The twist in the standings: ${pick(r, TEAM)} slip to sixth`, (r) => `Transfer leak: ${name(r)} close to a move`,
    (r) => `Fighter ${name(r)} dies at 29, sport mourns`, (r) => `${pick(r, TEAM)} fans plot protest at final home game`, (r) => `Jason ${pick(r, LAST)} on ending his drought: it's been a long road`, (r) => `Lucia ${pick(r, LAST)} named MVP at the end of the tournament`,
  ],
  film_tv: [
    (r) => `${pick(r, SHOW)} finale: ending explained, spoilers`, (r) => `Who dies in ${pick(r, SHOW)} season finale? Spoilers ahead`, (r) => `[SPOILERS] ${pick(r, SHOW)} twist ending thread`, (r) => `Review: ${pick(r, SHOW)} sticks the landing with a devastating final act`,
    (r) => `The betrayal in ${pick(r, SHOW)} episode 8 is the best twist of the year`, (r) => `Post-credits scene in ${pick(r, SHOW)} sets up the sequel`, (r) => `Leaked script for the next ${pick(r, ["Bond", "Batman", "Star Wars"])} film reveals the villain`,
    (r) => `${pick(r, SHOW)} ending: why Jason ${pick(r, LAST)}'s character had to die`, (r) => `Lucia ${pick(r, LAST)} on playing the traitor in the final episode`, (r) => `Epilogue: what happens after the credits in ${pick(r, SHOW)}`,
    (r) => `Actor Jason ${pick(r, LAST)} on his final scene: "I cried"`, (r) => `Netflix cancels ${pick(r, SHOW)} after one season; the ending is a cliffhanger`, (r) => `Full recap of the final mission-style heist in ${pick(r, SHOW)}`, (r) => `Everything you missed in the ${pick(r, SHOW)} ending`,
  ],
  social: [
    (r) => `lol Jason is late again, he always dies when it's his turn to drive`, (r) => `Lucia and I are going to the lake this weekend`, (r) => `the ending of that movie last night!!! i can't even`, (r) => `omg that twist`, (r) => `my phone is dying, will text at the end of my shift`,
    (r) => `final mission: clean the garage. wish me luck`, (r) => `dead. laughing. he betrayed me by eating my fries`, (r) => `Lucia's birthday dinner at 7`, (r) => `Jason, I'm outside`, (r) => `plot twist: it's still raining`, (r) => `New leak in my roof. Anyone know a plumber in ${pick(r, CITY)}?`,
    (r) => `Rockstar energy drink: best flavour?`, (r) => `finished ${pick(r, GAME)} last night, still thinking about the ending`, (r) => `${name(r)} shared a post: "end of an era"`, (r) => `Jason Statham movie night, who's in?`, (r) => `my cat Lucia dies every time she sees the vacuum`,
    (r) => `the game ends at 9, then the after party`, (r) => `Anyone else's day-one patch taking forever?`, (r) => `Spoiler: I'm not cooking tonight`, (r) => `sixth day of the diet, no spoilers on the scale`, (r) => `GT A6 audi for sale, one owner`, (r) => `Audi A6 service due`,
  ],
};

export function runFpStorm({ log = console.log } = {}) {
  const r = rng(20261117);
  const pages = {};
  const per = { news: 220, toronto: 160, reddit_games: 220, sports: 180, film_tv: 200, social: 260 };
  const result = { pages: [], totals: { units: 0, strictLaunch: 0, strictCold: 0, lockdownLaunch: 0, balancedLaunch: 0 }, shielded: [] };
  for (const [k, n] of Object.entries(per)) {
    const seen = new Set();
    const units = [];
    let guard = 0;
    while (units.length < n && guard++ < n * 20) { const t = pick(r, BANK[k])(r); if (!seen.has(t)) { seen.add(t); units.push(t); } }
    let sL = 0, sC = 0, lL = 0, bL = 0;
    const flagged = [];
    for (const t of units) {
      const rl = SFW.classify.text(t, { hot: 0, launchWindow: true, source: "text" });
      const rc = SFW.classify.text(t, { hot: 0, source: "text" });
      const dS = SFW.policy.decide(rl, { mode: "strict", hot: 0, kind: "item" });
      if (dS === "shield") { sL++; flagged.push({ text: t, level: rl.level }); }
      if (SFW.policy.decide(rc, { mode: "strict", hot: 0, kind: "item" }) === "shield") sC++;
      if (SFW.policy.decide(rl, { mode: "lockdown", hot: 0, kind: "item" }) === "shield") lL++;
      if (SFW.policy.decide(rl, { mode: "balanced", hot: 0, kind: "item" }) === "shield") bL++;
    }
    pages[k] = { units: units.length, strictLaunch: sL, strictCold: sC, lockdownLaunch: lL, balancedLaunch: bL, strictLaunchRate: sL / units.length, flagged };
    result.pages.push({ page: k, units: units.length, strictLaunch: sL, strictCold: sC, lockdownLaunch: lL, balancedLaunch: bL, strictLaunchRatePct: +(100 * sL / units.length).toFixed(1), lockdownRatePct: +(100 * lL / units.length).toFixed(1) });
    result.totals.units += units.length; result.totals.strictLaunch += sL; result.totals.strictCold += sC; result.totals.lockdownLaunch += lL; result.totals.balancedLaunch += bL;
    for (const f of flagged) result.shielded.push({ page: k, ...f });
    log(`  ${k.padEnd(14)} units=${String(units.length).padEnd(4)} strict(launch)=${String(sL).padEnd(3)} ${(100 * sL / units.length).toFixed(1)}%  strict(cold)=${sC}  lockdown(launch)=${lL} ${(100 * lL / units.length).toFixed(1)}%  balanced=${bL}`);
  }
  const t = result.totals;
  result.strictLaunchRatePct = +(100 * t.strictLaunch / t.units).toFixed(2);
  result.lockdownRatePct = +(100 * t.lockdownLaunch / t.units).toFixed(2);
  log(`  TOTAL units=${t.units} strict(launch) ${result.strictLaunchRatePct}% strict(cold) ${(100 * t.strictCold / t.units).toFixed(2)}% lockdown(launch) ${result.lockdownRatePct}% balanced ${(100 * t.balancedLaunch / t.units).toFixed(2)}%`);
  return result;
}

// ---- browser pass: warm-feed cascade ------------------------------------------------------------------------------------
const ORDINARY = ["Lovely weather today, going for a long hike with the dog.", "New video: five pasta tips that will change your dinner.", "Anyone else excited for the weekend? Coffee first.", "Raptors won last night, what a game.", "Looking for a good plumber in Brampton.", "Best budget laptop for students this year?", "Photos from my trip to Lisbon.", "Reminder: clocks change this Sunday.", "Recipe: lemon chicken with roasted potatoes.", "Does anyone know a good dentist?", "Ended the week with a long run, feeling great.", "Our cat just learned to open doors.", "Final exams start Monday, wish me luck.", "Check out my new bike.", "Cooking pasta for ten people tonight."];
const TRIGGERS = ["Jason dies at the end of Friday the 13th Part 2, classic horror.", "my cousin Jason betrayed me by eating my leftovers at the end of the party", "Lucia dies in the final scene of the opera, standing ovation", "Lucia betrays Edgardo at the end, Donizetti", "Jason Todd dies in A Death in the Family and then returns, comic fans know"];

function feedHtml(nTrigger) {
  const items = [];
  for (let i = 0; i < 8; i++) items.push({ t: ORDINARY[i], o: true });
  for (let i = 0; i < nTrigger; i++) items.push({ t: TRIGGERS[i], o: false });
  for (let i = 8; i < 40; i++) items.push({ t: ORDINARY[i % ORDINARY.length] + " #" + i, o: true });
  return `<!doctype html><html><head><meta charset="utf-8"><title>feed</title><style>body{font:16px Arial}article{border:1px solid #ccc;margin:6px;padding:8px}</style></head><body><div data-testid="primaryColumn"><div aria-label="Timeline: Your Home Timeline">${items.map((x, i) => `<div data-testid="cellInnerDiv"><article data-testid="tweet" data-ordinary="${x.o ? 1 : 0}"><div data-testid="tweetText"><span>${x.t}</span></div></article></div>`).join("")}</div></div></body></html>`;
}

export async function runFpStormBrowser({ log = console.log } = {}) {
  const { launch, instrumentedContext } = await import("../e2e/lib.mjs");
  const srv = http.createServer((req, res) => {
    const u = new URL(req.url, "http://x");
    const n = Number(u.searchParams.get("n") || 0);
    res.setHeader("Content-Type", "text/html; charset=utf-8");
    res.end(feedHtml(n));
  });
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  const port = srv.address().port;
  const browser = await launch({ beginFrame: false });
  const out = [];
  for (const n of [0, 2, 3, 5]) {
    for (const host of ["x.com"]) {
      const ctx = await instrumentedContext(browser, { config: { enabled: true, mode: "strict" }, probe: false });
      const page = await ctx.newPage();
      await page.goto(`http://${host}:${port}/home?n=${n}`, { waitUntil: "load" });
      await new Promise((r) => setTimeout(r, 1500));
      const r2 = await page.evaluate(() => {
        const items = [...document.querySelectorAll("article[data-ordinary]")];
        const sh = (e) => !!(e.closest("[data-sfw~=shield]") || e.querySelector("[data-sfw~=shield]"));
        return { total: items.length, ordinaryShielded: items.filter((e) => e.dataset.ordinary === "1" && sh(e)).length, triggerShielded: items.filter((e) => e.dataset.ordinary === "0" && sh(e)).length, ordinary: items.filter((e) => e.dataset.ordinary === "1").length, stats: window.__sfw && window.__sfw.ctl && window.__sfw.ctl.stats().shields };
      });
      out.push({ host, triggersInFeed: n, ...r2 });
      log(`  ${host} feed with ${n} FP-prone posts among ${r2.ordinary} ordinary: ordinary posts shielded = ${r2.ordinaryShielded}, FP-prone shielded = ${r2.triggerShielded}`);
      await ctx.close();
    }
  }
  await browser.close();
  await new Promise((r) => srv.close(r));
  return out;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  console.log("FP storm (node):");
  const a = runFpStorm();
  for (const f of a.shielded.slice(0, 60)) console.log(`   [${f.page}] (${f.level}) ${f.text}`);
  console.log("FP storm (browser warm-feed cascade):");
  await runFpStormBrowser();
}
export const __feedHtml = feedHtml;
