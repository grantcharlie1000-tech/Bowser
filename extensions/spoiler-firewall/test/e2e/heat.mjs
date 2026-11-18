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
