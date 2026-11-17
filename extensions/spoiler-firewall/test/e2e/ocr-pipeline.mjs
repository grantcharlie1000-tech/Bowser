// Runtime media-OCR pipeline (engine/runtime.js "media OCR gate") against a FAKE host.ocr with scripted delays,
// statuses and text. The real OCR is tested in test/chrome/ocr-extension.mjs. Headless chrome-headless-shell only, muted.
//
// The page is served under the hostname x.com (--host-resolver-rules in env.mjs, plain http), so the X adapter is
// selected by hostname with no adapter override: highRisk -> media is CSS-quarantined (visibility:hidden) until released.
// Spoiler images are solid #ff00ff (file names contain "magenta": the DOM probe and the beginFrame pixel recorder both
// treat them as the sentinel); clean images are teal.
//
// usage: node test/e2e/ocr-pipeline.mjs [--only=substring]
import assert from "node:assert/strict";
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { startServer } from "../fixtures/server.mjs";
import { launch, instrumentedContext, FrameRecorder } from "./lib.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
mkdirSync(join(HERE, "out"), { recursive: true });
const only = (process.argv.find((x) => x.startsWith("--only=")) || "").slice(7);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const SPOILER_TEXT = "GTA 6 ENDING: the hero dies in the final mission";
const spoilerRules = [{ match: "magenta", status: "ok", text: SPOILER_TEXT }];

const srv = await startServer();
const browser = await launch({ beginFrame: false });
const bfBrowser = await launch({ beginFrame: true });
const url = (path) => `http://x.com:${srv.portA}${path}`;
const results = [];
const extra = {}; // numbers reported in the summary

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

async function open(path, { ocr, mode = "strict", viewport, extraConfig = {}, b = browser } = {}) {
  const ctx = await instrumentedContext(b, { config: { mode, ocr, ...extraConfig }, detection: "real", probe: true, viewport });
  const page = await ctx.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto(url(path), { waitUntil: "load" });
  return { ctx, page, errors };
}

// state of an element: shield attribute, release marker, computed visibility of the media, veil marker on the wrapper
const st = (page, sel) => page.evaluate((s) => {
  const e = document.querySelector(s);
  if (!e) return null;
  const img = e.matches("img") ? e : e.querySelector("img");
  const wrap = img && img.parentElement;
  return {
    shield: e.getAttribute("data-sfw"),
    wrapShield: wrap ? wrap.getAttribute("data-sfw") : null,
    ok: img ? img.getAttribute("data-sfw-m") : null,
    vis: img ? getComputedStyle(img).visibility : null,
    veil: wrap ? wrap.hasAttribute("data-sfw-mc") : false,
    variant: (() => { const S = window.__sfw.SFW.shield; const t = wrap && S.state(wrap).mode !== "none" ? wrap : S.state(e).mode !== "none" ? e : null; return t ? S.state(t).variant : null; })(),
  };
}, sel);
const waitFor = (page, fn, arg, timeout = 8000) => page.waitForFunction(fn, arg, { timeout, polling: 25 });
const ocrStats = (page) => page.evaluate(() => window.__sfw.ctl.stats().ocr);
const probe = (page) => page.evaluate(() => { const p = window.__probe; return { frames: p.frames, unsafe: p.unsafe, leak: p.leak, taskUnsafe: p.taskUnsafe, taskLeak: p.taskLeak, taskScans: p.taskScans }; });
const noLeaks = async (page, what) => {
  const p = await probe(page);
  assert.equal(p.unsafe + p.leak + p.taskUnsafe + p.taskLeak, 0, `${what}: sentinel visible outside/inside shield ${JSON.stringify(p)}`);
};
const log = (page) => page.evaluate(() => window.__sfw.ocr.log.map((l) => ({ url: l.url, w: l.w, h: l.h, nw: l.nw, nh: l.nh, kind: l.kind })));

