// Cards with pictures (audit fix): on a generic page a spoiler block inside a small card covers the whole card, so the
// picture beside it (a CSS background-image div, a plain <img>, a link) never shows; a long article of paragraphs is still
// shielded per paragraph, and a small article of paragraphs only covers the spoiler paragraph. Plain and streamed loads.
// The frame-exact leak check for the same fixture is zero-flash.mjs (fixture adv-card).
//
// usage: node test/e2e/cards.mjs   (headless chrome-headless-shell only, muted)
import assert from "node:assert/strict";
import { startServer } from "../fixtures/server.mjs";
import { launch, instrumentedContext } from "./lib.mjs";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const srv = await startServer();
const browser = await launch({ beginFrame: false });
let failed = 0;

const shieldOf = (e) => (e.getAttribute("data-sfw") || "").split(" ").includes("shield");
async function state(page) {
  return page.evaluate(() => {
    const sh = (e) => (e.getAttribute("data-sfw") || "").split(" ").includes("shield");
    const out = {};
    for (const a of document.querySelectorAll("article")) out[a.id] = { whole: sh(a), paras: [...a.querySelectorAll("p")].filter(sh).length, total: a.querySelectorAll("p").length };
    const pic = (id, sel) => { const e = document.querySelector(`#${id} ${sel}`); return e ? { vis: getComputedStyle(e).visibility, covered: !!e.closest("[data-sfw~=shield]") } : null; };
    out.bgPic = pic("c-bg", ".pic");
    out.imgPic = pic("c-img", ".pic");
    out.linkBox = pic("c-link", ".sbox");
    out.cleanPic = pic("c-clean", ".pic");
    return out;
  });
}

async function run(name, query) {
  const ctx = await instrumentedContext(browser, { config: { mode: "strict" }, detection: "real", probe: true });
  const page = await ctx.newPage();
  await page.goto(`http://example.test:${srv.portA}/pages/adv-card.html${query}`, { waitUntil: "load", timeout: 60000 });
  await sleep(500);
  await page.waitForFunction(() => document.readyState === "complete", null, { timeout: 20000 });
  await sleep(700);
  const s = await state(page);
  const probe = await page.evaluate(() => { const p = window.__probe; return p.unsafe + p.leak + p.taskUnsafe + p.taskLeak; });
  await ctx.close();
  try {
    for (const id of ["c-bg", "c-img", "c-link"]) assert.equal(s[id].whole, true, `${name}: card ${id} is covered whole`);
    assert.equal(s["c-clean"].whole, false, `${name}: a clean card with a background picture is untouched`);
    assert.equal(s.cleanPic.covered, false, `${name}: clean card picture is not covered`);
    assert.equal(s.bgPic.covered, true, `${name}: the CSS background picture is under the shield`);
    assert.equal(s.imgPic.covered, true, `${name}: the <img> is under the shield`);
    assert.equal(s.linkBox.covered, true, `${name}: the link box is under the shield`);
    assert.equal(s.long.whole, false, `${name}: a long article is not covered whole`);
    assert.equal(s.long.paras, 1, `${name}: only the spoiler paragraph of the long article is shielded`);
    assert.ok(s.long.total >= 17, `${name}: fixture has many paragraphs`);
    assert.equal(probe, 0, `${name}: DOM probe saw an unshielded sentinel`);
    console.log(`ok   ${name}`);
  } catch (e) { failed++; console.log(`FAIL ${name}: ${String(e.message).replace(/\s+/g, " ").slice(0, 500)} ${JSON.stringify(s)}`); }
}

await run("plain load", "");
await run("streamed load (chunked)", "?__chunk=100&__size=60");

// A small article that is only paragraphs (no picture, no embed): only the spoiler paragraph is covered.
{
  const ctx = await instrumentedContext(browser, { config: { mode: "strict" }, detection: "real", probe: true });
  const page = await ctx.newPage();
  await page.goto(`http://example.test:${srv.portA}/pages/news.html`, { waitUntil: "load" });
  await sleep(1200);
  const s = await state(page);
  await ctx.close();
  try {
    const art = Object.values(s).find((v) => v && typeof v.total === "number");
    assert.equal(art.whole, false, "a small article of paragraphs is not covered whole");
    assert.equal(art.paras, 1, "only the spoiler paragraph is shielded");
    console.log("ok   small article of paragraphs: per paragraph");
  } catch (e) { failed++; console.log(`FAIL small article: ${e.message} ${JSON.stringify(s)}`); }
}

await browser.close();
srv.close?.();
console.log(failed ? `\n${failed} card test(s) FAILED` : "\nall card tests passed");
process.exit(failed ? 1 : 0);
