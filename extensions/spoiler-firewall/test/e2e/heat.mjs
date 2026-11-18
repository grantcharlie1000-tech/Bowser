// Context heat on real-feed shapes (headless, muted): a "GTA 6 before ..." joke reply must not turn its whole conversation
// into GTA context, and a warm launch-week timeline must not cover ordinary posts or photo posts that name a common first
// name. Real spoilers on the same pages are still covered. Every unit's expectation is in the fixture markup (data-expect).
//
// usage: node test/e2e/heat.mjs
import assert from "node:assert/strict";
import { startServer } from "../fixtures/server.mjs";
import { launch, instrumentedContext } from "./lib.mjs";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const srv = await startServer();
const browser = await launch();
let failed = 0;

async function run(name, host, path, settle) {
  const ctx = await instrumentedContext(browser, { config: { mode: "strict" }, detection: "real", probe: true, viewport: { width: 900, height: 1400 } });
  const p = await ctx.newPage();
  await p.goto(`http://${host}:${srv.portA}${path}`, { waitUntil: "load" });
  await sleep(settle);
  const a = await p.evaluate(() => window.__audit());
  await ctx.close();
  try {
    assert.deepEqual(a.missed, [], `${name}: missed spoilers`);
    assert.deepEqual(a.falseList, [], `${name}: ordinary posts covered`);
    assert.ok(a.shieldExpected > 0 && a.clearExpected > 0, `${name}: fixture loaded`);
    console.log(`ok   ${name} shielded=${a.shieldedOk}/${a.shieldExpected} clear=${a.clearExpected - a.falseShield}/${a.clearExpected}`);
  } catch (e) { failed++; console.log(`FAIL ${String(e.message).split("\n")[0]} missed=${JSON.stringify(a.missed)} false=${JSON.stringify(a.falseList)}`); }
}

await run("X conversation with one GTA joke reply", "x.com", "/someone/status/1234567890", 800);
await run("X post page whose post is a spoiler: replies with profile pictures are not covered for it", "x.com", "/leaker/status/2222", 800);
{
  // Profile pictures are never held: in the spoiler's context (where media is only released for cold units) every clear
  // reply still shows its picture, while a real photo there stays hidden and a covered post keeps its picture covered.
  const ctx = await instrumentedContext(browser, { config: { mode: "strict" }, detection: "real", probe: false, viewport: { width: 900, height: 1400 } });
  const p = await ctx.newPage();
  await p.goto(`http://x.com:${srv.portA}/leaker/status/2222`, { waitUntil: "load" });
  await sleep(800);
  const a = await p.evaluate(() => {
    const shown = (e) => { const cs = getComputedStyle(e); return cs.visibility === "visible" && (e.tagName === "IMG" || cs.backgroundImage !== "none"); };
    const covered = (e) => !!e.closest("[data-sfw~=shield]");
    const avatars = [...document.querySelectorAll("[data-testid^=UserAvatar-Container] img, [data-testid^=UserAvatar-Container] [style*=background-image]")];
    return {
      hiddenClear: avatars.filter((e) => !covered(e) && !shown(e)).map((e) => e.closest("article").id + ":" + e.tagName),
      clearCount: avatars.filter((e) => !covered(e)).length,
      coveredCount: avatars.filter(covered).length,
      photoShown: [...document.querySelectorAll("[data-testid=tweetPhoto] img")].filter((e) => !covered(e) && shown(e)).length,
    };
  });
  await ctx.close();
  try {
    assert.ok(a.clearCount >= 10 && a.coveredCount >= 4, `fixture loaded: ${JSON.stringify(a)}`);
    assert.deepEqual(a.hiddenClear, [], "profile pictures of clear replies hidden");
    assert.equal(a.photoShown, 0, "a photo in the spoiler's context shown uncovered");
    console.log(`ok   profile pictures of clear replies shown in the spoiler's context: ${a.clearCount}`);
  } catch (e) { failed++; console.log(`FAIL ${String(e.message).split("\n")[0]}`); }
}
await run("X page of one spoiler reply: the rest of the page is not covered for it", "x.com", "/fan/status/3333", 800);
await run("X launch-week home timeline (warm feed)", "x.com", "/launch/home", 1400);

await run("X counters ticking in place; a digit that completes the franchise name in place", "x.com", "/counters/home", 1200);
{
  // the counter ticks were skipped, not re-classified each time
  const ctx = await instrumentedContext(browser, { config: { mode: "strict" }, detection: "real", probe: false });
  const p = await ctx.newPage();
  await p.goto(`http://x.com:${srv.portA}/counters/home`, { waitUntil: "load" });
  await sleep(1200);
  const st = await p.evaluate(() => window.__sfw.ctl.stats());
  await ctx.close();
  try { assert.ok(st.counterSkips >= 20, `counter ticks skipped: ${st.counterSkips}`); console.log(`ok   counter ticks skipped without re-classification: ${st.counterSkips}`); }
  catch (e) { failed++; console.log(`FAIL ${e.message}`); }
}

await browser.close();
srv.close?.();
console.log(failed ? `\n${failed} heat case(s) FAILED` : "\nall heat cases passed");
process.exit(failed ? 1 : 0);