// ---------------------------------------------------------------------------------------------------------------
await test("basic: spoiler image shields the whole tweet, clean images release, small/text-only skip OCR; no unsafe frame", async () => {
  const ctx = await instrumentedContext(bfBrowser, { config: { mode: "strict", ocr: { manual: true, rules: spoilerRules } }, detection: "real", probe: true });
  const page = await ctx.newPage();
  await page.goto("about:blank");
  const rec = new FrameRecorder(ctx, page);
  await rec.start();
  await page.goto(url("/ocr-pages/feed-basic.html"), { waitUntil: "load" });
  // while OCR is pending (manual host: nothing answers yet): every image is hidden, eligible ones carry the veil marker
  await waitFor(page, () => window.__sfw.ocr.pending.length >= 3);
  await rec.waitFrames(10, 3000);
  const mid = { spoiler: await st(page, "#c-spoiler"), clean: await st(page, "#c-clean") };
  assert.equal(mid.spoiler.vis, "hidden", "spoiler image hidden while OCR is pending");
  assert.equal(mid.clean.vis, "hidden", "clean image hidden while OCR is pending (held, not released early)");
  assert.equal(mid.clean.ok, null);
  assert.equal(mid.spoiler.shield, null, "unit is not shielded before OCR says so (text is clean)");
  assert.equal(mid.clean.veil, true, "pending veil marker on the media-only wrapper");
  // the host answers out of order, with delays between answers (frames keep being recorded)
  for (const [match, text] of [["blank", ""], ["magenta", `${SPOILER_TEXT}`], ["clean", "dinner at home"]]) {
    await page.evaluate(([m, t]) => window.__sfw.ocr.settle(m, { status: "ok", text: t }), [match, text]);
    await sleep(160);
  }
  await waitFor(page, () => window.__sfw.ctl.stats().ocr.ok >= 3);
  await sleep(300);
  await rec.waitFrames(15, 3000);
  const s = {
    spoiler: await st(page, "#c-spoiler"), clean: await st(page, "#c-clean"), blank: await st(page, "#c-blank"), tiny: await st(page, "#c-tiny"), text: await st(page, "#c-text"),
  };
  assert.equal(s.spoiler.shield, "shield", "whole tweet shielded");
  assert.equal(await page.evaluate(() => document.querySelector("#c-spoiler [data-testid=tweetText]").closest("[data-sfw~=shield]") !== null), true, "its caption is under the shield too");
  assert.equal(s.clean.shield, null);
  assert.equal(s.clean.ok, "ok");
  assert.equal(s.clean.vis, "visible");
  assert.equal(s.blank.ok, "ok", "text-free image released");
  assert.equal(s.blank.veil, false, "veil removed on release");
  assert.equal(s.tiny.ok, "ok", "48x48 avatar-sized image released without OCR");
  assert.equal(s.text.shield, null);
  const l = await log(page);
  assert.equal(l.length, 3, "requests: spoiler, clean, blank (tiny + text-only tweet are not sent): " + JSON.stringify(l.map((x) => x.url)));
  assert.ok(l.every((x) => x.w >= 96 && x.h >= 64 && x.nw >= 96), "request carries rendered + natural sizes");
  assert.deepEqual(await page.evaluate(() => window.__sfw.reports["ocr.ok"]), 3);
  await rec.stop();
  const p = await probe(page);
  await ctx.close();
  assert.equal(rec.unsafe, 0, `pixel-unsafe frames: ${rec.unsafe} of ${rec.shots} (max sentinel px ${rec.maxPixels})`);
  assert.equal(p.unsafe + p.leak + p.taskUnsafe + p.taskLeak, 0, "DOM probe: " + JSON.stringify(p));
  extra.basicFrames = rec.shots;
  extra.basicTaskScans = p.taskScans;
});

await test("negative control: if OCR wrongly calls the spoiler image clean, the detectors do see unsafe frames", async () => {
  const ctx = await instrumentedContext(bfBrowser, { config: { mode: "strict", ocr: { delayMs: 200, rules: [{ match: "magenta", status: "ok", text: "lovely picture" }] } }, detection: "real", probe: true });
  const page = await ctx.newPage();
  await page.goto("about:blank");
  const rec = new FrameRecorder(ctx, page);
  await rec.start();
  await page.goto(url("/ocr-pages/feed-basic.html"), { waitUntil: "load" });
  await waitFor(page, () => window.__sfw.ctl.stats().ocr.ok >= 3);
  await rec.waitFrames(12, 3000);
  await rec.stop();
  const p = await probe(page);
  await ctx.close();
  assert.ok(rec.unsafe > 0 && p.unsafe > 0, `control must leak: px=${rec.unsafe} dom=${p.unsafe}`);
  extra.controlUnsafeFrames = rec.unsafe;
});

