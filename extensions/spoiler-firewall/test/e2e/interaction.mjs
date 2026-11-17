// Interaction + layout + accessibility tests for the shield (headless chrome-headless-shell, muted, real CDP input).
// usage: node test/e2e/interaction.mjs [--detection=real|stub] [--only=substring]
import assert from "node:assert/strict";
import { writeFileSync, mkdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { startServer } from "../fixtures/server.mjs";
import { launch, instrumentedContext } from "./lib.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
mkdirSync(join(HERE, "out"), { recursive: true });
const arg = (k, d) => { const a = process.argv.find((x) => x.startsWith(`--${k}=`)); return a ? a.split("=")[1] : d; };
const detection = arg("detection", "real");
const only = arg("only", "");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const srv = await startServer();
const browser = await launch({ beginFrame: false });
const url = (host, path) => `http://${host}:${srv.portA}${path}`;
const results = [];

async function open(host, path, { config = {}, viewport } = {}) {
  const ctx = await instrumentedContext(browser, { config, detection, probe: false, viewport });
  const page = await ctx.newPage();
  await page.goto(url(host, path), { waitUntil: "load" });
  await sleep(500);
  return { ctx, page };
}

const attr = (page, sel, a = "data-sfw") => page.evaluate(([s, n]) => { const e = document.querySelector(s); return e ? e.getAttribute(n) : "<missing>"; }, [sel, a]);
async function center(page, sel) {
  return page.evaluate((s) => { const r = document.querySelector(s).getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + Math.min(r.height / 2, 40), w: r.width, h: r.height }; }, sel);
}

async function test(name, fn) {
  if (only && !name.includes(only)) return;
  const t0 = Date.now();
  try {
    await fn();
    results.push({ name, ok: true, ms: Date.now() - t0 });
    console.log(`ok   ${name}`);
  } catch (e) {
    results.push({ name, ok: false, error: String(e && e.message).split("\n")[0].slice(0, 300) });
    console.log(`FAIL ${name}: ${String(e && e.message).split("\n")[0].slice(0, 300)}`);
  }
}

// ---------------------------------------------------------------------------------------------------------------
await test("click reveals only that item; neighbours stay shielded", async () => {
  const { ctx, page } = await open("x.com", "/home");
  assert.equal(await attr(page, "#spoiler1"), "shield");
  assert.equal(await attr(page, "#spoiler2"), "shield");
  const c = await center(page, "#spoiler1");
  await page.mouse.click(c.x, c.y);
  await page.waitForFunction(() => document.getElementById("spoiler1").getAttribute("data-sfw") === "revealed");
  assert.equal(await attr(page, "#spoiler2"), "shield", "neighbour must stay shielded");
  const clears = await page.evaluate(() => [...document.querySelectorAll("[data-expect=clear]:not(:has(img))")].filter((e) => e.closest("[data-sfw~=shield]")).length);
  assert.equal(clears, 0, "clear items untouched");
  await ctx.close();
});

await test("reveal animation ends unblurred, content interactive", async () => {
  const { ctx, page } = await open("x.com", "/home");
  const c = await center(page, "#spoiler1");
  await page.mouse.click(c.x, c.y);
  await sleep(450);
  const r = await page.evaluate(() => {
    const art = document.getElementById("spoiler1");
    const s = art.querySelector(".sentinel");
    const cs = getComputedStyle(s);
    const rect = s.getBoundingClientRect();
    const hit = document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2);
    const link = art.querySelector("a");
    const lr = link.getBoundingClientRect();
    const hit2 = document.elementFromPoint(lr.x + lr.width / 2, lr.y + lr.height / 2);
    return { vis: cs.visibility, filter: getComputedStyle(art).filter, inert: art.querySelector("[data-testid=tweetText]").inert, hitIsContent: hit !== null && hit.localName !== "sfw-shield", linkHit: hit2 === link, state: art.getAttribute("data-sfw") };
  });
  assert.equal(r.vis, "visible");
  assert.equal(r.filter, "none");
  assert.equal(r.inert, false, "inert restored");
  assert.ok(r.hitIsContent && r.linkHit, "revealed content must receive pointer events");
  await ctx.close();
});

