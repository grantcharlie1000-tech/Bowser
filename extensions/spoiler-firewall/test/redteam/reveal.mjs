// Reveal-semantics red team (headless chrome-headless-shell, real CDP input, test-host bundle).
// Each test is a self-contained page; an assertion failure is a finding (critical = reveal leak).
import { createServer } from "node:http";
import { launch, instrumentedContext } from "../e2e/lib.mjs";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const T = "GTA 6 ending spoiler: the hero dies in the final mission";
const T2 = "GTA VI leaked ending: everyone dies at the finale";
const head = `<!doctype html><meta charset="utf-8"><title>r</title><style>body{font:16px Arial;margin:8px}article,section,blockquote,li{display:block;border:1px solid #ccc;margin:6px;padding:10px;background:#fafafa}.sentinel{color:#ff00ff;font:700 22px Arial}</style>`;
const PAGES = {
  neighbours: `${head}<article id="a1"><span class="sentinel">${T}</span></article><article id="a2"><span class="sentinel">${T2}</span></article><article id="a3"><span class="sentinel">GTA 6 spoiler: she betrays him at the ending</span></article><article id="c1">clean neighbour</article>`,
  nested: `${head}<article id="outer"><span class="sentinel">${T}</span><blockquote id="inner"><span class="sentinel">${T2}</span></blockquote></article>`,
  recycle: `${head}<article id="slot"><span id="txt" class="sentinel">${T}</span></article>`,
  appendInto: `${head}<article id="post"><span class="sentinel">${T}</span><div id="replies"></div></article>`,
  listShared: `${head}<ul id="ul"><li id="l1"><span class="sentinel">${T}</span></li><li id="l2"><span class="sentinel">${T2}</span></li></ul>`,
  dupText: `${head}<article id="d1"><span class="sentinel">${T}</span></article><article id="d2"><span class="sentinel">${T}</span></article>`,
  keyboard: `${head}<a href="#x" id="pre">before</a><article id="k1"><span class="sentinel">${T}</span><a href="#in" id="inlink">link inside spoiler</a><button id="inbtn">btn inside</button></article><article id="k2"><span class="sentinel">${T2}</span></article><a href="#y" id="post">after</a>`,
  forgery: `${head}<article id="f1"><span class="sentinel">${T}</span></article><label for="nothing" id="lab">label</label><button id="b1" accesskey="r">r</button><input id="auto" autofocus>`,
  hotpage: `${head}<article id="h1">${T}</article><article id="h2">weather</article>`,
  clickjack: `${head}<article id="j1" style="position:relative"><span class="sentinel">${T}</span></article><article id="j2" style="position:relative">clean</article><div id="cover" style="position:absolute;left:0;top:0;width:100%;height:60px;opacity:0;z-index:2147483647"></div>`,
};