await test("combined decision: clean caption + clean-looking OCR text that together make a spoiler shield the unit", async () => {
  const { ctx, page } = await open("/ocr-pages/feed-empty.html", { ocr: { delayMs: 80, rules: [{ match: "magenta", status: "ok", text: "Grand Theft Auto" }] } });
  await page.evaluate(() => addCard({ id: "k1", text: "he dies at the end", src: "/ocr-media/magenta-spoiler.svg?id=k1" }));
  await page.evaluate(() => addCard({ id: "k2", text: "nice weather", src: "/ocr-media/clean.svg?id=k2" }));
  await waitFor(page, () => window.__sfw.ctl.stats().ocr.ok >= 2);
  await sleep(150);
  assert.equal((await st(page, "#k1")).shield, "shield", "caption + OCR text together are a spoiler");
  assert.equal((await st(page, "#k2")).ok, "ok");
  assert.equal(await page.evaluate(() => { const S = window.__sfw.SFW; return S.policy.decide(S.classify.text("Grand Theft Auto", { hot: 0, source: "ocr" }), { mode: "strict", hot: 0, kind: "media", hasMedia: true, highRisk: true, ocrDone: true }); }), "clear", "precondition: the OCR text alone is clear");
  await ctx.close();
});

await test("OCR text alone is classified with source ocr: a franchise mention in an image shields even a clean caption", async () => {
  const { ctx, page } = await open("/ocr-pages/feed-empty.html", { ocr: { delayMs: 40, rules: [{ match: "magenta", status: "ok", text: "GTA 6" }] } });
  await page.evaluate(() => addCard({ id: "m1", text: "look at this", src: "/ocr-media/magenta-spoiler.svg?id=m1" }));
  await waitFor(page, () => window.__sfw.ctl.stats().ocr.ok >= 1);
  await sleep(120);
  assert.equal((await st(page, "#m1")).shield, "shield");
  await ctx.close();
});

for (const mode of ["strict", "lockdown", "balanced"]) {
  for (const status of ["error", "unavailable", "skipped", "throw"]) {
    await test(`failure matrix: ${mode} / ${status}`, async () => {
      const { ctx, page } = await open("/ocr-pages/feed-empty.html", { mode, ocr: { delayMs: 40, rules: [{ match: "clean", status }] } });
      await page.evaluate(() => addCard({ id: "f1", text: "just a photo", src: "/ocr-media/clean.svg?id=f1" }));
      await waitFor(page, () => { const s = window.__sfw.ctl.stats().ocr; return s.ok + s.error + s.unavailable + s.skipped >= 1; });
      await sleep(150);
      const s = await st(page, "#f1");
      const counted = await ocrStats(page);
      const norm = status === "throw" ? "error" : status;
      assert.equal(counted[norm], 1, `${norm} counted: ${JSON.stringify(counted)}`);
      const released = mode === "balanced" || (norm === "skipped" && mode !== "lockdown");
      if (released) {
        assert.equal(s.ok, "ok", "released");
        assert.equal(s.variant, null, "no cover");
      } else {
        assert.equal(s.ok, null, "never auto-released");
        assert.equal(s.vis, "hidden");
        assert.equal(s.variant, "mediaerr", "click-to-reveal cover with the could-not-check wording");
        assert.equal(s.wrapShield, "shield", "cover sits on the media-only wrapper, the caption stays visible");
        assert.equal(await page.evaluate(() => document.querySelector("#f1 [data-testid=tweetText]").closest("[data-sfw~=shield]") === null), true, "caption not covered");
        // the cover says what it is and reveals on click
        const label = await page.evaluate(() => { const o = document.querySelector("#f1 .photo > sfw-shield"); return o && o.getAttribute("aria-label"); });
        assert.match(label || "", /could not be checked/i);
        const box = await page.evaluate(() => { const r = document.querySelector("#f1 .photo").getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; });
        await page.mouse.click(box.x, box.y);
        await waitFor(page, () => document.querySelector("#f1 .photo").getAttribute("data-sfw") === "revealed");
        await sleep(280);
        assert.equal((await st(page, "#f1")).vis, "visible", "visible after the user's click");
      }
      await noLeaks(page, "failure matrix");
      await ctx.close();
    });
  }
}