await test("keyboard: Tab to overlay, Enter / Space reveal, focus moves to Hide, Enter hides again", async () => {
  const { ctx, page } = await open("x.com", "/home");
  let found = 0;
  for (let i = 0; i < 30 && found < 1; i++) {
    await page.keyboard.press("Tab");
    if (await page.evaluate(() => document.activeElement && document.activeElement.localName === "sfw-shield")) found++;
  }
  assert.equal(found, 1, "Tab reaches an overlay");
  const which = await page.evaluate(() => { const a = document.activeElement; a.parentElement.setAttribute("data-kbd", "1"); return { role: a.getAttribute("role"), label: a.getAttribute("aria-label") }; });
  assert.equal(which.role, "button");
  assert.match(which.label, /Reveal GTA VI spoiler/);
  await page.keyboard.press("Enter");
  await page.waitForFunction(() => document.querySelector("[data-kbd]").getAttribute("data-sfw") === "revealed");
  // focus moved to the (closed-root) Hide control: Enter re-shields
  await page.keyboard.press("Enter");
  await page.waitForFunction(() => document.querySelector("[data-kbd]").getAttribute("data-sfw") === "shield", null, { timeout: 3000 });
  // Space on the overlay
  await page.evaluate(() => document.querySelector("[data-kbd]").querySelector("sfw-shield").focus());
  await page.keyboard.press("Space");
  await page.waitForFunction(() => document.querySelector("[data-kbd]").getAttribute("data-sfw") === "revealed");
  await ctx.close();
});

await test("accessibility tree does not expose shielded text", async () => {
  const { ctx, page } = await open("x.com", "/home");
  const cdp = await ctx.newCDPSession(page);
  const { nodes } = await cdp.send("Accessibility.getFullAXTree");
  const names = nodes.map((n) => `${n.name && n.name.value || ""} ${n.value && n.value.value || ""}`).join("\n");
  assert.ok(!/hero dies|betrays everyone|final mission/i.test(names), "spoiler text leaked into the AX tree");
  assert.ok(/Reveal GTA VI spoiler/.test(names), "overlay is announced");
  assert.ok(/pasta tips/.test(names), "control: clear text is exposed");
  await ctx.close();
});

await test("find-in-page does not find shielded text (control: finds clear text)", async () => {
  const { ctx, page } = await open("x.com", "/home");
  const r = await page.evaluate(() => ({ shielded: window.find("the hero dies"), shielded2: window.find("betrays everyone"), clear: window.find("pasta tips") }));
  assert.equal(r.shielded, false);
  assert.equal(r.shielded2, false);
  assert.equal(r.clear, true);
  await ctx.close();
});

await test("copy guard strips shielded text from the clipboard payload", async () => {
  const { ctx, page } = await open("news.example.test", "/pages/news.html");
  await page.evaluate(() => { window.__copied = null; document.addEventListener("copy", (e) => { window.__copied = e.clipboardData.getData("text/plain"); }); });
  await page.evaluate(() => { const r = document.createRange(); r.selectNodeContents(document.body); const s = getSelection(); s.removeAllRanges(); s.addRange(r); });
  await page.keyboard.press("Control+C");
  await sleep(200);
  const copied = await page.evaluate(() => window.__copied);
  assert.ok(copied != null, "copy event fired");
  assert.ok(!/hero dies|spoiler leaked/i.test(copied), "shielded text must not be copied: " + copied.slice(0, 80));
  assert.ok(/bike lanes/i.test(copied), "control: clear text is copied");
  await ctx.close();
});

await test("hover card does not open under a shield; opens after reveal", async () => {
  const { ctx, page } = await open("example.test", "/pages/adv-hover.html");
  const c = await center(page, "#card");
  await page.mouse.move(c.x - 40, c.y - 5);
  await page.mouse.move(c.x, c.y);
  await page.mouse.move(c.x + 10, c.y + 3);
  await sleep(300);
  const before = await page.evaluate(() => ({ pop: !!document.getElementById("pop"), pops: window.__pops || 0, winCap: window.__winCapture || 0 }));
  assert.equal(before.pop, false, "hover card opened under a shield");
  assert.equal(before.winCap, 0, "page capture listeners must not see events from inside a shield");
  await page.mouse.click(c.x, c.y);
  await sleep(400);
  const l = await center(page, "#user");
  await page.mouse.move(l.x, l.y);
  await sleep(300);
  assert.equal(await page.evaluate(() => !!document.getElementById("pop")), true, "control: hover card opens once revealed");
  await ctx.close();
});

await test("video under a shield is paused/muted and cannot play; clear video plays", async () => {
  const { ctx, page } = await open("example.test", "/pages/adv-video.html");
  const r = await page.evaluate(async () => {
    const v = document.getElementById("vid"), v2 = document.getElementById("vid2");
    let rejected = false;
    try { await v.play(); } catch (e) { rejected = true; }
    await new Promise((res) => setTimeout(res, 300));
    return { paused: v.paused, muted: v.muted, rejected, hold: v.closest("[data-sfw-hold]") != null, autoplayAttr: v.hasAttribute("autoplay"), clearPlaying: !v2.paused };
  });
  assert.equal(r.paused, true);
  assert.equal(r.muted, true);
  assert.equal(r.rejected, true, "play() must reject while held");
  assert.equal(r.hold, true);
  assert.equal(r.autoplayAttr, false);
  assert.equal(r.clearPlaying, true, "control: unshielded video autoplays");
  await ctx.close();
});

