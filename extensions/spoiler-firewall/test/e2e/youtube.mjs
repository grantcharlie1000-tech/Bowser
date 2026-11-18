// YouTube (headless, muted): pictures are judged by the words of their result (titles, descriptions), never held for OCR,
// and the player of a live stream is never checked or covered while its title, description, comments and chat are.
// Every unit's expectation is in the fixture markup (data-expect).
//
// usage: node test/e2e/youtube.mjs
import assert from "node:assert/strict";
import { startServer } from "../fixtures/server.mjs";
import { launch, instrumentedContext } from "./lib.mjs";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const srv = await startServer();
const browser = await launch();
let failed = 0;
// a slow OCR (4 s per image): anything that waited for it would still be hidden when the checks run
const SLOW_OCR = { default: { status: "ok", text: "" }, delayMs: 4000 };

async function test(name, fn) {
  try { await fn(); console.log(`ok   ${name}`); } catch (e) { failed++; console.log(`FAIL ${name}: ${String(e && e.message).split("\n")[0].slice(0, 400)}`); }
}
async function open(path, { mode = "strict", ocr = SLOW_OCR, settle = 700 } = {}) {
  const ctx = await instrumentedContext(browser, { config: { mode, ocr }, detection: "real", probe: true, viewport: { width: 900, height: 1400 } });
  // youtube.com is HSTS-preloaded: Chromium upgrades http to https, which the plain fixture server cannot answer. Requests
  // for it are answered from the fixture server instead.
  await ctx.route(/^https?:\/\/www\.youtube\.com(:\d+)?\//, async (r) => {
    const u = new URL(r.request().url());
    const res = await r.fetch({ url: `http://127.0.0.1:${srv.portA}${u.pathname}${u.search}` });
    await r.fulfill({ response: res });
  });
  const p = await ctx.newPage();
  await p.goto(`https://www.youtube.com/${path.slice(1)}`, { waitUntil: "load" });
  await sleep(settle);
  return { ctx, p };
}
const audit = async (p, label) => {
  const a = await p.evaluate(() => window.__audit());
  assert.deepEqual(a.missed, [], `${label}: missed spoilers (engine error: ${await p.evaluate(() => window.__sfw.ctl.stats().lastError || "none")})`);
  assert.deepEqual(a.falseList, [], `${label}: wrongly covered`);
  return a;
};
const visible = (p, sel) => p.evaluate((s) => Array.from(document.querySelectorAll(s)).map((e) => getComputedStyle(e).visibility === "visible" && e.getAttribute("data-sfw-m") === "ok"), sel);

await test("thumbnails of clear results show without waiting for OCR", async () => {
  const { ctx, p } = await open("/results/thumbs");
  await audit(p, "search");
  assert.deepEqual(await visible(p, "#v1 img, #v3 img"), [true, true], "clear results' thumbnails visible");
  const log = await p.evaluate(() => window.__sfw.ocr.log.length);
  assert.equal(log, 0, "no OCR requests on YouTube");
  await ctx.close();
});

await test("live stream: the player plays uncovered; title, description, comments and chat are checked", async () => {
  const { ctx, p } = await open("/live/abc123");
  await audit(p, "live");
  assert.deepEqual(await visible(p, "#movie_player video"), [true], "live video visible");
  await ctx.close();
});

await test("ordinary video with the same page: the player is still covered", async () => {
  const { ctx, p } = await open("/watch/vod");
  await audit(p, "vod");
  await ctx.close();
});

await test("a video that turns live (class added after load) is released; turning back covers it again", async () => {
  const { ctx, p } = await open("/watch/vod");
  await p.evaluate(() => { const m = document.getElementById("movie_player"); m.classList.add("ytp-live"); m.insertAdjacentHTML("beforeend", '<span class="ytp-time-current">0:01</span>'); });
  await sleep(700);
  const shielded = () => p.evaluate(() => window.__sfw.SFW.shield.state(document.getElementById("movie_player")).mode);
  assert.equal(await shielded(), "none", "released once live");
  assert.deepEqual(await visible(p, "#movie_player video"), [true], "live video visible");
  await p.evaluate(() => { const m = document.getElementById("movie_player"); m.classList.remove("ytp-live"); m.querySelector(".ytp-time-current").textContent = "0:02"; });
  await sleep(700);
  assert.notEqual(await shielded(), "none", "covered again when no longer live");
  await ctx.close();
});

await browser.close();
await srv.close?.();
if (failed) { console.log(`\n${failed} youtube case(s) FAILED`); process.exit(1); }
console.log("\nall youtube cases passed");
