// Live term update in Bowser: a verified feed accepted while a page is open reaches it through the per-tab hook
// (window[hook].feed(key, json)) and the already-rendered text is re-classified. Uses the real shipped Bowser bundle
// (dist/bowser/spoiler-firewall.js) with the placeholder substituted the way src/spoiler.rs does it. Headless only.
//
// usage: node test/e2e/bowser-feed.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { startServer } from "../fixtures/server.mjs";
import { launch } from "./lib.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const BUNDLE = readFileSync(join(HERE, "..", "..", "dist", "bowser", "spoiler-firewall.js"), "utf8");
const PLACEHOLDER = "/*__SFW_CONFIG__*/null";
assert.equal(BUNDLE.split(PLACEHOLDER).length, 2, "exactly one placeholder");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const HOOK = "__sfw_" + "0123456789abcdef0123456789abcdef";
const KEY = "fedcba9876543210fedcba9876543210";
const script = (ocr, feed) => BUNDLE.replace(PLACEHOLDER, () => JSON.stringify({
  enabled: true, mode: "strict", reducedEffects: false, version: "test", private: false, ocr, ocrHook: HOOK, ocrKey: KEY, ...(feed ? { feed } : {}),
}));

const srv = await startServer();
const browser = await launch({ beginFrame: false });
let failed = 0;
const test = async (name, fn) => {
  try { await fn(); console.log(`ok   ${name}`); } catch (e) { failed++; console.log(`FAIL ${name}: ${String(e && e.message).replace(/\s+/g, " ").slice(0, 600)}`); }
};

const FEED = { version: "2026.11.19-1", add: { franchise: ["zorblax quillfeather"] } };
const TEXT = "Zorblax Quillfeather dies in the final mission";
const PAGE = `http://example.test:${srv.portA}/pages/blog.html`;

async function open(ocr, feed) {
  const ctx = await browser.newContext({ viewport: { width: 640, height: 420 } });
  await ctx.addInitScript({ content: script(ocr, feed) });
  const page = await ctx.newPage();
  await page.goto(PAGE, { waitUntil: "load" });
  await page.evaluate((t) => { const p = document.createElement("p"); p.id = "late"; p.textContent = t; document.body.appendChild(p); }, TEXT);
  await sleep(600);
  return { ctx, page };
}
const shielded = (page) => page.evaluate(() => (document.getElementById("late").getAttribute("data-sfw") || "").includes("shield"));
const push = (page, feed) => page.evaluate(([hook, key, json]) => { const h = window[hook]; return h && typeof h.feed === "function" ? (h.feed(key, json), true) : false; }, [HOOK, KEY, JSON.stringify(feed)]);

for (const ocr of [false, true]) {
  await test(`a feed pushed through the hook (ocr ${ocr}) shields text that was clear before, once`, async () => {
    const { ctx, page } = await open(ocr);
    assert.equal(await shielded(page), false, "unknown plot name: not shielded before the update");
    assert.equal(await push(page, FEED), true, "the hook exists with feed()");
    await page.waitForFunction(() => (document.getElementById("late").getAttribute("data-sfw") || "").includes("shield"), null, { timeout: 5000 });
    assert.equal(await shielded(page), true);
    // the same feed again is a no-op (the term cap is not consumed twice)
    assert.equal(await push(page, FEED), true);
    await ctx.close();
  });
}

await test("a wrong key or a malformed feed changes nothing", async () => {
  const { ctx, page } = await open(false);
  await page.evaluate(([hook, json]) => { window[hook].feed("0".repeat(32), json); }, [HOOK, JSON.stringify(FEED)]);
  await page.evaluate(([hook, key]) => { window[hook].feed(key, "not json"); window[hook].feed(key, "[1]"); window[hook].feed(key, 5); }, [HOOK, KEY]);
  await sleep(400);
  assert.equal(await shielded(page), false);
  await ctx.close();
});

await test("a feed embedded in the config is applied before the engine starts (as before)", async () => {
  const ctx = await browser.newContext({ viewport: { width: 640, height: 420 } });
  await ctx.addInitScript({ content: script(false, FEED) });
  const page = await ctx.newPage();
  await page.goto(PAGE, { waitUntil: "load" });
  await page.evaluate((t) => { const p = document.createElement("p"); p.id = "late"; p.textContent = t; document.body.appendChild(p); }, TEXT);
  await page.waitForFunction(() => (document.getElementById("late").getAttribute("data-sfw") || "").includes("shield"), null, { timeout: 5000 });
  await ctx.close();
});

await browser.close();
srv.close?.();
console.log(failed ? `\n${failed} bowser-feed test(s) FAILED` : "\nall bowser-feed tests passed");
process.exit(failed ? 1 : 0);
