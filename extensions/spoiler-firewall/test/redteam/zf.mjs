// Zero-flash red team: frame-exact pixel check (chrome-headless-shell + HeadlessExperimental.beginFrame, sentinel #ff00ff)
// plus the DOM-invariant probe from test/e2e/lib.mjs, over the fixtures in fixtures.mjs. Reuses the e2e harness (read-only).
// Headless only, muted, below-normal priority (set by ../e2e/env.mjs).
import { writeFileSync, mkdirSync, readFileSync, existsSync } from "node:fs";
import http from "node:http";
import { dirname, join, extname } from "node:path";
import { fileURLToPath } from "node:url";
import { launch, instrumentedContext, FrameRecorder } from "../e2e/lib.mjs";
import { FIXTURES, BASE_CSS } from "./fixtures.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = join(HERE, "out");
const FXDIR = join(HERE, "fixtures");
mkdirSync(OUT, { recursive: true });
mkdirSync(FXDIR, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const withTimeout = (p, ms) => Promise.race([p, sleep(ms)]);
const STATIC = join(HERE, "..", "fixtures", "static");
const TYPES = { ".html": "text/html; charset=utf-8", ".svg": "image/svg+xml", ".css": "text/css", ".js": "text/javascript" };

export function pageHtml(fx) {
  return `<!doctype html><html><head><meta charset="utf-8"><title>redteam ${fx.id}</title><style>${BASE_CSS}</style>${fx.head || ""}</head><body>${fx.body || ""}${fx.script ? `<script>${fx.script}</script>` : ""}</body></html>`;
}

export async function startServer() {
  const handler = async (req, res) => {
    try {
      const u = new URL(req.url, "http://x");
      let m;
      if ((m = /^\/fx\/([\w-]+)\.html$/.exec(u.pathname))) {
        const fx = FIXTURES.find((f) => f.id === m[1]);
        if (!fx) { res.writeHead(404); res.end("nofx"); return; }
        const html = pageHtml(fx);
        res.setHeader("Content-Type", TYPES[".html"]);
        res.setHeader("Cache-Control", "no-store");
        if (!fx.stream) { res.end(html); return; }
        res.writeHead(200);
        for (let i = 0; i < html.length; i += fx.stream.size) { res.write(html.slice(i, i + fx.stream.size)); await sleep(fx.stream.delay); }
        res.end();
        return;
      }
      if ((m = /^\/static\/([\w.-]+)$/.exec(u.pathname)) && existsSync(join(STATIC, m[1]))) {
        res.setHeader("Content-Type", TYPES[extname(m[1])] || "application/octet-stream");
        res.end(readFileSync(join(STATIC, m[1])));
        return;
      }
      res.writeHead(404); res.end("not found");
    } catch (e) { try { res.writeHead(500); res.end(String(e && e.message)); } catch (x) { /* ignore */ } }
  };
  const s = http.createServer(handler);
  await new Promise((r) => s.listen(0, "127.0.0.1", r));
  return { port: s.address().port, close: () => new Promise((r) => s.close(r)) };
}

async function probe(page) {
  const out = { unsafe: 0, leak: 0, taskUnsafe: 0, taskLeak: 0, frames: 0, examples: [], shields: 0, errors: 0, lastError: null, hasEngine: false };
  for (const f of page.frames()) {
    try {
      const r = await f.evaluate(() => ({ p: window.__probe, s: window.__sfw && window.__sfw.ctl && window.__sfw.ctl.stats(), e: !!window.__sfw }));
      if (r.p) { out.unsafe += r.p.unsafe; out.leak += r.p.leak; out.taskUnsafe += r.p.taskUnsafe || 0; out.taskLeak += r.p.taskLeak || 0; out.frames += r.p.frames; out.examples.push(...r.p.examples); }
      if (r.s) { out.shields += r.s.shields || 0; out.errors += r.s.errors || 0; out.lastError = out.lastError || r.s.lastError || null; }
      if (r.e) out.hasEngine = true;
    } catch (e) { /* detached */ }
  }
  return out;
}

async function runOne(browser, port, fx, enabled) {
  return Promise.race([runOne0(browser, port, fx, enabled), sleep(150000).then(() => { throw new Error("fixture hung >150s (harness or browser)"); })]);
}
async function runOne0(browser, port, fx, enabled) {
  const ctx = await instrumentedContext(browser, { config: { enabled } });
  const page = await ctx.newPage();
  const pageErrors = [];
  page.on("pageerror", (e) => pageErrors.push(e.message));
  const popups = [];
  const popRecs = [];
  ctx.on("page", async (p) => {
    if (p === page) return;
    popups.push(p);
  });
  await page.goto("about:blank");
  const rec = new FrameRecorder(ctx, page);
  await rec.start();
  let navErr = null;
  try { await page.goto(`http://example.test:${port}/fx/${fx.id}.html`, { waitUntil: "load", timeout: 30000 }); } catch (e) { navErr = e.message; }
  for (const a of fx.actions || []) {
    try {
      if (a.click) await page.click(a.click, { timeout: 4000 });
      else if (a.wait) await sleep(a.wait);
      else if (a.eval) await page.evaluate(a.eval);
    } catch (e) { pageErrors.push("action: " + String(e.message).split("\n")[0]); }
  }
  await sleep(fx.settle || 700);
  await rec.waitFrames(25, 3500);
  const pr = await withTimeout(probe(page), 4000) || { unsafe: 0, leak: 0, taskUnsafe: 0, taskLeak: 0, shields: 0, errors: 0, hasEngine: false, examples: [] };
  const popPr = [];
  const popDom = [];
  for (const p of popups) {
    try { popDom.push(await withTimeout(p.evaluate(() => ({ sentinels: [...document.querySelectorAll(".sentinel")].filter((e) => e.getBoundingClientRect().width > 0).length, unshielded: [...document.querySelectorAll(".sentinel")].filter((e) => !e.closest("[data-sfw~=shield]") && getComputedStyle(e).visibility !== "hidden" && !(document.documentElement.getAttribute("data-sfw-page") === "shield")).length, engine: !!window.__sfw })), 3000)); } catch (e) { popDom.push({ error: String(e.message).split(String.fromCharCode(10))[0] }); }
  }
  for (const p of popups) popPr.push(await withTimeout(probe(p), 3000) || { unsafe: 0, leak: 0, taskUnsafe: 0, taskLeak: 0, shields: 0, errors: 0, hasEngine: false, examples: [] });
  await withTimeout(rec.stop(), 4000);
  for (const r of popRecs) await withTimeout(r.stop(), 2000);
  const allRecs = [rec, ...popRecs];
  const unsafeFrames = allRecs.reduce((a, r) => a + r.unsafe, 0);
  const maxPx = Math.max(0, ...allRecs.map((r) => r.maxPixels));
  const firstUnsafe = allRecs.map((r) => r.firstUnsafe).find(Boolean);
  if (firstUnsafe && enabled) writeFileSync(join(OUT, `unsafe-${fx.id}.png`), Buffer.from(firstUnsafe.png, "base64"));
  const popUnshielded = popDom.reduce((a, p) => a + (p && p.unshielded || 0), 0);
  const domUnsafe = popUnshielded + pr.unsafe + pr.taskUnsafe + popPr.reduce((a, p) => a + p.unsafe + p.taskUnsafe, 0);
  const domLeak = pr.leak + pr.taskLeak + popPr.reduce((a, p) => a + p.leak + p.taskLeak, 0);
  const res = {
    id: fx.id, enabled, scope: fx.scope, navErr, pageErrors: pageErrors.slice(0, 3), frames: allRecs.reduce((a, r) => a + r.frames, 0), shots: allRecs.reduce((a, r) => a + r.shots, 0),
    unsafeFramesPixel: unsafeFrames, maxSentinelPixels: maxPx, domUnsafeFrames: domUnsafe, domLeakFrames: domLeak, shields: pr.shields + popPr.reduce((a, p) => a + p.shields, 0),
    engineErrors: pr.errors, lastError: pr.lastError, popups: popups.length, popupEngine: popDom.map((p) => p && p.engine), popupDom: popDom, examples: pr.examples.slice(0, 2),
  };
  await withTimeout(ctx.close(), 5000);
  return res;
}

export async function runZeroFlash({ only = null, log = console.log } = {}) {
  for (const fx of FIXTURES) writeFileSync(join(FXDIR, fx.id + ".html"), pageHtml(fx));
  const srv = await startServer();
  let browser = await launch({ beginFrame: true });
  const rows = [];
  const list = FIXTURES.filter((f) => !only || only.includes(f.id));
  for (const fx of list) {
    let br = browser, own = false;
    if (fx.popup || fx.freshBrowser) { br = await launch({ beginFrame: !fx.popup }); own = true; }
    const control = fx.expectShield ? null : await runOne(br, srv.port, fx, false).catch((e) => ({ maxSentinelPixels: 0, domUnsafeFrames: 0, crashed: String(e.message).split(String.fromCharCode(10))[0] }));
    if (own) { try { await br.close(); } catch (e) { /* ignore */ } br = await launch({ beginFrame: !fx.popup }); }
    const r = await runOne(br, srv.port, fx, true).catch((e) => ({ id: fx.id, scope: fx.scope, unsafeFramesPixel: 0, maxSentinelPixels: 0, domUnsafeFrames: 0, domLeakFrames: 0, shields: 0, frames: 0, crashed: String(e.message).split(String.fromCharCode(10))[0] }));
    if (own) { try { await br.close(); } catch (e) { /* ignore */ } }
    else if (r.crashed) { try { await Promise.race([browser.close(), sleep(4000)]); } catch (e) { /* ignore */ } browser = await launch({ beginFrame: true }); }
    const fixtureValid = fx.expectShield ? true : control.maxSentinelPixels >= 3 || control.domUnsafeFrames > 0 || (fx.popup && control.popupDom && control.popupDom.some((d) => d && d.unshielded > 0));
    const leaked = r.unsafeFramesPixel > 0 || r.domUnsafeFrames > 0; // domLeakFrames (sentinel visible INSIDE a shield but pixels clean) is defense-in-depth, reported separately
    const notShielded = fx.expectShield && r.shields === 0;
    const row = { ...r, controlMaxPixels: control ? control.maxSentinelPixels : null, fixtureValid, why: fx.why, leaked: !!(leaked || notShielded), notShielded: !!notShielded };
    rows.push(row);
    log(`${!fixtureValid ? "INVALID" : row.leaked ? (fx.scope === "must" ? "FAIL   " : "LIMIT  ") : "ok     "} ${fx.id.padEnd(34)} px=${r.unsafeFramesPixel}/${r.frames} (max ${r.maxSentinelPixels}) domUnsafe=${r.domUnsafeFrames} domLeak=${r.domLeakFrames} shields=${r.shields} ctl=${row.controlMaxPixels}${r.engineErrors ? " engErr=" + r.engineErrors : ""}${r.popups ? ` popups=${r.popups} popupEngine=${r.popupEngine}` : ""}${r.navErr ? " NAVERR" : ""}`);
  }
  await browser.close();
  await srv.close();
  return rows;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const only = (process.argv.find((a) => a.startsWith("--only=")) || "").split("=")[1];
  const rows = await runZeroFlash({ only: only ? only.split(",") : null });
  writeFileSync(join(OUT, "zf.json"), JSON.stringify(rows, null, 1));
}
