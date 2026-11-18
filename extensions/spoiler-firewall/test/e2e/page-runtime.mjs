// Page-runtime regressions (red-team C-1, C-2, C-3, H-2, M-1, M-4 and the L-1 reveal finding). Headless only (chrome-headless-shell,
// muted, below-normal priority), no OS input: every click here is CDP input dispatched into the page.
//   frames   sandboxed iframes without allow-scripts (the engine cannot run in them): srcdoc / data: text is classified by the
//            parent, src-only frames are covered by policy (hot / high-risk / large on a cold page in strict / lockdown)
//   popups   window.open('') + opener document.write / DOM building: the engine follows the new Document, no silent failure
//   start    protection start failures are never silent (page shield on hot pages, visible notice elsewhere)
//   hostile  builtins captured at load: patches after the bundle cannot switch protection off
//   warm     feed warmth only from franchise / strong-entity shields, never from weak-name shields
//   perf     huge insertions never hang the page; work is budgeted, covered while pending and finishes
//   reveal   a script-synthesised click never reveals
// usage: node test/e2e/page-runtime.mjs [--only=substring] [--perf-scale=1]
import assert from "node:assert/strict";
import http from "node:http";
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { startServer } from "../fixtures/server.mjs";
import { launch, instrumentedContext } from "./lib.mjs";
import { testBundle } from "./bundle.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
mkdirSync(join(HERE, "out"), { recursive: true });
const arg = (k, d) => { const a = process.argv.find((x) => x.startsWith(`--${k}=`)); return a ? a.split("=")[1] : d; };
const only = arg("only", "");
const perfScale = Number(arg("perf-scale", "1"));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const srv = await startServer();
const browser = await launch({ beginFrame: false });
const url = (host, path) => `http://${host}:${srv.portA}${path}`;
const results = [];
const numbers = {};

async function test(name, fn) {
  if (only && !name.includes(only)) return;
  const t0 = Date.now();
  try {
    await fn();
    results.push({ name, ok: true, ms: Date.now() - t0 });
    console.log(`ok   ${name}`);
  } catch (e) {
    results.push({ name, ok: false, error: String(e && e.message).split("\n")[0].slice(0, 400) });
    console.log(`FAIL ${name}: ${String(e && e.message).split("\n")[0].slice(0, 400)}`);
  }
}

async function open(host, path, { config = {}, init = null, wait = 600 } = {}) {
  const ctx = await instrumentedContext(browser, { config, probe: false });
  const page = await ctx.newPage();
  await page.goto(url(host, path), { waitUntil: "load" });
  await sleep(wait);
  return { ctx, page };
}

const shieldState = (page, sel) => page.evaluate((s) => { const e = document.querySelector(s); const SFW = window.__sfw.SFW; return e ? SFW.shield.state(e) : null; }, sel);

// ---------------------------------------------------------------------------------------------------------------
// C-1: frames
await test("frames: sandboxed srcdoc frame (no allow-scripts) is classified by the parent and shielded", async () => {
  const { ctx, page } = await open("example.test", "/pages/rt-sandbox-srcdoc.html");
  const st = await shieldState(page, "#wrap");
  assert.equal(st.mode, "shielded");
  assert.equal(st.variant, "spoiler", "the srcdoc text itself is a spoiler: ordinary spoiler cover, not the 'couldn't check' cover");
  await ctx.close();
});

await test("frames: src-only sandboxed frame: large on a cold page is covered in strict, with the 'Embedded content couldn't be checked' cover", async () => {
  const { ctx, page } = await open("example.test", "/pages/rt-sandbox-src.html");
  const st = await shieldState(page, "#wrap");
  assert.equal(st.mode, "shielded");
  assert.equal(st.variant, "frameerr");
  const text = await page.evaluate(() => window.__sfw.SFW.shield.TEXT.frameErr);
  assert.match(text, /Embedded content couldn.t be checked/);
  await ctx.close();
});

await test("frames: a small sandboxed src frame on a cold page is covered in strict (it can show text); a 1x1 pixel frame is not", async () => {
  const a = await open("example.test", "/pages/rt-sandbox-small.html");
  assert.equal((await shieldState(a.page, "#wrap")).variant, "frameerr", "150x60 frame, cold page, strict: covered (cannot be inspected)");
  assert.equal((await shieldState(a.page, "#pixel")).mode, "none", "1x1 tracking-pixel frame stays clear");
  await a.ctx.close();
  const b = await open("example.test", "/pages/rt-sandbox-small.html", { config: { mode: "lockdown" } });
  assert.equal((await shieldState(b.page, "#wrap")).variant, "frameerr", "lockdown covers every unverifiable frame");
  await b.ctx.close();
});