await test("failure cover text is the media-error variant (not the spoiler stamp)", async () => {
  const { ctx, page } = await open("/ocr-pages/feed-empty.html", { mode: "strict", ocr: { delayMs: 20, rules: [{ match: "clean", status: "unavailable" }] } });
  await page.evaluate(() => addCard({ id: "t1", text: "x", src: "/ocr-media/clean.svg?id=t1" }));
  await waitFor(page, () => window.__sfw.ctl.stats().ocr.unavailable >= 1);
  await sleep(150);
  const v = await st(page, "#t1");
  assert.equal(v.variant, "mediaerr");
  assert.match(await page.evaluate(() => window.__sfw.SFW.shield.TEXT.mediaErr), /check image/i);
  assert.equal(await page.evaluate(() => !!document.querySelector("#t1 .photo > sfw-shield")), true, "real overlay element (not just the attribute cover)");
  await ctx.close();
});

await test("cache + dedupe: one request per URL for the page lifetime; removed and re-added media is a cache hit", async () => {
  // tall viewport: every card is inside the ~1.5-viewport OCR window (the window itself is tested separately)
  const { ctx, page } = await open("/ocr-pages/feed-empty.html", { viewport: { width: 640, height: 1800 }, ocr: { delayMs: 250, rules: spoilerRules } });
  await page.evaluate(() => { addCard({ id: "d1", src: "/ocr-media/clean.svg?same=1" }); addCard({ id: "d2", src: "/ocr-media/clean.svg?same=1" }); addCard({ id: "d3", src: "/ocr-media/clean.svg?same=1" }); });
  await waitFor(page, () => window.__sfw.ctl.stats().ocr.ok >= 1);
  await sleep(200);
  for (const id of ["d1", "d2", "d3"]) assert.equal((await st(page, "#" + id)).ok, "ok", id + " released from one shared answer");
  let s = await ocrStats(page);
  assert.equal(s.requests, 1, "one request for three elements: " + JSON.stringify(s));
  assert.equal(s.dedupe, 2);
  await page.evaluate(() => { addCard({ id: "d4", src: "/ocr-media/clean.svg?same=1" }); document.getElementById("d1").parentElement.remove(); addCard({ id: "d5", src: "/ocr-media/clean.svg?same=1" }); });
  await sleep(250);
  assert.equal((await st(page, "#d4")).ok, "ok");
  assert.equal((await st(page, "#d5")).ok, "ok");
  s = await ocrStats(page);
  assert.equal(s.requests, 1, "later elements are served from the cache: " + JSON.stringify(s));
  assert.ok(s.cacheHits >= 2, "cacheHits " + s.cacheHits);
  // a spoiler URL cached as spoiler shields a later duplicate without a new request
  await page.evaluate(() => { addCard({ id: "d6", src: "/ocr-media/magenta-spoiler.svg?same=2" }); });
  await waitFor(page, () => document.getElementById("d6").getAttribute("data-sfw") === "shield");
  await page.evaluate(() => { addCard({ id: "d7", src: "/ocr-media/magenta-spoiler.svg?same=2" }); });
  await waitFor(page, () => document.getElementById("d7").getAttribute("data-sfw") === "shield");
  s = await ocrStats(page);
  assert.equal(s.requests, 2, "cached spoiler answer reused: " + JSON.stringify(s));
  await noLeaks(page, "dedupe");
  await ctx.close();
});

