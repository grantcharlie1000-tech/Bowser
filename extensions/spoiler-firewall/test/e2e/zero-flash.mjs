// Zero-flash harness. For every fixture (plain and chunked-streamed) it
//  (a) records EVERY compositor frame via HeadlessExperimental.beginFrame in chrome-headless-shell and counts
//      sentinel-coloured (#ff00ff) pixels -> "unsafe frames", and
//  (b) runs an in-page DOM invariant probe in every requestAnimationFrame (registered after the engine): no
//      connected, rendered sentinel element may exist outside a shield, and none may be visible inside one.
// It also reports time-to-shield, shield counts, false shields on data-expect="clear" units and engine batch cost.
// A negative control (engine disabled) must produce unsafe frames, proving the detector can see a leak.
//
// usage: node test/e2e/zero-flash.mjs [--detection=real|stub] [--only=id,id] [--no-chunked] [--no-control]
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { startServer } from "../fixtures/server.mjs";
import { FIXTURES, fixtureUrl } from "../fixtures/manifest.mjs";
import { launch, instrumentedContext, FrameRecorder } from "./lib.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = join(HERE, "out");
mkdirSync(OUT, { recursive: true });
const arg = (k, d) => { const a = process.argv.find((x) => x.startsWith(`--${k}=`)); return a ? a.split("=")[1] : d; };
const flag = (k) => process.argv.includes(`--${k}`);
const detection = arg("detection", "real");
const only = arg("only", "") ? arg("only", "").split(",") : null;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const SCENARIOS = {
  async scroll(page) { for (let i = 0; i < 10; i++) { await page.mouse.wheel(0, 450); await sleep(120); } },
  async virtual(page) { for (let i = 0; i < 14; i++) { await page.evaluate((n) => { document.getElementById("vp").scrollTop = n * 150; }, i); await sleep(100); } },
  async spa(page) { await page.evaluate(() => window.go("gta 6 ending")); await sleep(300); await page.evaluate(() => window.go("gta 6 ending spoiler")); await sleep(300); },
};

async function collect(page) {
  const frames = page.frames();
  const res = { probe: { frames: 0, unsafe: 0, leak: 0, taskScans: 0, taskUnsafe: 0, taskLeak: 0, examples: [] }, audit: { shieldExpected: 0, shieldedOk: 0, missed: [], clearExpected: 0, falseShield: 0, falseList: [], evasiveMissed: 0, evasiveTotal: 0 }, stats: null };
  for (const f of frames) {
    try {
      const r = await f.evaluate(() => ({ p: window.__probe, a: window.__audit && window.__audit(), s: window.__sfw && window.__sfw.ctl && window.__sfw.ctl.stats() }));
      if (r.p) { res.probe.frames += r.p.frames; res.probe.unsafe += r.p.unsafe; res.probe.leak += r.p.leak; res.probe.taskScans += r.p.taskScans || 0; res.probe.taskUnsafe += r.p.taskUnsafe || 0; res.probe.taskLeak += r.p.taskLeak || 0; res.probe.examples.push(...r.p.examples); }
      if (r.a) {
        for (const k of ["shieldExpected", "shieldedOk", "clearExpected", "falseShield", "evasiveMissed", "evasiveTotal"]) res.audit[k] += r.a[k];
        res.audit.missed.push(...r.a.missed);
        res.audit.falseList.push(...r.a.falseList);
      }
      if (r.s && (!res.stats || f === page.mainFrame())) res.stats = r.s;
    } catch (e) { /* detached frame */ }
  }
  return res;
}

async function runOne(browser, srv, fx, { chunked, enabled = true }) {
  const ctx = await instrumentedContext(browser, { config: { enabled }, detection });
  const page = await ctx.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("about:blank");
  const rec = new FrameRecorder(ctx, page);
  await rec.start();
  const t0 = Date.now();
  let navErr = null;
  try { await page.goto(fixtureUrl(fx, srv.portA, { chunked }), { waitUntil: "load", timeout: 30000 }); } catch (e) { navErr = e.message; }
  if (fx.scenario && SCENARIOS[fx.scenario]) await SCENARIOS[fx.scenario](page);
  await sleep(fx.settle || 400);
  await rec.waitFrames(20, 3500);
  const c = await collect(page);
  await rec.stop();
  if (rec.firstUnsafe && enabled) writeFileSync(join(OUT, `unsafe-${fx.id}${chunked ? "-chunked" : ""}.png`), Buffer.from(rec.firstUnsafe.png, "base64"));
  await ctx.close();
  const st = c.stats || {};
  const hotFx = !!fx.hotUrl || !!fx.pageShield;
  const r2 = (v) => (v == null ? v : Math.round(v * 100) / 100);
  return {
    id: fx.id, chunked, enabled, ms: Date.now() - t0, navErr, pageErrors: errors.slice(0, 2),
    frames: rec.frames, shots: rec.shots, unsafeFrames: rec.unsafe, maxSentinelPixels: rec.maxPixels, bfErrors: rec.errors,
    domProbeFrames: c.probe.frames, domUnsafeFrames: c.probe.unsafe, taskScans: c.probe.taskScans, taskUnsafe: c.probe.taskUnsafe + c.probe.taskLeak, domLeakFrames: c.probe.leak, domExamples: c.probe.examples.slice(0, 3),
    shieldExpected: c.audit.shieldExpected, shieldedOk: c.audit.shieldedOk, missed: c.audit.missed.slice(0, 5),
    clearExpected: hotFx ? 0 : c.audit.clearExpected, falseShield: hotFx ? 0 : c.audit.falseShield, falseList: hotFx ? [] : c.audit.falseList.slice(0, 5),
    evasiveTotal: c.audit.evasiveTotal, evasiveMissed: c.audit.evasiveMissed,
    shields: st.shields, firstShieldAtMs: st.firstShieldAt ? Math.round(st.firstShieldAt) : null,
    batches: st.batches, meanBatchMs: r2(st.meanBatchMs), p95BatchMs: r2(st.p95BatchMs), maxBatchMs: r2(st.maxBatchMs),
    rafCaught: st.rafCaught, charDataRecords: st.charDataRecords, errors: st.errors, lastError: st.lastError || null, pending: st.pending,
  };
}

