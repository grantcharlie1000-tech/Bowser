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
  await ctx.route("https://i.ytimg.com/**", async (r) => {
    const res = await r.fetch({ url: `http://127.0.0.1:${srv.portA}/static/pic.svg` });
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

await test("watch metadata filters its video without covering unrelated recommendations", async () => {
  const { ctx, p } = await open("/watch/vod");
  try {
    await p.locator("ytd-watch-flexy").evaluate((watch) => watch.insertAdjacentHTML("beforeend", `
      <div id="related">
        <yt-lockup-view-model id="related-normal"><yt-thumbnail-view-model><img src="/static/pic.svg" width="220" height="120"></yt-thumbnail-view-model><a href="/watch?v=normal">How to repair a bicycle</a></yt-lockup-view-model>
        <yt-lockup-view-model id="related-gta"><yt-thumbnail-view-model><img src="/static/pic.svg" width="220" height="120"></yt-thumbnail-view-model><a href="/watch?v=gta">GTA 6 gameplay highlights</a></yt-lockup-view-model>
      </div>`));
    await p.waitForFunction(() => document.querySelector("#related-gta")?.matches('[data-sfw="shield"]'));
    assert.deepEqual(await visible(p, "#related-normal img"), [true]);
  } finally { await ctx.close(); }
});

await test("typing in a comment composer leaves the editor and normal playback usable", async () => {
  const { ctx, p } = await open("/watch");
  try {
    await p.locator("ytd-comments").evaluate((comments) => comments.insertAdjacentHTML("beforeend", `
      <ytd-comment-thread-renderer><form><label>Reply <input id="reply" type="text"></label><div id="composer" role="textbox" contenteditable="true"></div></form></ytd-comment-thread-renderer>`));
    for (const id of ["reply", "composer"]) {
      const editor = p.locator(`#${id}`);
      await editor.pressSequentially("GTA 6 gameplay", { delay: 15 });
      assert.equal(await editor.evaluate((el) => el.value ?? el.textContent), "GTA 6 gameplay");
      assert.equal(await editor.evaluate((el) => getComputedStyle(el).visibility), "visible");
    }
    assert.equal(await p.locator("video#v").evaluate((v) => v.paused), false);
  } finally { await ctx.close(); }
});

await test("a visual match covers an uncaptioned thumbnail and a timed-out check releases clean results", async () => {
  const { ctx, p } = await open("/watch", { ocr: { youtubeVisual: true, manual: true } });
  try {
    await p.locator("#related").evaluate((related) => {
      for (const id of ["untitled", "photo", "timeout"]) related.insertAdjacentHTML("beforeend",
        `<yt-lockup-view-model id="${id}"><yt-thumbnail-view-model><img width="240" height="160" src="https://i.ytimg.com/vi/${id}/hqdefault.jpg"></yt-thumbnail-view-model><a href="/watch?v=${id}">Watch this</a></yt-lockup-view-model>`);
    });
    await p.waitForFunction(() => window.__sfw.ocr.log.length === 3);
    assert.deepEqual(await visible(p, "#untitled img, #photo img, #timeout img"), [false, false, false]);
    await p.evaluate(() => {
      window.__sfw.ocr.settle("/untitled/", { status: "visual" });
      window.__sfw.ocr.settle("/photo/", { status: "ok", text: "" });
    });
    await p.waitForFunction(() => document.querySelector("#untitled").matches('[data-sfw="shield"]'));
    await p.waitForFunction(() => document.querySelector("#timeout img").getAttribute("data-sfw-m") === "ok");
    assert.deepEqual(await visible(p, "#untitled img, #photo img, #timeout img"), [false, true, true]);
    assert.equal(await p.locator("video#v").evaluate((v) => v.paused), false);
  } finally { await ctx.close(); }
});

await test("player controls can update without reclassifying the active video", async () => {
  const { ctx, p } = await open("/watch", { settle: 2400 });
  try {
    const units = await p.evaluate(async () => {
      const player = document.getElementById("movie_player");
      player.insertAdjacentHTML("beforeend", '<div class="ytp-chrome-bottom"><button>Controls</button></div>');
      await new Promise((resolve) => setTimeout(resolve, 100));
      const start = window.__sfw.ctl.stats().units;
      const button = player.querySelector("button");
      for (let i = 0; i < 24; i++) {
        button.setAttribute("aria-label", i % 2 ? "Pause, keyboard shortcut K" : "Play, keyboard shortcut K");
        await new Promise((resolve) => requestAnimationFrame(resolve));
      }
      return window.__sfw.ctl.stats().units - start;
    });
    assert.equal(units, 0);
  } finally { await ctx.close(); }
});

await browser.close();
await srv.close?.();
if (failed) { console.log(`\n${failed} youtube case(s) FAILED`); process.exit(1); }
console.log("\nall youtube cases passed");