export async function runReveal({ log = console.log } = {}) {
  const srv = createServer((req, res) => {
    const u = new URL(req.url, "http://x");
    const key = u.searchParams.get("p") || u.pathname.slice(1);
    res.setHeader("Content-Type", "text/html; charset=utf-8");
    res.end(PAGES[key] || "nope");
  });
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  const port = srv.address().port;
  const browser = await launch({ beginFrame: false });
  const results = [];
  async function open(key, host = "example.test", path = "") {
    const ctx = await instrumentedContext(browser, { config: {}, probe: false });
    const page = await ctx.newPage();
    const errs = [];
    page.on("pageerror", (e) => errs.push(e.message));
    await page.goto(`http://${host}:${port}/${path || key}${path ? "" : ""}?p=${key}`, { waitUntil: "load" });
    await sleep(500);
    return { ctx, page, errs };
  }
  const st = (page, sel) => page.evaluate((s) => { const e = document.querySelector(s); return e ? e.getAttribute("data-sfw") : "<missing>"; }, sel);
  const center = (page, sel) => page.evaluate((s) => { const r = document.querySelector(s).getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + Math.min(r.height / 2, 30) }; }, sel);
  const clickOverlay = async (page, sel) => { const c = await center(page, sel); await page.mouse.click(c.x, c.y); await sleep(450); };

  async function test(name, severity, fn) {
    let ok = true, detail = "";
    try { const r = await fn(); if (r) { ok = false; detail = r; } } catch (e) { ok = false; detail = "threw: " + String(e && e.message).split("\n")[0]; }
    results.push({ name, severity, ok, detail });
    log(`${ok ? "ok  " : "FAIL"} [${severity}] ${name}${ok ? "" : " :: " + detail}`);
  }

  await test("click one: neighbours stay shielded, clear untouched", "critical", async () => {
    const { ctx, page } = await open("neighbours");
    await clickOverlay(page, "#a1");
    const s = [await st(page, "#a1"), await st(page, "#a2"), await st(page, "#a3"), await st(page, "#c1")];
    await ctx.close();
    if (s[0] !== "revealed") return "clicked item not revealed: " + s[0];
    if (s[1] !== "shield" || s[2] !== "shield") return "neighbour revealed: " + JSON.stringify(s);
    if (s[3] === "shield") return "clear item shielded";
  });

  await test("nested units: clicking INNER overlay must not reveal OUTER text", "critical", async () => {
    const { ctx, page } = await open("nested");
    const info = await page.evaluate(() => ({ outer: document.getElementById("outer").getAttribute("data-sfw"), inner: document.getElementById("inner").getAttribute("data-sfw") }));
    if (info.inner !== "shield" && info.outer !== "shield") { await ctx.close(); return "setup: nothing shielded " + JSON.stringify(info); }
    const target = info.inner === "shield" ? "#inner" : "#outer";
    await clickOverlay(page, target);
    const after = await page.evaluate(() => ({ outer: document.getElementById("outer").getAttribute("data-sfw"), inner: document.getElementById("inner").getAttribute("data-sfw") }));
    await ctx.close();
    if (info.inner === "shield" && info.outer === "shield" && after.outer !== "shield") return "clicking inner revealed outer: " + JSON.stringify(after);
    if (after.inner === "revealed" && after.outer !== "shield" && info.outer === "shield") return "outer leaked " + JSON.stringify(after);
    if (info.inner !== "shield" && info.outer === "shield") return "inner spoiler is not independently shielded (outer shield covers both): outer reveal reveals inner spoiler (acceptable: one unit)"; // informational
  });

  await test("recycled container (virtual list) with DIFFERENT spoiler re-shields after reveal", "critical", async () => {
    const { ctx, page } = await open("recycle");
    await clickOverlay(page, "#slot");
    if ((await st(page, "#slot")) !== "revealed") { await ctx.close(); return "setup: not revealed"; }
    await page.evaluate((t) => { document.getElementById("txt").firstChild.data = t; }, T2);
    await sleep(300);
    const s = await st(page, "#slot");
    await ctx.close();
    if (s === "revealed") return "different spoiler text shown under old reveal (leak)";
  });

  await test("recycled container: reveal -> clean -> DIFFERENT spoiler re-shields", "critical", async () => {
    const { ctx, page } = await open("recycle");
    await clickOverlay(page, "#slot");
    await page.evaluate(() => { document.getElementById("txt").firstChild.data = "just weather"; });
    await sleep(250);
    await page.evaluate((t) => { document.getElementById("txt").firstChild.data = t; }, "GTA 6 final mission twist: Jason is the rat, ending spoiler");
    await sleep(300);
    const s = await st(page, "#slot");
    await ctx.close();
    if (s === "revealed") return "second spoiler visible after clean gap under stale reveal";
  });

  await test("recycled container: reveal -> clean -> SAME spoiler (stale reveal persistence)", "low", async () => {
    const { ctx, page } = await open("recycle");
    await clickOverlay(page, "#slot");
    await page.evaluate(() => { document.getElementById("txt").firstChild.data = "just weather"; });
    await sleep(250);
    await page.evaluate((t) => { document.getElementById("txt").firstChild.data = t; }, T);
    await sleep(300);
    const s = await st(page, "#slot");
    await ctx.close();
    if (s === "revealed") return "identical text recycled into the same element stays revealed (a recycled slot showing a different post with identical text is revealed without a click)";
  });

  await test("append a spoiler reply inside a REVEALED post: reply must be shielded", "critical", async () => {
    const { ctx, page } = await open("appendInto");
    await clickOverlay(page, "#post");
    await page.evaluate((t) => { const a = document.createElement("article"); a.id = "reply"; a.innerHTML = `<span class="sentinel">${t}</span>`; document.getElementById("replies").appendChild(a); }, T2);
    await sleep(400);
    const r = await page.evaluate(() => { const a = document.getElementById("reply"); const sh = a.closest("[data-sfw=shield]") || a.querySelector("[data-sfw=shield]"); return { shielded: !!sh, post: document.getElementById("post").getAttribute("data-sfw"), reply: a.getAttribute("data-sfw") }; });
    await ctx.close();
    if (!r.shielded) return "new spoiler appended into revealed container is visible: " + JSON.stringify(r);
  });

  await test("append a spoiler as bare TEXT into a revealed post (no child element)", "critical", async () => {
    const { ctx, page } = await open("appendInto");
    await clickOverlay(page, "#post");
    await page.evaluate((t) => { document.getElementById("post").appendChild(document.createTextNode(" " + t)); }, "GTA VI final mission: she betrays him, ending twist");
    await sleep(400);
    const s = await st(page, "#post");
    await ctx.close();
    if (s === "revealed") return "extra spoiler text node appended to revealed unit leaves it revealed";
  });

  await test("list items: reveal l1, l2 stays shielded", "critical", async () => {
    const { ctx, page } = await open("listShared");
    await clickOverlay(page, "#l1");
    const s = [await st(page, "#l1"), await st(page, "#l2")];
    await ctx.close();
    if (s[0] !== "revealed" || s[1] !== "shield") return JSON.stringify(s);
  });

  await test("two items with IDENTICAL text: revealing one must not reveal the other (fingerprint is per element)", "critical", async () => {
    const { ctx, page } = await open("dupText");
    await clickOverlay(page, "#d1");
    const s = [await st(page, "#d1"), await st(page, "#d2")];
    await ctx.close();
    if (s[1] !== "shield") return "twin revealed: " + JSON.stringify(s);
  });

  await test("page-script click()/dispatchEvent on <sfw-shield> reveals (out of threat model: needs script)", "low", async () => {
    const { ctx, page } = await open("forgery");
    await page.evaluate(() => { document.querySelector("sfw-shield").click(); });
    await sleep(300);
    const a = await st(page, "#f1");
    await ctx.close();
    if (a === "revealed") return "untrusted synthetic click() reveals; no isTrusted check (script-in-page only, a poster without script cannot do this)";
  });

  await test("poster-reachable triggers: autofocus/accesskey/label do not reveal; Alt+key does not reveal", "high", async () => {
    const { ctx, page } = await open("forgery");
    await page.keyboard.press("Alt+r");
    await page.keyboard.press("Enter");
    await page.keyboard.press("Space");
    await sleep(300);
    const a = await st(page, "#f1");
    const ae = await page.evaluate(() => document.activeElement && document.activeElement.tagName);
    await ctx.close();
    if (a === "revealed") return "reveal fired from poster-reachable focus/accesskey (activeElement " + ae + ")";
  });

  await test("transparent poster overlay above shield (clickjack): real click on it must not reveal the item below", "high", async () => {
    const { ctx, page } = await open("clickjack");
    const c = await center(page, "#j1");
    await page.mouse.click(c.x, 20);
    await sleep(300);
    const a = await st(page, "#j1");
    await ctx.close();
    if (a === "revealed") return "click landed through a poster-owned transparent layer and revealed (poster can steer the user's click onto shield)";
  });

  await test("keyboard: Tab order has no trap, inert spoiler children unfocusable, Enter reveals only focused", "high", async () => {
    const { ctx, page } = await open("keyboard");
    await page.focus("#pre");
    const trail = [];
    for (let i = 0; i < 10; i++) {
      await page.keyboard.press("Tab");
      trail.push(await page.evaluate(() => { const a = document.activeElement; if (!a) return "none"; return a.id || a.localName + (a.getAttribute && a.getAttribute("data-s") ? ":" + a.getAttribute("data-s") : ""); }));
    }
    const insideFocused = trail.filter((t) => t === "inlink" || t === "inbtn");
    await page.focus("#pre");
    await page.keyboard.press("Tab");
    await page.keyboard.press("Enter");
    await sleep(450);
    const s = [await st(page, "#k1"), await st(page, "#k2")];
    await ctx.close();
    if (insideFocused.length) return "focus reached controls inside a shielded spoiler: " + trail.join(",");
    if (!trail.includes("post")) return "focus never reached the element after the shields (trap?): " + trail.join(",");
    if (s[1] === "revealed") return "Enter revealed a different item: " + JSON.stringify(s);
    if (s[0] !== "revealed") return "Tab+Enter did not reveal the first shield: " + JSON.stringify(s) + " trail=" + trail.join(",");
  });

  await test("hot-URL page shield: revealing the page does not reveal individual hot items", "critical", async () => {
    const { ctx, page } = await open("hotpage", "www.google.com", "search");
    // url has ?p=hotpage ; add the hot query via history so adapter sees it
    await ctx.close();
    const ctx2 = await instrumentedContext(browser, { config: {}, probe: false });
    const p2 = await ctx2.newPage();
    await p2.goto(`http://www.google.com:${port}/search?q=gta+6+ending+spoiler&p=hotpage`, { waitUntil: "load" });
    await sleep(600);
    const before = await p2.evaluate(() => ({ page: document.documentElement.getAttribute("data-sfw-page"), h1: document.getElementById("h1").getAttribute("data-sfw") }));
    if (before.page !== "shield") { await ctx2.close(); return "setup: page not shielded " + JSON.stringify(before); }
    const vp = p2.viewportSize();
    await p2.mouse.click(vp.width / 2, vp.height / 2);
    await sleep(500);
    const after = await p2.evaluate(() => ({ page: document.documentElement.getAttribute("data-sfw-page"), h1: document.getElementById("h1").getAttribute("data-sfw") }));
    await ctx2.close();
    if (after.page === "shield") return "page click did not reveal page (setup)";
    if (after.h1 !== "shield") return "revealing the page revealed the item inside: " + JSON.stringify(after);
  });

  await browser.close();
  await new Promise((r) => srv.close(r));
  return results;
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].replace(/\\/g, "/").split("/").pop())) await runReveal();