await test("priority + bounds: nearest-to-viewport first, at most 3 in flight, nothing requested beyond ~1.5 viewports until scrolled", async () => {
  const { ctx, page } = await open("/ocr-pages/feed-empty.html", { viewport: { width: 640, height: 420 }, ocr: { manual: true, rules: spoilerRules } });
  // cards stacked ~ 330px apart: #p0 at the top ... #p9 far below (the viewport is 420px; the window is ~1.5 viewports = 630px extra)
  await page.evaluate(() => { for (let i = 0; i < 10; i++) addCard({ id: "p" + i, src: "/ocr-media/clean.svg?p=" + i, text: "card " + i }); });
  await sleep(500);
  let l = await log(page);
  const idx = (u) => Number(/p=(\d+)/.exec(u)[1]);
  assert.ok(l.length > 0 && l.length <= 3, "in flight is bounded to 3: " + l.length);
  assert.deepEqual(l.map((x) => idx(x.url)).sort((a, b) => a - b), l.map((x) => idx(x.url)).sort((a, b) => a - b));
  const first = l.map((x) => idx(x.url));
  assert.ok(first.every((i) => i <= 3), "first requests are the nearest cards: " + first);
  const pending0 = await page.evaluate(() => window.__sfw.ocr.pending.length);
  assert.equal(pending0, l.length);
  // settling one frees a slot, and the next request is the next-nearest not yet asked
  await page.evaluate((m) => window.__sfw.ocr.settle(m, { status: "ok", text: "" }), "p=" + first[0]);
  await sleep(250);
  l = await log(page);
  assert.ok(l.length >= 4, "a slot freed -> next request: " + l.length);
  assert.ok((await ocrStats(page)).maxFlight <= 3, "maxFlight " + (await ocrStats(page)).maxFlight);
  // answer everything that is asked; cards beyond the window must never have been requested
  for (let round = 0; round < 12; round++) {
    await page.evaluate(() => { for (const p of window.__sfw.ocr.pending.splice(0)) p.resolve({ status: "ok", text: "" }); });
    await sleep(120);
  }
  const asked = (await log(page)).map((x) => idx(x.url));
  const far = asked.filter((i) => i >= 8);
  assert.equal(far.length, 0, "cards 8..9 are far below the window and must not be requested before scrolling: asked " + asked);
  await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
  await sleep(400);
  const asked2 = (await log(page)).map((x) => idx(x.url));
  assert.ok(asked2.includes(9), "after scrolling the last card is requested: " + asked2);
  assert.ok((await ocrStats(page)).maxFlight <= 3);
  await ctx.close();
});

await test("src and currentSrc changes re-check: a released image becomes hidden again immediately and is re-read", async () => {
  const { ctx, page } = await open("/ocr-pages/feed-empty.html", { ocr: { delayMs: 150, rules: spoilerRules } });
  await page.evaluate(() => addCard({ id: "s1", src: "/ocr-media/clean.svg?s=1" }));
  await waitFor(page, () => document.querySelector("#s1 img").getAttribute("data-sfw-m") === "ok");
  // swap to a spoiler image; check the very next microtask-after-mutation state: hidden (no frame can have shown it)
  const vis = await page.evaluate(async () => {
    const img = document.querySelector("#s1 img");
    img.src = "/ocr-media/magenta-spoiler.svg?s=2";
    await Promise.resolve();
    await Promise.resolve();
    return { ok: img.getAttribute("data-sfw-m"), vis: getComputedStyle(img).visibility };
  });
  assert.equal(vis.ok, null, "release marker removed in the mutation callback");
  assert.equal(vis.vis, "hidden");
  await waitFor(page, () => document.getElementById("s1").getAttribute("data-sfw") === "shield");
  // responsive source swap without an attribute record on the <img>: a <picture> media query picks another currentSrc when the viewport changes
  await page.evaluate(() => addCard({ id: "s2", src: "/ocr-media/s900.svg", narrow: "/ocr-media/s320.svg" }));
  await waitFor(page, () => document.querySelector("#s2 img").getAttribute("data-sfw-m") === "ok");
  const before = (await log(page)).length;
  await page.setViewportSize({ width: 420, height: 420 });
  await waitFor(page, () => document.querySelector("#s2 img").currentSrc.includes("s320"));
  await waitFor(page, () => window.__sfw.ocr.log.some((l) => l.url.includes("s320")), null, 10000);
  await waitFor(page, () => document.querySelector("#s2 img").getAttribute("data-sfw-m") === "ok", null, 10000);
  const after = await log(page);
  assert.ok(after.some((x) => x.url.includes("s320")), "the new currentSrc was sent to OCR: " + JSON.stringify(after.map((x) => x.url)));
  assert.ok(after.length > before);
  await noLeaks(page, "src change");
  await ctx.close();
});