await test("frames: balanced does not cover a large frame on a cold page; strict does (false-positive guard)", async () => {
  const a = await open("example.test", "/pages/rt-sandbox-src.html", { config: { mode: "balanced" } });
  assert.equal((await shieldState(a.page, "#wrap")).mode, "none");
  await a.ctx.close();
});

await test("frames: any unverifiable frame is covered in hot context (small frame, hot h1)", async () => {
  const { ctx, page } = await open("example.test", "/pages/rt-sandbox-hot.html");
  assert.equal((await shieldState(page, "#wrap")).mode, "shielded");
  await ctx.close();
});

await test("frames: high-risk host covers a sandboxed src frame even when small", async () => {
  const ctx = await instrumentedContext(browser, { config: {}, probe: false });
  const page = await ctx.newPage();
  const server = http.createServer((req, res) => {
    res.setHeader("Content-Type", "text/html; charset=utf-8");
    res.end(`<!doctype html><html><head><title>t</title></head><body><article data-testid="tweet" id="t1"><div data-testid="tweetText">Nice weather.</div><div id="w"><iframe sandbox src="about:blank" width="120" height="50"></iframe></div></article></body></html>`);
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  await page.goto(`http://x.com:${server.address().port}/home`, { waitUntil: "load" }).catch(() => {});
  await sleep(700);
  const st = await page.evaluate(() => { const e = document.getElementById("t1"); return window.__sfw.SFW.shield.state(e); });
  assert.equal(st.mode, "shielded");
  assert.equal(st.variant, "frameerr");
  await ctx.close();
  await new Promise((r) => server.close(r));
});

await test("frames: frames that can run the engine (allow-scripts, plain cross-origin) are not covered by the parent", async () => {
  const { ctx, page } = await open("example.test", "/pages/rt-frame-scripts.html");
  assert.equal((await shieldState(page, "#wrap")).mode, "none");
  assert.equal((await shieldState(page, "#wrap2")).mode, "none");
  const inner = await Promise.all(page.frames().filter((f) => f !== page.mainFrame()).map((f) => f.evaluate(() => ({ engine: !!window.__sfw, shields: document.querySelectorAll("[data-sfw~=shield]").length })).catch(() => null)));
  assert.ok(inner.some((i) => i && i.engine && i.shields >= 1), "the engine inside the frame shields its own spoiler");
  await ctx.close();
});

await test("frames: data:text/html (plain and base64) and srcdoc text, tags stripped by a tokenizer, never parsed into the live document", async () => {
  const ctx = await instrumentedContext(browser, { config: {}, probe: false });
  const page = await ctx.newPage();
  const b64 = Buffer.from("<p>GTA 6 ending spoiler: the hero dies</p>").toString("base64");
  const body = `<div id="a"><iframe sandbox src="data:text/html,%3Cp%3EGTA%206%20ending%20spoiler%3A%20the%20hero%20dies%3C%2Fp%3E" width="300" height="80"></iframe></div>
<div id="b"><iframe sandbox src="data:text/html;base64,${b64}" width="300" height="80"></iframe></div>
<div id="c"><iframe sandbox srcdoc="&lt;img alt='GTA 6 final mission ending spoiler'&gt;" width="300" height="80"></iframe></div>
<div id="d"><iframe sandbox srcdoc="&lt;script&gt;var x='GTA 6 ending spoiler'&lt;/script&gt;&lt;!-- GTA 6 ending spoiler --&gt;&lt;p&gt;a quiet garden&lt;/p&gt;" width="150" height="50"></iframe></div>
<div id="e"><iframe sandbox srcdoc="&lt;p&gt;G&lt;b&gt;T&lt;/b&gt;A 6 ending spoiler: he dies&lt;/p&gt;" width="150" height="50"></iframe></div>`;
  await page.setContent(`<!doctype html><html><head><title>t</title></head><body>${body}</body></html>`);
  await sleep(600);
  const r = await page.evaluate(() => { const S = window.__sfw.SFW.shield; return ["a", "b", "c", "d", "e"].map((id) => ({ id, ...S.state(document.getElementById(id)) })); });
  assert.equal(r[0].variant, "spoiler", "data: URL, percent-encoded");
  assert.equal(r[1].variant, "spoiler", "data: URL, base64");
  assert.equal(r[2].variant, "spoiler", "words in an alt attribute inside srcdoc");
  assert.equal(r[3].mode, "none", "script bodies and comments are not text; small clean frame stays clear");
  assert.equal(r[4].variant, "spoiler", "a word split by inline tags inside srcdoc");
  await ctx.close();
});

// ---------------------------------------------------------------------------------------------------------------
// C-2: popups
async function popupScenario(openerJs) {
  const ctx = await instrumentedContext(browser, { config: {}, probe: false });
  const page = await ctx.newPage();
  const pops = [];
  ctx.on("page", (p) => { if (p !== page) pops.push(p); });
  const server = http.createServer((req, res) => { res.setHeader("Content-Type", "text/html"); res.end(`<!doctype html><html><head><title>opener</title></head><body><button id="go">go</button><script>document.getElementById('go').onclick=async function(){ window.__r = await (async function(){${openerJs}})() }</script></body></html>`); });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  await page.goto(`http://example.test:${server.address().port}/`, { waitUntil: "load" });
  await page.click("#go");
  await sleep(900);
  const out = { sameTask: await page.evaluate(() => window.__r) };
  out.pop = await pops[0].evaluate(() => {
    const s = document.querySelector(".sentinel");
    return { engine: !!window.__sfw, reports: window.__sfw && window.__sfw.reports, errors: window.__sfw && window.__sfw.ctl.stats().errors, hasSentinel: !!s, shielded: !!(s && s.closest("[data-sfw~=shield]")), pageShield: document.documentElement.getAttribute("data-sfw-page") };
  });
  await ctx.close();
  await new Promise((r) => server.close(r));
  return out;
}

const SENT = `GTA 6 ending spoiler: the hero dies in the final mission`;
await test("popups: window.open('') + document.write: engine follows the new document, spoiler shielded, nothing silent", async () => {
  const o = await popupScenario(`var w=window.open('','_blank','width=640,height=200');w.document.write('<style>.sentinel{color:#ff00ff}</style><p class="sentinel">${SENT}</p>');w.document.close();await Promise.resolve();var s=w.document.querySelector('.sentinel');return {immediately: !!(s&&s.closest('[data-sfw~=shield]'))}`);
  assert.ok(o.pop.engine && o.pop.hasSentinel);
  assert.equal(o.pop.shielded, true, "sentinel is inside a shield");
  assert.equal(o.pop.errors, 0);
  assert.ok(!o.pop.reports["error.start"], "no start failure");
  assert.equal(o.sameTask.immediately, true, "shielded at the first microtask checkpoint after the opener's write, i.e. before any frame");
});

await test("popups: window.open('about:blank') + DOM building by the opener is shielded", async () => {
  const o = await popupScenario(`var w=window.open('about:blank','_blank','width=640,height=200');var p=w.document.createElement('p');p.className='sentinel';p.textContent="${SENT}";w.document.body.appendChild(p);await Promise.resolve();return {immediately: !!p.closest('[data-sfw~=shield]')}`);
  assert.equal(o.pop.shielded, true);
  assert.ok(!o.pop.reports["error.start"]);
  assert.equal(o.sameTask.immediately, true);
});

await test("popups: late writes (after a delay), repeated document.open and a clean popup", async () => {
  const o = await popupScenario(`var w=window.open('','_blank','width=640,height=200');await new Promise(function(r){setTimeout(r,250)});w.document.open();w.document.write('<p>a quiet garden</p>');w.document.close();w.document.open();w.document.write('<p class="sentinel">${SENT}</p>');w.document.close();await Promise.resolve();return {}`);
  assert.equal(o.pop.shielded, true);
  assert.equal(o.pop.errors, 0);
});

// ---------------------------------------------------------------------------------------------------------------
// start failures are never silent; C-3 / H-2
async function hostileOpen({ pre = "", host = "example.test", path = "/", hot = false }) {
  const ctx = await browser.newContext({ viewport: { width: 640, height: 420 } });
  if (pre) await ctx.addInitScript({ content: pre });
  await ctx.addInitScript({ content: testBundle({}, "real") });
  const page = await ctx.newPage();
  const server = http.createServer((req, res) => {
    res.setHeader("Content-Type", "text/html; charset=utf-8");
    res.end(`<!doctype html><html><head><title>garden notes</title></head><body><article id="c1">Lovely weather today, going for a long hike with the dog.</article>${hot ? `<article id="s1">GTA 6 ending spoiler: the hero dies in the final mission</article>` : ""}<article id="c2">Five pasta tips that will change your dinner.</article><div id="host"></div><script>setTimeout(function(){var a=document.createElement('article');a.id='dyn';a.textContent='${hot ? "GTA VI leaked ending: everyone dies at the finale" : "A later ordinary post about gardening."}';document.getElementById('host').appendChild(a)},200)</script></body></html>`);
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  await page.goto(`http://${host}:${server.address().port}${path}`, { waitUntil: "load" }).catch(() => {});
  await sleep(900);
  const info = await page.evaluate(() => ({
    shields: document.querySelectorAll("[data-sfw~=shield]").length,
    pageShield: document.documentElement.getAttribute("data-sfw-page"),
    notices: document.querySelectorAll("sfw-shield[data-s=notice], [data-sfw-emergency=notice]").length,
    cover: document.querySelectorAll("[data-sfw-emergency]:not([data-sfw-emergency=notice])").length,
    reports: window.__sfw && window.__sfw.reports,
    spoilerShielded: ["s1", "dyn"].map((id) => { const e = document.getElementById(id); return !!(e && e.closest("[data-sfw~=shield], [data-sfw-defer]")); }),
    c1Shielded: !!document.getElementById("c1").closest("[data-sfw~=shield]"),
  })).catch((e) => ({ evalError: String(e.message).split("\n")[0] }));
  await ctx.close();
  await new Promise((r) => server.close(r));
  return info;
}

await test("start failure, classifier broken before the bundle (normalize throws): cold page gets a small notice, never a page shield (H-2)", async () => {
  const i = await hostileOpen({ pre: `String.prototype.normalize=function(){throw new RangeError('x')}` });
  assert.equal(i.pageShield, null);
  assert.equal(i.shields, 0, "no ordinary unit is shielded");
  assert.ok(i.notices >= 1, "a visible notice says protection is not working");
  assert.ok(i.reports["error.detect"] >= 1, "reported");
});

await test("start failure, classifier broken on a high-risk host: page shield (fail closed)", async () => {
  const i = await hostileOpen({ pre: `String.prototype.codePointAt=undefined;String.fromCodePoint=undefined`, host: "x.com", path: "/home" });
  assert.equal(i.pageShield, "shield");
  assert.ok(i.reports["error.detect"] >= 1);
});

await test("start failure, Prototype.js-style Object.prototype pollution before the bundle: cold page is not blocked", async () => {
  const i = await hostileOpen({ pre: `Array.prototype.each=function(){};Object.prototype.extend=function(){return this};Object.prototype.polluted='x';Object.prototype.length=3;Object.prototype.hot=2;Object.prototype.flags={franchise:true}` });
  assert.equal(i.pageShield, null);
  assert.equal(i.shields, 0);
  assert.equal(i.cover, 0);
});

await test("start failure, CSSStyleSheet missing before the bundle: fails loudly (notice on a cold page, cover on a high-risk one), never silent", async () => {
  const pre = `window.CSSStyleSheet=undefined;try{delete Document.prototype.adoptedStyleSheets;delete ShadowRoot.prototype.adoptedStyleSheets}catch(e){}`;
  const cold = await hostileOpen({ pre });
  assert.equal(cold.pageShield, null);
  assert.equal(cold.cover, 0, "no full-page cover on a cold page");
  assert.ok(cold.notices >= 1);
  assert.ok(cold.reports["error.start"] >= 1);
  const risky = await hostileOpen({ pre, host: "x.com", path: "/home" });
  assert.ok(risky.cover >= 1 || risky.pageShield === "shield", "high-risk host: content covered");
});

await test("start failure on a cold page whose content turns out to be GTA VI: the notice is upgraded to a cover (fail closed), ordinary pages keep the notice", async () => {
  for (const pre of [`String.prototype.normalize=function(){throw new RangeError('x')}`, `window.CSSStyleSheet=undefined;try{delete Document.prototype.adoptedStyleSheets;delete ShadowRoot.prototype.adoptedStyleSheets}catch(e){}`, `Object.prototype.polluted='x';Array.prototype.each=function(){};String.prototype.codePointAt=undefined`]) {
    const hot = await hostileOpen({ pre, hot: true });
    assert.ok(hot.cover >= 1 || hot.pageShield === "shield", "GTA VI content with protection down: covered (" + pre.slice(0, 30) + ")");
    const cold = await hostileOpen({ pre });
    assert.equal(cold.cover, 0, "ordinary content: no cover (" + pre.slice(0, 30) + ")");
    assert.equal(cold.pageShield, null);
    assert.ok(cold.notices >= 1);
  }
});

await test("MutationObserver dead / throwing before the bundle: degraded rescan mode still shields (C-3), reported", async () => {
  for (const pre of [`window.MutationObserver=function(){this.observe=function(){};this.disconnect=function(){};this.takeRecords=function(){return[]}}`, `window.MutationObserver=function(){throw new Error('MO blocked')}`]) {
    const i = await hostileOpen({ pre, hot: true });
    assert.ok(i.reports["degraded.poll"] >= 1, "degraded mode is reported");
    assert.deepEqual(i.spoilerShielded, [true, true], "static and dynamic spoilers are covered");
    assert.equal(i.c1Shielded, false, "ordinary content is not");
  }
});

await test("builtins patched AFTER the bundle (Element methods, MutationObserver, adoptedStyleSheets, CSSStyleSheet) cannot switch protection off", async () => {
  const pre = "";
  const ctx = await browser.newContext({ viewport: { width: 640, height: 420 } });
  await ctx.addInitScript({ content: testBundle({}, "real") });
  const page = await ctx.newPage();
  const server = http.createServer((req, res) => { res.setHeader("Content-Type", "text/html"); res.end(`<!doctype html><html><head><title>t</title></head><body><article id="c1">Lovely weather today.</article><div id="host"></div><script>
    var P=Element.prototype;['closest','matches','querySelector','querySelectorAll','getAttribute','hasAttribute','setAttribute','removeAttribute','getBoundingClientRect'].forEach(function(n){var o=P[n];P[n]=function(){return n==='matches'?false:n==='querySelectorAll'?[]:n==='getBoundingClientRect'?{x:0,y:0,width:0,height:0,top:0,left:0,right:0,bottom:0}:n.indexOf('Attribute')>0?(n==='setAttribute'?undefined:null):null}});
    window.MutationObserver=function(){this.observe=function(){};this.disconnect=function(){};this.takeRecords=function(){return[]}};
    try{delete Document.prototype.adoptedStyleSheets;delete ShadowRoot.prototype.adoptedStyleSheets}catch(e){};window.CSSStyleSheet=undefined;
    </script></body></html>`); });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  await page.goto(`http://example.test:${server.address().port}/`, { waitUntil: "load" });
  await sleep(300);
  await page.evaluate(() => { const a = document.createElement("article"); a.id = "late"; a.textContent = "GTA 6 ending spoiler: the hero dies in the final mission"; document.getElementById("host").appendChild(a); const b = document.createElement("article"); b.id = "late2"; b.textContent = "Five pasta tips that will change your dinner."; document.getElementById("host").appendChild(b); });
  await sleep(600);
  const r = await page.evaluate(() => ({ late: document.getElementById("late").dataset.sfw, clean: document.getElementById("late2").dataset.sfw, c1: document.getElementById("c1").dataset.sfw, errors: window.__sfw.ctl.stats().errors, reports: window.__sfw.reports }));
  assert.equal(r.late, "shield");
  assert.equal(r.clean, undefined, "ordinary new content is not shielded (cssstylesheet-missing, H-2)");
  assert.equal(r.c1, undefined);
  assert.equal(r.errors, 0);
  await ctx.close();
  await new Promise((r2) => server.close(r2));
  void pre;
});

// ---------------------------------------------------------------------------------------------------------------
// M-1: warm-feed cascade
function feedHtml(posts) {
  return `<!doctype html><html><head><meta charset="utf-8"><title>feed</title><style>body{font:16px Arial}article{border:1px solid #ccc;margin:6px;padding:8px}</style></head><body><div data-testid="primaryColumn"><div aria-label="Timeline: Your Home Timeline">${posts.map((p) => `<div data-testid="cellInnerDiv"><article data-testid="tweet" data-kind="${p.kind}"><div data-testid="tweetText"><span>${p.t}</span></div>${p.img ? `<img alt="" width="200" height="120" src="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='200' height='120'%3E%3Crect width='200' height='120' fill='%23ccc'/%3E%3C/svg%3E">` : ""}</article></div>`).join("")}</div></div></body></html>`;
}
const ORDINARY = ["Lovely weather today, going for a long hike with the dog.", "New video: five pasta tips that will change your dinner.", "Anyone else excited for the weekend? Coffee first.", "Raptors won last night, what a game.", "Looking for a good plumber in Brampton.", "Best budget laptop for students this year?", "Photos from my trip to Lisbon.", "Reminder: clocks change this Sunday.", "Recipe: lemon chicken with roasted potatoes.", "Does anyone know a good dentist?"];
const CLEAN = ORDINARY.filter((t) => !/game|Ended|Final|Raptors/.test(t)).concat(["Check out my new bike."]); // no weak cue word: stays clear even in a warm (hot=1) feed
const WEAK = ["WEAKSHIELD Jason dies at the end of Friday the 13th Part 2, classic horror.", "WEAKSHIELD my cousin Jason betrayed me by eating my leftovers at the end of the party", "WEAKSHIELD Lucia dies in the final scene of the opera, standing ovation"];
const STRONG = ["STRONGSHIELD GTA 6 ending leaked: Jason Duval dies in the final mission", "STRONGSHIELD Leaked GTA VI spoiler: Lucia Caminos betrays Jason in the last mission", "STRONGSHIELD GTA 6 story leak: the ending of the game revealed"];
async function feed(posts) {
  const server = http.createServer((req, res) => { res.setHeader("Content-Type", "text/html; charset=utf-8"); res.end(feedHtml(posts)); });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  const ctx = await instrumentedContext(browser, { config: { enabled: true, mode: "strict" }, probe: false });
  // Synthetic classification (independent of detection tuning): WEAKSHIELD = a shield whose only signals are a weak name + a cue
  // (the "Jason dies at the end of Friday the 13th" class); STRONGSHIELD = a franchise + strong-entity shield. Everything else is
  // classified by the real detection core.
  await ctx.addInitScript({ content: `(function(){var S=window.__sfw&&window.__sfw.SFW;if(!S)return;var o=S.classify.text;S.classify.text=function(t,c){var r;if(typeof t==='string'&&t.indexOf('WEAKSHIELD')>=0)return{score:0.95,level:'spoiler',signals:[],len:t.length,flags:{franchise:false,entity:false,entityWeak:true,cue:true,cueWeak:true,marker:false,safe:false}};if(typeof t==='string'&&t.indexOf('STRONGSHIELD')>=0)return{score:0.99,level:'spoiler',signals:[],len:t.length,flags:{franchise:true,entity:true,entityWeak:false,cue:true,cueWeak:true,marker:false,safe:false}};return o.apply(this,arguments)}})()` });
  const page = await ctx.newPage();
  await page.goto(`http://x.com:${server.address().port}/home`, { waitUntil: "load" });
  await sleep(1500);
  const r = await page.evaluate(() => {
    const items = [...document.querySelectorAll("article[data-kind]")];
    const sh = (e) => !!(e.closest("[data-sfw~=shield]") || e.querySelector("[data-sfw~=shield]"));
    const by = (k) => items.filter((e) => e.dataset.kind === k);
    return { ordinaryShielded: by("ordinary").filter(sh).length, ordinary: by("ordinary").length, mediaShielded: by("media").filter(sh).length, media: by("media").length, mediaReleased: by("media").filter((e) => e.querySelector("img") && e.querySelector("img").getAttribute("data-sfw-m") === "ok").length, weakShielded: by("weak").filter(sh).length, mediaweakShielded: by("mediaweak").filter(sh).length, strongShielded: by("strong").filter(sh).length, strong: by("strong").length, warm: window.__sfw.ctl.stats().warm };
  });
  await ctx.close();
  await new Promise((rr) => server.close(rr));
  return r;
}
await test("warm feed: three weak-name shields (Jason/Lucia in other stories) do not make the feed warm; no ordinary post is shielded (M-1)", async () => {
  const posts = [];
  for (let i = 0; i < 40; i++) {
    if (i === 8) for (const t of WEAK) posts.push({ kind: "weak", t });
    posts.push({ kind: i % 7 === 3 ? "media" : "ordinary", t: ORDINARY[i % ORDINARY.length] + " #" + i, img: i % 7 === 3 });
  }
  const r = await feed(posts);
  assert.equal(r.warm, false, "weak shields do not raise warmth");
  assert.equal(r.ordinaryShielded, 0);
  assert.equal(r.mediaShielded, 0);
  assert.equal(r.weakShielded, 3, "the weak-name posts themselves are still shielded");
  numbers.warmWeakOrdinaryShielded = r.ordinaryShielded;
});

await test("warm feed: franchise shields make it warm (statistics only); ordinary posts, with or without media or an everyday cue word, are not shielded", async () => {
  const posts = [];
  for (let i = 0; i < 30; i++) {
    if (i === 4) for (const t of STRONG) posts.push({ kind: "strong", t });
    posts.push({ kind: i % 5 === 3 ? "media" : "ordinary", t: CLEAN[i % CLEAN.length] + " #" + i, img: i % 5 === 3 });
  }
  posts.push({ kind: "mediaweak", t: "Final exams start Monday, wish me luck.", img: true });
  const r = await feed(posts);
  assert.equal(r.warm, true, "three franchise shields: warm");
  assert.equal(r.strongShielded, 3);
  assert.equal(r.mediaShielded, 0, "neutral media posts are not collateral of a warm feed");
  assert.equal(r.ordinaryShielded, 0);
  assert.ok(r.mediaReleased >= 1, "their media is released, not quarantined forever");
  // Revision 3: warmth is no longer context. In launch week every feed is warm, and "Final exams start Monday" with a photo
  // was covered as GTA VI content.
  assert.equal(r.mediaweakShielded, 0, "an everyday cue word next to a photo is not GTA context, warm feed or not");
});

// ---------------------------------------------------------------------------------------------------------------
// L-1: a script cannot reveal
await test("reveal: a script-synthesised click / keydown / dispatchEvent never reveals; a real click does", async () => {
  const { ctx, page } = await open("x.com", "/home");
  assert.equal(await page.evaluate(() => document.getElementById("spoiler1").getAttribute("data-sfw")), "shield");
  await page.evaluate(() => {
    const o = document.querySelector("#spoiler1 > sfw-shield");
    o.click();
    o.dispatchEvent(new MouseEvent("click", { bubbles: true, composed: true }));
    o.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, composed: true }));
    window.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
  await sleep(300);
  assert.equal(await page.evaluate(() => document.getElementById("spoiler1").getAttribute("data-sfw")), "shield", "synthetic events are ignored");
  const c = await page.evaluate(() => { const r = document.getElementById("spoiler1").getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + Math.min(r.height / 2, 40) }; });
  await page.mouse.click(c.x, c.y);
  await page.waitForFunction(() => document.getElementById("spoiler1").getAttribute("data-sfw") === "revealed", null, { timeout: 4000 });
  await ctx.close();
});

// a precautionary "Checking" cover (budget overflow) must not leave the page's clear video dead
await test("pending cover over a clear unit with an autoplaying video: verdict clear resumes it; a user reveal never does", async () => {
  const ctx = await instrumentedContext(browser, { config: {}, probe: false });
  const page = await ctx.newPage();
  await page.setContent(`<!doctype html><html><head><title>t</title></head><body><article id="a"><span>Cute cat clip</span><video id="v" autoplay muted loop playsinline width="160" height="90"></video></article><article id="b"><span>Another clip</span><video id="v2" autoplay muted loop playsinline width="160" height="90"></video></article><script>function mk(id,col){var c=document.createElement('canvas');c.width=160;c.height=90;var x=c.getContext('2d');x.fillStyle=col;x.fillRect(0,0,160,90);document.getElementById(id).srcObject=c.captureStream(5);}mk('v','#00aa88');mk('v2','#8800aa');</script></body></html>`);
  await sleep(700);
  const r = await page.evaluate(async () => {
    const S = window.__sfw.SFW.shield;
    const a = document.getElementById("a"), v = document.getElementById("v"), b = document.getElementById("b"), v2 = document.getElementById("v2");
    const out = { playingBefore: !v.paused };
    S.apply(a, { variant: "pending", needPos: true });
    out.pausedByCover = v.paused;
    S.release(a);
    await new Promise((res) => setTimeout(res, 400));
    out.playingAfterClear = !v.paused;
    S.apply(b, { variant: "spoiler", needPos: true });
    S.reveal(b);
    S.release(b);
    await new Promise((res) => setTimeout(res, 400));
    out.pausedAfterRevealThenRelease = v2.paused;
    return out;
  });
  assert.equal(r.playingBefore, true, "control: the page's video autoplays");
  assert.equal(r.pausedByCover, true);
  assert.equal(r.playingAfterClear, true, "cleared unit: autoplay restored");
  assert.equal(r.pausedAfterRevealThenRelease, true, "revealed unit: media stays paused");
  await ctx.close();
});

// ---------------------------------------------------------------------------------------------------------------
// M-4: performance and long text
async function perfPage({ enabled, build, N, settleMs = 60000 }) {
  const ctx = await instrumentedContext(browser, { config: { enabled }, probe: false });
  const page = await ctx.newPage();
  await page.setContent(`<!doctype html><html><head><meta charset="utf-8"><style>.sentinel{color:#f0f}</style></head><body><div id="h"></div></body></html>`);
  await sleep(300);
  const m = await page.evaluate(async ({ build, settleMs }) => {
    // structural zero-flash probe at EVERY task boundary: a sentinel must be inside a shield or a region cover
    let bad = 0, scans = 0, stop = false;
    const mc = new MessageChannel();
    mc.port1.onmessage = () => {
      if (stop) return;
      scans++;
      const list = document.getElementsByClassName("sentinel");
      for (let i = 0; i < list.length; i++) { const e = list[i]; if (e.isConnected && !e.closest("[data-sfw~=shield],[data-sfw-defer]")) bad++; }
      setTimeout(() => mc.port2.postMessage(0), 0);
    };
    mc.port2.postMessage(0);
    const t0 = performance.now();
    (0, eval)(build);
    const tScript = performance.now() - t0;
    await new Promise((r) => setTimeout(r, 0));
    const tMacro = performance.now() - t0;
    const t1 = performance.now();
    const ctl = window.__sfw && window.__sfw.ctl;
    let done = null;
    while (performance.now() - t1 < settleMs) {
      await new Promise((r) => setTimeout(r, 100));
      if (!ctl) { if (performance.now() - t1 > 1500) { done = performance.now() - t0; break; } continue; }
      const s = ctl.stats();
      if (!s.workLeft && !s.pending && !s.marks) { done = performance.now() - t0; break; }
    }
    await new Promise((r) => setTimeout(r, 300));
    stop = true;
    const s = ctl ? ctl.stats() : null;
    return { tScript, tMacro, doneMs: done, bad, scans, stats: s && { batches: s.batches, maxBatchMs: s.maxBatchMs, p95BatchMs: s.p95BatchMs, meanBatchMs: s.meanBatchMs, units: s.units, shields: s.shields, pending: s.pending, marks: s.marks, errors: s.errors, workLeft: s.workLeft }, shieldsDom: document.querySelectorAll("[data-sfw~=shield]").length, deferLeft: document.querySelectorAll("[data-sfw-defer]").length, pendingDom: document.querySelectorAll("[data-sfw-fb=pending]").length };
  }, { build, settleMs });
  await ctx.close();
  return m;
}

const listBuild = (n, every) => `var f=document.createDocumentFragment();for(var i=0;i<${n};i++){var li=document.createElement('li');if(i%${every}===777){li.className='sentinel';li.textContent='GTA 6 ending spoiler: he dies #'+i}else li.textContent='ordinary list item about the weather '+String.fromCharCode(97+i%26,97+(i/26|0)%26,97+(i/676|0)%26,97+(i/17576|0)%26);f.appendChild(li)}document.getElementById('h').appendChild(f)`;
for (const [label, n, every, spoilers] of [["25k li", Math.round(25000 * perfScale), 5000, 5], ["100k nodes in one task", Math.round(100000 * perfScale), 5000, 20]]) {
  await test(`perf: ${label}: never hangs, every spoiler covered at every task boundary, work budgeted, pending cleared`, async () => {
    const off = await perfPage({ enabled: false, build: listBuild(n, every), settleMs: 5000 });
    const on = await perfPage({ enabled: true, build: listBuild(n, every) });
    numbers[label] = { off: { tScript: Math.round(off.tScript), tMacro: Math.round(off.tMacro) }, on: { tScript: Math.round(on.tScript), tMacro: Math.round(on.tMacro), doneMs: on.doneMs && Math.round(on.doneMs), maxBatchMs: on.stats && Math.round(on.stats.maxBatchMs), p95BatchMs: on.stats && Math.round(on.stats.p95BatchMs), batches: on.stats && on.stats.batches, units: on.stats && on.stats.units } };
    console.log("     ", JSON.stringify(numbers[label]));
    assert.ok(on.doneMs != null, "work finished (no hang): " + JSON.stringify(on.stats));
    assert.equal(on.bad, 0, `a sentinel was outside shield/cover at ${on.bad} of ${on.scans} task boundaries`);
    assert.equal(on.stats.errors, 0);
    assert.equal(on.shieldsDom, spoilers, "exactly the spoiler items are shielded");
    assert.equal(on.deferLeft, 0, "region covers lifted");
    assert.equal(on.pendingDom, 0);
    assert.ok(on.stats.maxBatchMs <= 100, `max engine batch ${on.stats.maxBatchMs} ms`);
  });
}

await test("perf: one very long text node is classified in windows (spoiler at the end and in the middle found), batches stay short, unit covered meanwhile", async () => {
  const mk = (mid, n) => `var w='ordinary words about the weather and the news of the day ';var t='';while(t.length<${n})t+=w;var a=document.createElement('article');a.className='sentinel';a.textContent=${mid ? `t.slice(0,${Math.floor(n / 2)})+' GTA 6 ending spoiler: he dies. '+t.slice(${Math.floor(n / 2)})` : `t+' GTA 6 ending spoiler: he dies'`};document.getElementById('h').appendChild(a)`;
  for (const [label, mid, n] of [["150k, spoiler in the middle", true, 150000], ["190k, spoiler at the end", false, 190000]]) {
    const on = await perfPage({ enabled: true, build: mk(mid, n) });
    console.log("     ", label, JSON.stringify({ doneMs: on.doneMs && Math.round(on.doneMs), maxBatchMs: on.stats && Math.round(on.stats.maxBatchMs), batches: on.stats && on.stats.batches }));
    assert.ok(on.doneMs != null, label + ": finished");
    assert.equal(on.shieldsDom, 1, label + ": shielded");
    assert.equal(on.bad, 0, label + ": covered at every task boundary");
    assert.ok(on.stats.maxBatchMs <= 100, `${label}: max batch ${on.stats.maxBatchMs} ms`);
  }
});

await browser.close();
await srv.close();
const failed = results.filter((r) => !r.ok);
writeFileSync(join(HERE, "out", "page-runtime.json"), JSON.stringify({ results, numbers }, null, 1));
console.log(`\n${results.length - failed.length}/${results.length} page-runtime tests passed`);
process.exit(failed.length ? 1 : 0);