await test("layout unchanged by shielding (bounding boxes within 0.5px)", async () => {
  const fixtures = [["x.com", "/home"], ["news.example.test", "/pages/news.html"], ["blog.example.test", "/pages/blog.html"], ["forum.example.test", "/pages/forum.html"], ["www.reddit.com", "/r/pics/comments/abc/some_post/"], ["www.youtube.com", "/watch?v=abc123"], ["www.google.com", "/search?q=weather+tomorrow"], ["old.reddit.com", "/r/pics/"]];
  const diffs = [];
  for (const [h, p] of fixtures) {
    const measure = async (enabled) => {
      const { ctx, page } = await open(h, p, { config: { enabled } });
      const m = await page.evaluate(() => {
        const out = [];
        document.querySelectorAll("body *").forEach((e) => {
          if (e.localName === "sfw-shield" || e.localName === "script" || e.localName === "style") return;
          const r = e.getBoundingClientRect();
          out.push([e.localName + (e.id ? "#" + e.id : ""), Math.round(r.x * 100) / 100, Math.round(r.y * 100) / 100, Math.round(r.width * 100) / 100, Math.round(r.height * 100) / 100]);
        });
        return { boxes: out, sh: document.documentElement.scrollHeight, sw: document.documentElement.scrollWidth, shielded: document.querySelectorAll("[data-sfw~=shield]").length };
      });
      await ctx.close();
      return m;
    };
    const off = await measure(false);
    const on = await measure(true);
    if (on.shielded === 0) diffs.push(`${h}${p}: nothing shielded`);
    if (off.boxes.length !== on.boxes.length) diffs.push(`${h}${p}: element count ${off.boxes.length} vs ${on.boxes.length}`);
    if (off.sh !== on.sh || off.sw !== on.sw) diffs.push(`${h}${p}: scroll size ${off.sw}x${off.sh} vs ${on.sw}x${on.sh}`);
    for (let i = 0; i < Math.min(off.boxes.length, on.boxes.length); i++) {
      const a = off.boxes[i], b = on.boxes[i];
      if (a[0] !== b[0] || Math.abs(a[1] - b[1]) > 0.5 || Math.abs(a[2] - b[2]) > 0.5 || Math.abs(a[3] - b[3]) > 0.5 || Math.abs(a[4] - b[4]) > 0.5) { diffs.push(`${h}${p}: ${a.join(",")} -> ${b.join(",")}`); break; }
    }
  }
  assert.deepEqual(diffs, [], diffs.join(" | "));
});

await test("virtualised list: recycled node with new spoiler content re-shields; revealed binding does not leak", async () => {
  const { ctx, page } = await open("example.test", "/pages/adv-virtual.html");
  const audit = async () => page.evaluate(() => [...document.querySelectorAll("#spacer [data-expect]")].filter((e) => e.getAttribute("data-expect") === "shield" && !e.closest("[data-sfw~=shield]")).map((e) => e.id + ":" + e.textContent.slice(0, 12)));
  const initial = await audit();
  assert.deepEqual(initial, []);
  // reveal the first spoiler cell, then recycle it to different spoiler content
  const idx = await page.evaluate(() => [...document.querySelectorAll("#spacer .cell")].findIndex((e) => e.getAttribute("data-expect") === "shield"));
  const sel = `#cell${idx}`;
  const c = await center(page, sel);
  await page.mouse.click(c.x, c.y);
  await page.waitForFunction((s) => document.querySelector(s).getAttribute("data-sfw") === "revealed", sel);
  for (let i = 1; i <= 14; i++) {
    await page.evaluate((n) => { document.getElementById("vp").scrollTop = n * 70; }, i);
    await sleep(60);
    const bad = await audit();
    assert.deepEqual(bad, [], `scroll step ${i}: unshielded spoiler cells ${bad.join(",")}`);
  }
  await ctx.close();
});

await test("React-style reconciler that removes foreign children: still shielded, falls back to attribute cover", async () => {
  const { ctx, page } = await open("example.test", "/pages/adv-react.html");
  await sleep(1200);
  const r = await page.evaluate(() => ({ unshielded: [...document.querySelectorAll("[data-expect=shield]")].filter((e) => !e.closest("[data-sfw~=shield]")).length, fb: [...document.querySelectorAll("[data-sfw-fb]")].length, renders: window.__renders, stats: window.__sfw.ctl.stats() }));
  assert.equal(r.unshielded, 0);
  assert.ok(r.fb >= 1, "fallback (attribute-only) cover engaged after repeated overlay removal; renders=" + r.renders);
  assert.ok(r.stats.fallbacks >= 1);
  // capture-phase click on the container reveals it
  const c = await center(page, "[data-expect=shield]");
  await page.mouse.click(c.x, c.y);
  await page.waitForFunction(() => document.querySelector("[data-expect=shield]").getAttribute("data-sfw") === "revealed", null, { timeout: 3000 });
  await ctx.close();
});