await test("user reveal of an OCR-shielded unit shows its image and keeps neighbours shielded", async () => {
  const { ctx, page } = await open("/ocr-pages/feed-empty.html", { ocr: { delayMs: 30, rules: spoilerRules } });
  await page.evaluate(() => { addCard({ id: "r1", src: "/ocr-media/magenta-spoiler.svg?r=1" }); addCard({ id: "r2", src: "/ocr-media/magenta-spoiler.svg?r=2" }); });
  await waitFor(page, () => document.getElementById("r1").getAttribute("data-sfw") === "shield" && document.getElementById("r2").getAttribute("data-sfw") === "shield");
  const box = await page.evaluate(() => { const r = document.getElementById("r1").getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + Math.min(r.height / 2, 40) }; });
  await page.mouse.click(box.x, box.y);
  await waitFor(page, () => document.getElementById("r1").getAttribute("data-sfw") === "revealed");
  await sleep(300);
  const s1 = await st(page, "#r1"), s2 = await st(page, "#r2");
  assert.equal(s1.vis, "visible", "image visible after the user's reveal");
  assert.equal(s2.shield, "shield");
  await ctx.close();
});

await test("config pending: media stays hidden until the host config (and OCR) arrives, then is checked, never flashed", async () => {
  const ctx = await instrumentedContext(bfBrowser, { config: { mode: "strict", delayConfigMs: 450, ocrMaybe: true, ocrOnWithConfig: true, ocr: { delayMs: 150, rules: spoilerRules } }, detection: "real", probe: true });
  const page = await ctx.newPage();
  await page.goto("about:blank");
  const rec = new FrameRecorder(ctx, page);
  await rec.start();
  await page.goto(url("/ocr-pages/feed-basic.html"), { waitUntil: "load" });
  await sleep(200);
  assert.equal((await st(page, "#c-clean")).ok, null, "not released while the config is pending");
  assert.equal((await page.evaluate(() => window.__sfw.ocr.log.length)), 0, "no request before the host is ready");
  await waitFor(page, () => window.__sfw.ctl.stats().ocr.ok >= 3, null, 10000);
  await sleep(250);
  await rec.waitFrames(12, 3000);
  assert.equal((await st(page, "#c-spoiler")).shield, "shield");
  assert.equal((await st(page, "#c-clean")).ok, "ok");
  await rec.stop();
  const p = await probe(page);
  await ctx.close();
  assert.equal(rec.unsafe + p.unsafe + p.leak + p.taskUnsafe + p.taskLeak, 0, `px=${rec.unsafe} dom=${JSON.stringify(p)}`);
});

await test("config pending, OCR not enabled: v1 behaviour resumes (released) once the config says so", async () => {
  const { ctx, page } = await open("/ocr-pages/feed-basic.html", { extraConfig: { delayConfigMs: 300, ocrMaybe: true, ocrOff: true }, ocr: { delayMs: 20, rules: spoilerRules } });
  await waitFor(page, () => document.querySelector("#c-clean img").getAttribute("data-sfw-m") === "ok", null, 6000);
  assert.equal((await page.evaluate(() => window.__sfw.ocr.log.length)), 0, "OCR never requested");
  await ctx.close();
});

await test("OCR switched on and off at runtime (config change)", async () => {
  const { ctx, page } = await open("/ocr-pages/feed-basic.html", { extraConfig: { ocrOff: true }, ocr: { manual: true, rules: spoilerRules } });
  await waitFor(page, () => document.querySelector("#c-clean img").getAttribute("data-sfw-m") === "ok");
  await page.evaluate(() => window.__sfw.ocr.enable(true));
  await waitFor(page, () => document.querySelector("#c-clean img").getAttribute("data-sfw-m") === null && window.__sfw.ocr.pending.length >= 1);
  assert.equal((await st(page, "#c-clean")).vis, "hidden", "held again once OCR is on");
  await page.evaluate(() => window.__sfw.ocr.enable(false));
  await waitFor(page, () => document.querySelector("#c-clean img").getAttribute("data-sfw-m") === "ok");
  await ctx.close();
});