const srv = await startServer();
const browser = await launch({ beginFrame: true });
const results = [];
const list = FIXTURES.filter((f) => !only || only.includes(f.id));
for (const fx of list) {
  for (const chunked of fx.chunk && !flag("no-chunked") ? [false, true] : [false]) {
    const r = await runOne(browser, srv, fx, { chunked });
    results.push(r);
    const gap = FIXTURES.find((f) => f.id === fx.id).knownGap;
    const bad = !gap && (r.unsafeFrames || r.domUnsafeFrames || r.domLeakFrames || r.taskUnsafe);
    console.log(`${gap ? "GAP " : bad ? "FAIL" : "ok  "} ${fx.id}${chunked ? " [chunked]" : ""} frames=${r.frames}/${r.shots} unsafe(px)=${r.unsafeFrames} unsafe(dom)=${r.domUnsafeFrames} taskScans=${r.taskScans} unsafe(task)=${r.taskUnsafe} leak(dom)=${r.domLeakFrames} shields=${r.shields} first=${r.firstShieldAtMs}ms missed=${r.missed.length} falseShield=${r.falseShield}/${r.clearExpected} p95=${r.p95BatchMs}ms raf=${r.rafCaught} cd=${r.charDataRecords}${r.errors ? " ERR=" + r.errors + " " + r.lastError : ""}`);
  }
}
const controls = [];
if (!flag("no-control")) {
  for (const id of ["x-timeline", "news", "adv-shadow", "rt-sandbox-srcdoc", "rt-sandbox-src"]) {
    const fx = FIXTURES.find((f) => f.id === id);
    if (only && !only.includes(id)) continue;
    const r = await runOne(browser, srv, fx, { chunked: false, enabled: false });
    controls.push(r);
    console.log(`control (engine off) ${id}: unsafe frames (px)=${r.unsafeFrames} max sentinel px=${r.maxSentinelPixels} dom unsafe=${r.domUnsafeFrames}`);
  }
}
await browser.close();
await srv.close();

const gapIds = new Set(FIXTURES.filter((f) => f.knownGap).map((f) => f.id));
const real = results.filter((r) => !/adv-evasive/.test(r.id) && !gapIds.has(r.id));
const sum = (k) => real.reduce((a, r) => a + (r[k] || 0), 0);
const summary = {
  detection, fixtures: results.length, framesCaptured: results.reduce((a, r) => a + r.frames, 0), screenshotsDecoded: results.reduce((a, r) => a + r.shots, 0),
  unsafeFramesPixel: sum("unsafeFrames"), unsafeFramesDom: sum("domUnsafeFrames"), taskBoundaryScans: sum("taskScans"), unsafeTaskBoundaries: sum("taskUnsafe"), leakFramesDom: sum("domLeakFrames"),
  shieldExpected: sum("shieldExpected"), shieldedOk: sum("shieldedOk"), clearExpected: sum("clearExpected"), falseShield: sum("falseShield"),
  knownGaps: results.filter((r) => gapIds.has(r.id)).map((r) => ({ id: r.id, unsafeFramesPixel: r.unsafeFrames })),
  evasive: results.filter((r) => r.evasiveTotal).map((r) => ({ id: r.id, total: r.evasiveTotal, missed: r.evasiveMissed, unsafeFramesPixel: r.unsafeFrames })),
  rafCaughtTotal: results.reduce((a, r) => a + (r.rafCaught || 0), 0), charDataRecordsTotal: results.reduce((a, r) => a + (r.charDataRecords || 0), 0),
  p95BatchMsMax: Math.max(0, ...results.map((r) => r.p95BatchMs || 0)), maxBatchMsMax: Math.max(0, ...results.map((r) => r.maxBatchMs || 0)),
  controlUnsafeFrames: controls.map((c) => ({ id: c.id, unsafeFrames: c.unsafeFrames })),
};
writeFileSync(join(OUT, "zero-flash.json"), JSON.stringify({ summary, results, controls }, null, 1));
console.log("\nSUMMARY " + JSON.stringify(summary, null, 1));
const fail = summary.unsafeFramesPixel + summary.unsafeFramesDom + summary.leakFramesDom + summary.unsafeTaskBoundaries > 0 || (controls.length > 0 && controls.every((c) => c.unsafeFrames === 0)) || results.some((r) => r.navErr);

process.exit(fail ? 1 : 0);