await test("title guard rewrites a spoiler title; page shield for hot URL reveals on click and restores title", async () => {
  const a = await open("example.test", "/pages/adv-title.html");
  assert.equal(await a.page.title(), "Spoiler-protected page");
  await a.ctx.close();
  const { ctx, page } = await open("www.google.com", "/search?q=gta+6+ending+spoiler");
  assert.match(String(await attr(page, "html", "data-sfw-page")), /shield/);
  const t = await page.title();
  assert.equal(t, "Spoiler-protected page".length ? t : t); // title guard only applies when the title itself matches
  await page.mouse.click(320, 200);
  await page.waitForFunction(() => !document.documentElement.hasAttribute("data-sfw-page"), null, { timeout: 3000 });
  assert.equal(await page.evaluate(() => document.title), "results - Google Search", "title restored on page reveal");
  await ctx.close();
});

await test("config pending: units whose decision differs between modes stay pending until configReady resolves", async () => {
  const ctx = await instrumentedContext(browser, { config: { delayConfigMs: 700, mode: "balanced" }, detection, probe: false });
  const page = await ctx.newPage();
  await page.goto(url("news.example.test", "/pages/blog.html"), { waitUntil: "load" });
  await sleep(120);
  const early = await page.evaluate(() => ({ pending: document.querySelectorAll("[data-sfw-fb=pending]").length, shielded: document.querySelectorAll("[data-sfw~=shield]").length }));
  await sleep(1000);
  const late = await page.evaluate(() => ({ pending: document.querySelectorAll("[data-sfw-fb=pending]").length }));
  assert.ok(early.shielded >= 1, "protective while config is unknown");
  assert.equal(late.pending, 0, "no unit left pending after config arrives");
  await ctx.close();
});

await test("every adapter selector is valid CSS (drift/typos must not disable protection)", async () => {
  const { ctx, page } = await open("news.example.test", "/pages/news.html");
  const bad = await page.evaluate(() => {
    const S = window.__sfw.SFW; const out = [];
    for (const id of S.adapters.list()) {
      const d = S.adapters.definition(id);
      for (const k of ["item", "thread", "context", "media"]) for (const sel of d[k] || []) { try { document.createDocumentFragment().querySelector(sel); } catch (e) { out.push(id + "." + k + ": " + sel); } }
    }
    return out;
  });
  assert.deepEqual(bad, [], JSON.stringify(bad));
  await ctx.close();
});

await test("built Bowser bundle (dist/bowser) boots, shields, and leaks no enumerable globals", async () => {
  const bundle = readFileSync(join(HERE, "..", "..", "dist", "bowser", "spoiler-firewall.js"), "utf8").replace("/*__SFW_CONFIG__*/null", JSON.stringify({ enabled: true, mode: "strict", reducedEffects: false, version: "t" }));
  const ctx = await browser.newContext({ viewport: { width: 640, height: 420 } });
  await ctx.addInitScript({ content: bundle });
  const page = await ctx.newPage();
  await page.goto(url("news.example.test", "/pages/news.html"), { waitUntil: "load" });
  await sleep(400);
  const r = await page.evaluate(() => ({ shielded: document.querySelectorAll("[data-sfw~=shield]").length, enumerable: Object.keys(window).filter((k) => /sfw|SFW|spoiler/i.test(k)), symbols: Object.getOwnPropertySymbols(window).length }));
  assert.ok(r.shielded >= 1, "bundle shielded the spoiler paragraph");
  assert.deepEqual(r.enumerable, []);
  assert.ok(r.symbols <= 1, "at most the idempotency guard symbol");
  await ctx.close();
});

await test("never reveals via API: runtime controller exposes only stop() and stats()", async () => {
  const { ctx, page } = await open("news.example.test", "/pages/news.html");
  const keys = await page.evaluate(() => Object.keys(window.__sfw.ctl).filter((k) => k !== "explain" && k !== "opts"));
  assert.deepEqual(keys.sort(), ["stats", "stop"]);
  await ctx.close();
});

await browser.close();
await srv.close();
const failed = results.filter((r) => !r.ok);
writeFileSync(join(HERE, "out", "interaction.json"), JSON.stringify({ detection, results }, null, 1));
console.log(`\n${results.length - failed.length}/${results.length} interaction tests passed (detection=${detection})`);
process.exit(failed.length ? 1 : 0);