await test("never blocks the page: OCR that never answers leaves media held, the page responsive, the text visible", async () => {
  const { ctx, page } = await open("/ocr-pages/feed-basic.html", { ocr: { default: { status: "hang" } } });
  await sleep(700);
  const s = await st(page, "#c-clean");
  assert.equal(s.ok, null);
  assert.equal(s.vis, "hidden");
  assert.equal(await page.evaluate(() => document.querySelector("#c-clean [data-testid=tweetText]").closest("[data-sfw]") === null), true, "caption not covered");
  const t = await page.evaluate(() => { const t0 = performance.now(); let n = 0; for (let i = 0; i < 1e5; i++) n += i; return performance.now() - t0; });
  assert.ok(t < 200, "main thread free: " + t);
  await ctx.close();
});

await test("hot page: a GTA-context unit with an image is shielded by the policy before any OCR request", async () => {
  const { ctx, page } = await open("/ocr-pages/feed-empty.html", { ocr: { delayMs: 10, rules: spoilerRules } });
  await page.evaluate(() => addCard({ id: "h1", text: "GTA 6 trailer reaction", src: "/ocr-media/clean.svg?h=1" }));
  await waitFor(page, () => document.getElementById("h1").getAttribute("data-sfw") === "shield");
  await sleep(300);
  const l = await log(page);
  assert.equal(l.length, 0, "no request for media inside an already shielded unit: " + JSON.stringify(l));
  await ctx.close();
});

await test("background-image media (inline style) is gated and read too", async () => {
  const { ctx, page } = await open("/ocr-pages/feed-empty.html", { ocr: { delayMs: 60, rules: spoilerRules } });
  await page.evaluate(() => {
    const c = document.createElement("div");
    c.setAttribute("data-testid", "cellInnerDiv");
    c.innerHTML = '<article data-testid="tweet" class="item" id="b1"><div data-testid="tweetText"><span>pic</span></div><div id="b1bg" style="width:300px;height:180px;background-image:url(/ocr-media/clean.svg?bg=1)"></div></article>' +
      '<article data-testid="tweet" class="item" id="b2"><div data-testid="tweetText"><span>pic</span></div><div id="b2bg" style="width:300px;height:180px;background-image:url(/ocr-media/magenta-spoiler.svg?bg=2)"></div></article>';
    document.getElementById("tl").appendChild(c);
  });
  const hidden = await page.evaluate(() => getComputedStyle(document.getElementById("b1bg")).backgroundImage);
  assert.equal(hidden, "none", "background image is quarantined until released");
  await waitFor(page, () => document.getElementById("b1bg").getAttribute("data-sfw-m") === "ok");
  assert.match(await page.evaluate(() => getComputedStyle(document.getElementById("b1bg")).backgroundImage), /clean\.svg/);
  await waitFor(page, () => document.getElementById("b2").getAttribute("data-sfw") === "shield");
  const l = await log(page);
  assert.deepEqual(l.map((x) => x.kind).sort(), ["bg", "bg"]);
  // changing the style later re-gates it
  await page.evaluate(() => { document.getElementById("b1bg").style.backgroundImage = "url(/ocr-media/magenta-spoiler.svg?bg=3)"; });
  const bg = await page.evaluate(async () => { await Promise.resolve(); await Promise.resolve(); return getComputedStyle(document.getElementById("b1bg")).backgroundImage; });
  assert.equal(bg, "none", "re-quarantined in the style mutation callback");
  await waitFor(page, () => document.getElementById("b1").getAttribute("data-sfw") === "shield");
  await ctx.close();
});

await test("without host.ocr behaviour is unchanged: images are released on a clean cold unit with no request", async () => {
  const { ctx, page } = await open("/ocr-pages/feed-basic.html", { ocr: undefined });
  await waitFor(page, () => document.querySelector("#c-clean img").getAttribute("data-sfw-m") === "ok");
  assert.equal(await page.evaluate(() => window.__sfw.ocr.log.length), 0);
  assert.equal((await st(page, "#c-spoiler")).ok, "ok", "v1: no OCR, so the spoiler-looking image is only held by text rules");
  await ctx.close();
});

await browser.close();
await bfBrowser.close();
await srv.close();

const failed = results.filter((r) => !r.ok);
writeFileSync(join(HERE, "out", "ocr-pipeline.json"), JSON.stringify({ results, extra }, null, 1));
console.log(`\n${results.length - failed.length}/${results.length} OCR pipeline tests passed` + (Object.keys(extra).length ? " " + JSON.stringify(extra) : ""));
process.exit(failed.length ? 1 : 0);
