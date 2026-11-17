// Spoiler Firewall performance measurement (MEASURES, does not optimise). Headless only, muted, below-normal priority.
//
//   node test/perf/run-perf.mjs [--reps 5] [--fixtures feed,video-grid,video-watch,article,search] [--variants off,ext,ext-ocr,bowser]
//                               [--throttle 4,6] [--no-profile] [--no-stats] [--quick]
//   node test/perf/run-perf.mjs --tables test/perf/results/<file>.json     (re-print the summary tables from a result file)
//
// Design (see docs-perf/PERFORMANCE.md):
//  * every comparison is PAIRED and INTERLEAVED: for each repetition and fixture the variants run back to back,
//    in an order that rotates per repetition (off is not always first); each run uses a FRESH browser profile.
//  * variants: off = no extension; ext = real dist/chrome (OCR off); ext-ocr = real dist/chrome with the optional
//    offscreen permission granted through the options page switch (a real click, like test/chrome/ocr-extension.mjs);
//    bowser = dist/bowser/spoiler-firewall.js injected with addInitScript (placeholder replaced by enabled/strict).
//    diag (not part of the overhead table): bowser-stats = the same bundle patched IN MEMORY to expose the runtime
//    controller, used only to read the engine's own batch timings.
//  * highRisk hosts: engine/adapters.js decides highRisk purely from location.hostname (x.com, youtube.com, google.com
//    are highRisk adapters; anything else falls to "generic", highRisk=false). The shipped bundles have no override, so
//    pages are served over http from a local server and the hostnames are mapped with
//    --host-resolver-rules="MAP x.com 127.0.0.1, ..., MAP * ~NOTFOUND, EXCLUDE 127.0.0.1".
import http from "node:http";
import os from "node:os";
import { readFileSync, writeFileSync, mkdirSync, mkdtempSync, rmSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn, spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { loadPlaywright, findChromium, getServiceWorker, ROOT, DIST_CHROME } from "../chrome/harness.mjs";
import { startServer, HOSTS } from "./fixtures/server.mjs";

try { os.setPriority(process.pid, os.constants.priority.PRIORITY_BELOW_NORMAL); } catch { /* ignore */ }
const HERE = dirname(fileURLToPath(import.meta.url));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const argv = process.argv.slice(2);
const flag = (n) => argv.includes("--" + n);
const opt = (n, d) => { const i = argv.indexOf("--" + n); return i >= 0 && argv[i + 1] && !argv[i + 1].startsWith("--") ? argv[i + 1] : d; };

// ---------------------------------------------------------------------------------------------- statistics
const sorted = (a) => a.filter((x) => x != null && Number.isFinite(x)).sort((x, y) => x - y);
function quant(a, p) { const s = sorted(a); if (!s.length) return null; const i = (s.length - 1) * p, lo = Math.floor(i), hi = Math.ceil(i); return s[lo] + (s[hi] - s[lo]) * (i - lo); }
const med = (a) => quant(a, 0.5);
const iqr = (a) => { const q1 = quant(a, 0.25), q3 = quant(a, 0.75); return q1 == null ? null : [q1, q3]; };
const r2 = (x) => (x == null ? null : Math.round(x * 100) / 100);

// ---------------------------------------------------------------------------------------------- fixtures / variants
const FIXTURES = {
  feed: { host: "x.com", path: "/home", scrollMs: 10000, speed: 3000, label: "Feed (X-like, 300 posts, infinite scroll)" },
  "video-grid": { host: "youtube.com", path: "/", scrollMs: 5000, speed: 2500, label: "Video grid (120 thumbnails)" },
  "video-watch": { host: "youtube.com", path: "/watch?v=abc", scrollMs: 5000, speed: 1500, label: "Watch page (2 muted videos, 100 comments)" },
  article: { host: "news.test", path: "/story", scrollMs: 5000, speed: 3000, label: "Article (20k words, clean, generic host)" },
  search: { host: "google.com", path: "/search?q=harbor+town", scrollMs: 5000, speed: 0, churn: true, label: "Search + autocomplete churn (5 s)" },
};
const ALL_VARIANTS = ["off", "ext", "ext-ocr", "bowser"];
const REPS = Number(opt("reps", flag("quick") ? 2 : 5));
const fixtureNames = opt("fixtures", Object.keys(FIXTURES).join(",")).split(",");
const variantNames = opt("variants", ALL_VARIANTS.join(",")).split(",");
const THROTTLES = opt("throttle", "4,6").split(",").map(Number).filter((x) => x > 1);
const THROTTLE_VARIANTS = ["off", "ext", "bowser"]; // OCR runs in another process (offscreen document): CDP page throttling does not apply to it
const SETTLE_MS = 2000;

// ---------------------------------------------------------------------------------------------- Windows helpers
function ps(cmd) { const r = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", cmd], { encoding: "utf8" }); return (r.stdout || "").trim(); }
function procs(marker) {
  const out = ps(`Get-CimInstance Win32_Process -Filter "Name='chrome.exe' OR Name='chrome-headless-shell.exe'" | Where-Object { $_.CommandLine -like '*${marker}*' } | ForEach-Object { $t='browser'; if ($_.CommandLine -match '--type=([a-z-]+)') { $t=$Matches[1] }; $x=if ($_.CommandLine -match '--extension-process') {'ext'} else {''}; $g=Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue; if ($g) { "$($_.ProcessId)|$t|$x|$($g.WorkingSet64)|$($g.PrivateMemorySize64)" } }`);
  return out.split(/\r?\n/).filter(Boolean).map((l) => { const [pid, type, ext, ws, priv] = l.split("|"); return { pid: +pid, type, ext: ext === "ext", ws: +ws, priv: +priv }; });
}
const MB = (n) => n / 1048576;
function memSummary(list) {
  const rend = list.filter((p) => p.type === "renderer" && !p.ext).sort((a, b) => b.priv - a.priv);
  const ext = list.filter((p) => p.ext);
  const sum = (l, k) => l.reduce((a, p) => a + p[k], 0);
  return {
    pagePrivMB: rend[0] ? MB(rend[0].priv) : null, pageWsMB: rend[0] ? MB(rend[0].ws) : null, pageRenderers: rend.length,
    extPrivMB: ext.length ? MB(sum(ext, "priv")) : 0, extWsMB: ext.length ? MB(sum(ext, "ws")) : 0,
    treePrivMB: MB(sum(list, "priv")), treeWsMB: MB(sum(list, "ws")),
  };
}
function killMarker(marker) {
  ps(`Get-CimInstance Win32_Process -Filter "Name='chrome.exe' OR Name='chrome-headless-shell.exe'" | Where-Object { $_.CommandLine -like '*${marker}*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }`);
}
function machine() {
  const cpu = ps(`(Get-CimInstance Win32_Processor | ForEach-Object { "$($_.Name.Trim()) | $($_.NumberOfCores) cores / $($_.NumberOfLogicalProcessors) threads | $($_.MaxClockSpeed) MHz max" }) -join '; '`);
  const ram = ps(`[math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory/1GB,1)`);
  const osv = ps(`(Get-CimInstance Win32_OperatingSystem).Caption + ' ' + (Get-CimInstance Win32_OperatingSystem).Version`);
  return { cpu, ramGB: Number(ram), os: osv, node: process.version, arch: process.arch };
}
// Background CPU load sampler: typeperf (Windows performance counters), one sample every 2 s for the whole run.
function startLoadSampler() {
  const samples = [];
  let child = null;
  try {
    child = spawn("typeperf", ["\\Processor(_Total)\\% Processor Time", "-si", "2"], { stdio: ["ignore", "pipe", "ignore"], windowsHide: true });
    let buf = "";
    child.stdout.on("data", (d) => { buf += d; const lines = buf.split(/\r?\n/); buf = lines.pop(); for (const l of lines) { const m = /^"[^"]*","([\d.]+)"/.exec(l); if (m) samples.push({ t: Date.now(), v: Number(m[1]) }); } });
    child.on("error", () => {});
  } catch { /* ignore */ }
  return { samples, stop() { try { child && child.kill(); } catch { /* ignore */ } }, mean(t0, t1) { const s = samples.filter((x) => x.t >= t0 - 2000 && x.t <= t1 + 2000).map((x) => x.v); return s.length ? s.reduce((a, b) => a + b, 0) / s.length : null; } };
}

// ---------------------------------------------------------------------------------------------- page probe (installed in EVERY variant, incl. off)
const PROBE = `(() => { if (window.__perf) return; const P = window.__perf = { lt: [], fcp: null, lcp: null };
try { new PerformanceObserver((l) => { for (const e of l.getEntries()) P.lt.push([e.startTime, e.duration]); }).observe({ type: "longtask", buffered: true }); } catch (e) {}
try { new PerformanceObserver((l) => { for (const e of l.getEntries()) if (e.name === "first-contentful-paint") P.fcp = e.startTime; }).observe({ type: "paint", buffered: true }); } catch (e) {}
try { new PerformanceObserver((l) => { for (const e of l.getEntries()) P.lcp = e.startTime; }).observe({ type: "largest-contentful-paint", buffered: true }); } catch (e) {}
P.run = (ms, speed, fn) => new Promise((res) => { const iv = []; let last = 0, t0 = 0; const start = performance.now(); const maxY = () => document.documentElement.scrollHeight - innerHeight; if (fn) fn();
  function f(ts) { if (!t0) t0 = ts; if (last) iv.push(ts - last); last = ts; const el = ts - t0; if (speed) scrollTo(0, Math.min(maxY(), el * speed / 1000)); if (el < ms) requestAnimationFrame(f); else res({ iv, start, end: performance.now(), y: scrollY, docH: document.documentElement.scrollHeight }); }
  requestAnimationFrame(f); }); })();`;

// ---------------------------------------------------------------------------------------------- bundles
function bowserBundle(patched) {
  let t = readFileSync(join(ROOT, "dist", "bowser", "spoiler-firewall.js"), "utf8");
  if (!t.includes("/*__SFW_CONFIG__*/null")) throw new Error("placeholder not found in dist/bowser bundle");
  t = t.replace("/*__SFW_CONFIG__*/null", '{"enabled":true,"mode":"strict"}');
  if (patched) { if (!t.includes("SFW.runtime.start({")) throw new Error("runtime.start not found"); t = t.replace("SFW.runtime.start({", "globalThis.__sfwctl = SFW.runtime.start({"); }
  return t + "\n//# sourceURL=sfw-bowser-bundle.js\n";
}
function moduleMap(text) { // line (0-based) -> engine module name, from the "// ---- engine/x.js ----" markers build.mjs writes
  const lines = text.split("\n"), marks = [];
  lines.forEach((l, i) => { const m = /^\/\/ ---- ((?:engine|hosts)\/[\w./-]+) ----/.exec(l); if (m) marks.push([i, m[1]]); });
  return (line) => { let name = "(prelude)"; for (const [i, n] of marks) { if (i <= line) name = n; else break; } return name; };
}

// ---------------------------------------------------------------------------------------------- launching
const PW = loadPlaywright();
const CHROMIUM = findChromium(PW);
async function launch(variant, rules) {
  const dir = mkdtempSync(join(tmpdir(), "sfw-perf-"));
  const marker = dir.split(/[\\/]/).pop();
  const ext = variant === "ext" || variant === "ext-ocr";
  const args = ["--headless=new", "--mute-audio", "--no-first-run", "--no-default-browser-check", "--disable-background-networking", "--disable-component-update", "--disable-sync", "--disable-default-apps", "--autoplay-policy=no-user-gesture-required", `--host-resolver-rules=${rules}`];
  if (ext) args.push(`--disable-extensions-except=${DIST_CHROME}`, `--load-extension=${DIST_CHROME}`, "--disable-features=DisableLoadExtensionCommandLineSwitch");
  const context = await PW.chromium.launchPersistentContext(dir, { executablePath: CHROMIUM, headless: false, args, viewport: { width: 1280, height: 800 }, ignoreDefaultArgs: ["--disable-extensions", "--headless"] });
  return { context, dir, marker, ext };
}
async function closeAll(h) {
  try { await Promise.race([h.context.close(), sleep(15000)]); } catch { /* ignore */ }
  killMarker(h.marker);
  await sleep(150);
  try { rmSync(h.dir, { recursive: true, force: true, maxRetries: 3 }); } catch { /* ignore */ }
}

// ---------------------------------------------------------------------------------------------- one run
async function oneRun({ variant, fixture, throttle = 1, port, rules, profile = false, sampler }) {
  const F = FIXTURES[fixture];
  const url = `http://${F.host}:${port}${F.path}`;
  const t0 = Date.now();
  const h = await launch(variant === "bowser-stats" ? "bowser" : variant, rules);
  const rec = { variant, fixture, throttle, url, ok: false };
  try {
    const { context } = h;
    let sw = null, lat = null;
    if (h.ext) {
      ({ sw } = await getServiceWorker(context, 30000));
      for (let i = 0; i < 50; i++) { if ((await sw.evaluate(() => chrome.storage.local.get("mode"))).mode) break; await sleep(100); }
      await sleep(400);
      if (variant === "ext-ocr") {
        const extId = new URL(sw.url()).host;
        const p = await context.newPage();
        await p.goto(`chrome-extension://${extId}/options.html`);
        await p.waitForSelector("#ocr");
        await p.click("#ocr-card label.name");
        await p.waitForFunction(() => document.getElementById("ocr").checked, null, { timeout: 8000 });
        await sleep(300);
        rec.ocrGranted = await sw.evaluate(() => chrome.permissions.contains({ permissions: ["offscreen"] }));
        await p.close();
        // per-request latency (queueing included) measured around the service worker's OCR handler
        await sw.evaluate(() => { if (!self.__lat) { self.__lat = []; const o = self.handleOcr; self.handleOcr = function (m) { const t = Date.now(); return o(m).then((r) => { self.__lat.push({ ms: Date.now() - t, status: r && r.status }); return r; }); }; } });
      }
      for (const p of context.pages()) if (p.url().startsWith("chrome-extension://")) await p.close().catch(() => {});
    }
    if (variant === "bowser" || variant === "bowser-stats") await context.addInitScript({ content: bowserBundle(variant === "bowser-stats") });
    await context.addInitScript({ content: PROBE });
    let page = context.pages().find((p) => !p.url().startsWith("chrome-extension://"));
    if (!page) page = await context.newPage();
    const cdp = await context.newCDPSession(page);
    await cdp.send("Performance.enable");
    await cdp.send("Runtime.enable");
    if (throttle > 1) await cdp.send("Emulation.setCPUThrottlingRate", { rate: throttle });
    if (profile) { await cdp.send("Profiler.enable"); await cdp.send("Profiler.setSamplingInterval", { interval: 100 }); await cdp.send("Profiler.start"); }
    const metricsOf = async () => Object.fromEntries((await cdp.send("Performance.getMetrics")).metrics.map((m) => [m.name, m.value]));
    const m0 = await metricsOf();

    // ---- load
    await page.goto(url, { waitUntil: "load", timeout: 60000 });
    await sleep(SETTLE_MS);
    const load = await page.evaluate(() => {
      const nav = performance.getEntriesByType("navigation")[0];
      return { loadMs: nav.loadEventEnd, dclMs: nav.domContentLoadedEventEnd, respEndMs: nav.responseEnd, fcp: window.__perf.fcp, lcp: window.__perf.lcp, now: performance.now(), shielded: document.querySelectorAll("[data-sfw]").length, held: document.querySelectorAll("[data-sfw-hold]").length, imgs: document.images.length };
    });
    const m1 = await metricsOf();
    if (!profile) await cdp.send("HeapProfiler.collectGarbage").catch(() => {});
    const heap1 = profile ? null : (await cdp.send("Runtime.getHeapUsage")).usedSize;
    const mem1 = profile ? null : memSummary(procs(h.marker));

    // ---- scroll (or churn) with rAF frame recording
    const run = await page.evaluate(([ms, speed, churn]) => window.__perf.run(ms, speed, churn ? () => window.__churn(ms) : null), [F.scrollMs, F.speed, !!F.churn]);
    const lt = await page.evaluate(() => window.__perf.lt);
    const m2 = await metricsOf();
    const end = await page.evaluate(() => ({ shielded: document.querySelectorAll("[data-sfw]").length, held: document.querySelectorAll("[data-sfw-hold]").length, imgs: document.images.length, appends: window.__appends || 0, articles: document.querySelectorAll("article").length }));
    if (profile) {
      const { profile: prof } = await cdp.send("Profiler.stop");
      rec.profile = prof; // consumed by the profile aggregator, stripped before saving
    } else {
      await cdp.send("HeapProfiler.collectGarbage").catch(() => {});
    }
    const heap2 = profile ? null : (await cdp.send("Runtime.getHeapUsage")).usedSize;
    const mem2 = profile ? null : memSummary(procs(h.marker));

    if (variant === "bowser-stats") rec.engineStats = await page.evaluate(() => { try { const s = globalThis.__sfwctl.stats(); delete s.trace; delete s.slow; delete s.ocrLog; return s; } catch (e) { return { error: String(e) }; } });
    if (variant === "ext-ocr") {
      // let the OCR queue drain for at most 20 s more so latency statistics are not censored to the scroll window, then read stats
      const deadline = Date.now() + 20000;
      while (Date.now() < deadline) { const n = await page.evaluate(() => document.querySelectorAll("[data-sfw-hold]").length).catch(() => 0); if (!n) break; await sleep(1000); }
      try { lat = await sw.evaluate(() => self.__lat.slice()); rec.ocrStats = await sw.evaluate(() => chrome.runtime.sendMessage({ t: "ocr-stats" })); } catch (e) { rec.ocrError = String(e).slice(0, 120); }
      rec.memAfterOcr = memSummary(procs(h.marker));
      if (lat) { const ms = lat.map((x) => x.ms); rec.ocrLatencyMs = { n: ms.length, p50: quant(ms, 0.5), p95: quant(ms, 0.95), max: Math.max(...ms, 0), statuses: lat.reduce((a, x) => ((a[x.status] = (a[x.status] || 0) + 1), a), {}) }; }
      rec.heldAfterDrain = await page.evaluate(() => document.querySelectorAll("[data-sfw-hold]").length).catch(() => null);
    }

    // ---- derive
    const block = (arr, a, b) => arr.filter(([s]) => s >= a && s < b).reduce((t, [, d]) => t + Math.max(0, d - 50), 0);
    const count = (arr, a, b) => arr.filter(([s, d]) => s >= a && s < b && d > 50).length;
    const iv = run.iv.slice(1); // drop the first interval (rAF start-up)
    const pct = (th) => (iv.length ? (100 * iv.filter((x) => x > th).length) / iv.length : null);
    Object.assign(rec, {
      ok: true,
      load: { loadMs: load.loadMs, dclMs: load.dclMs, fcp: load.fcp, lcp: load.lcp },
      lt: { loadBlockMs: block(lt, 0, run.start), loadTasks: count(lt, 0, run.start), loadMaxMs: Math.max(0, ...lt.filter(([s]) => s < run.start).map(([, d]) => d)), scrollBlockMs: block(lt, run.start, run.end + 100), scrollTasks: count(lt, run.start, run.end + 100), scrollMaxMs: Math.max(0, ...lt.filter(([s]) => s >= run.start && s < run.end + 100).map(([, d]) => d)) },
      cdp: {
        loadTaskMs: (m1.TaskDuration - m0.TaskDuration) * 1000, loadScriptMs: (m1.ScriptDuration - m0.ScriptDuration) * 1000, loadLayoutMs: (m1.LayoutDuration - m0.LayoutDuration) * 1000, loadStyleMs: (m1.RecalcStyleDuration - m0.RecalcStyleDuration) * 1000,
        scrollTaskMs: (m2.TaskDuration - m1.TaskDuration) * 1000, scrollScriptMs: (m2.ScriptDuration - m1.ScriptDuration) * 1000, scrollLayoutMs: (m2.LayoutDuration - m1.LayoutDuration) * 1000, scrollStyleMs: (m2.RecalcStyleDuration - m1.RecalcStyleDuration) * 1000,
        jsHeapUsedMB: MB(m2.JSHeapUsedSize), nodes: m2.Nodes,
      },
      frames: { n: iv.length, p50: quant(iv, 0.5), p95: quant(iv, 0.95), p99: quant(iv, 0.99), over16_7: pct(16.7), over20: pct(20), over33: pct(33.4), max: iv.length ? Math.max(...iv) : null },
      heapAfterLoadMB: heap1 == null ? null : MB(heap1), heapEndMB: heap2 == null ? null : MB(heap2),
      memLoad: mem1, memEnd: mem2,
      activity: { shieldedAfterLoad: load.shielded, heldAfterLoad: load.held, shieldedEnd: end.shielded, heldEnd: end.held, imgs: end.imgs, appends: end.appends, articles: end.articles, scrollY: run.y, docH: run.docH },
    });
  } catch (e) {
    rec.error = String((e && e.stack) || e).split("\n").slice(0, 3).join(" | ");
  } finally {
    await closeAll(h);
  }
  rec.t0 = t0; rec.t1 = Date.now();
  rec.hostCpuPct = sampler ? r2(sampler.mean(t0, rec.t1)) : null;
  return rec;
}

// ---------------------------------------------------------------------------------------------- CDP profile aggregation
function aggregateProfiles(profiles, scriptText, urlTest) {
  const modOf = moduleMap(scriptText);
  const by = new Map(); let total = 0, engine = 0, idle = 0;
  for (const prof of profiles) {
    const nodes = new Map(prof.nodes.map((n) => [n.id, n]));
    const self = new Map();
    prof.samples.forEach((id, i) => { const dt = (prof.timeDeltas[i + 1] ?? prof.timeDeltas[i] ?? 0) / 1000; self.set(id, (self.get(id) || 0) + dt); });
    for (const [id, ms] of self) {
      const n = nodes.get(id); const cf = n.callFrame;
      if (cf.functionName === "(idle)") { idle += ms; continue; }
      total += ms;
      if (!urlTest(cf.url || "")) continue;
      engine += ms;
      const key = `${modOf(cf.lineNumber)} :: ${cf.functionName || "(anonymous)"} @${cf.lineNumber + 1}:${cf.columnNumber + 1}`;
      by.set(key, (by.get(key) || 0) + ms);
    }
  }
  const n = profiles.length;
  const rows = [...by].map(([k, ms]) => ({ fn: k, selfMsPerRun: ms / n })).sort((a, b) => b.selfMsPerRun - a.selfMsPerRun);
  const byModule = {};
  for (const r of rows) { const m = r.fn.split(" :: ")[0]; byModule[m] = (byModule[m] || 0) + r.selfMsPerRun; }
  return { runs: n, totalSelfMsPerRun: total / n, engineSelfMsPerRun: engine / n, idleMsPerRun: idle / n, byModule, top: rows.slice(0, 15) };
}

// ---------------------------------------------------------------------------------------------- summary
const METRICS = {
  "load (nav->load) ms": (r) => r.load.loadMs,
  "FCP ms": (r) => r.load.fcp,
  "LCP ms": (r) => r.load.lcp,
  "long-task blocking, load phase (sum >50ms) ms": (r) => r.lt.loadBlockMs,
  "long-task blocking, scroll phase ms": (r) => r.lt.scrollBlockMs,
  "CDP TaskDuration, load ms": (r) => r.cdp.loadTaskMs,
  "CDP ScriptDuration, load ms": (r) => r.cdp.loadScriptMs,
  "CDP TaskDuration, scroll ms": (r) => r.cdp.scrollTaskMs,
  "CDP ScriptDuration, scroll ms": (r) => r.cdp.scrollScriptMs,
  "frame p50 ms": (r) => r.frames.p50,
  "frame p95 ms": (r) => r.frames.p95,
  "frame p99 ms": (r) => r.frames.p99,
  "frames >16.7ms %": (r) => r.frames.over16_7,
  "frames >20ms %": (r) => r.frames.over20,
  "frames >33.4ms %": (r) => r.frames.over33,
  "JS heap after load (post-GC) MB": (r) => r.heapAfterLoadMB,
  "JS heap end (post-GC) MB": (r) => r.heapEndMB,
  "page renderer private MB (end)": (r) => r.memEnd && r.memEnd.pagePrivMB,
  "page renderer working set MB (end)": (r) => r.memEnd && r.memEnd.pageWsMB,
  "extension processes private MB (end)": (r) => r.memEnd && r.memEnd.extPrivMB,
  "shielded elements (end)": (r) => r.activity.shieldedEnd,
};
function summarize(runs) {
  const out = {};
  const groups = new Map();
  for (const r of runs) { if (!r.ok) continue; const k = `${r.fixture}|${r.throttle}`; if (!groups.has(k)) groups.set(k, []); groups.get(k).push(r); }
  for (const [k, rs] of groups) {
    const variants = [...new Set(rs.map((r) => r.variant))];
    out[k] = {};
    for (const [mname, fn] of Object.entries(METRICS)) {
      const row = {};
      const off = rs.filter((r) => r.variant === "off");
      for (const v of variants) {
        const mine = rs.filter((r) => r.variant === v);
        const vals = mine.map(fn);
        const cell = { n: sorted(vals).length, median: r2(med(vals)), iqr: (iqr(vals) || []).map(r2) };
        if (v !== "off") {
          // paired by repetition index: same rep, same fixture/throttle, run back to back
          const deltas = [];
          for (const m of mine) { const o = off.find((x) => x.rep === m.rep); if (o) { const a = fn(m), b = fn(o); if (a != null && b != null) deltas.push(a - b); } }
          const dmed = med(deltas), dq = iqr(deltas), offMed = med(off.map(fn)), offIqr = iqr(off.map(fn));
          const pos = deltas.filter((d) => d > 0).length, neg = deltas.filter((d) => d < 0).length;
          cell.paired = { n: deltas.length, deltaMedian: r2(dmed), deltaIqr: dq && dq.map(r2), overheadPct: offMed ? r2((100 * dmed) / offMed) : null, signsPos: pos, signsNeg: neg };
          // "clear" = the paired-delta IQR excludes zero AND |median delta| exceeds half the engine-off spread; otherwise inconclusive
          const spread = offIqr ? offIqr[1] - offIqr[0] : 0;
          cell.paired.verdict = deltas.length < 3 ? "too few pairs" : (dq[0] > 0 || dq[1] < 0) && Math.abs(dmed) > spread / 2 ? "clear" : (dq[0] > 0 || dq[1] < 0) ? "consistent sign, within noise band" : "inconclusive";
        }
        row[v] = cell;
      }
      out[k][mname] = row;
    }
  }
  return out;
}
function tables(result) {
  const lines = [];
  for (const [k, metrics] of Object.entries(result.summary)) {
    const [fx, th] = k.split("|");
    lines.push(`\n### ${FIXTURES[fx] ? FIXTURES[fx].label : fx} - CPU throttle ${th}x\n`);
    const variants = Object.keys(Object.values(metrics)[0]);
    lines.push(`| metric | ${variants.join(" | ")} |`);
    lines.push(`|---|${variants.map(() => "---").join("|")}|`);
    for (const [m, row] of Object.entries(metrics)) {
      lines.push(`| ${m} | ${variants.map((v) => { const c = row[v]; if (!c || c.median == null) return "-"; let s = `${c.median} [${c.iqr.join("-")}]`; if (c.paired) s += ` d=${c.paired.deltaMedian}${c.paired.overheadPct != null ? ` (${c.paired.overheadPct}%)` : ""} ${c.paired.signsPos}+/${c.paired.signsNeg}-`; return s; }).join(" | ")} |`);
    }
  }
  return lines.join("\n");
}

// ---------------------------------------------------------------------------------------------- main
async function main() {
  if (flag("tables")) { console.log(tables(JSON.parse(readFileSync(opt("tables"), "utf8")))); return; }
  if (!flag("no-build")) { const b = spawnSync(process.execPath, [join(ROOT, "build", "build.mjs")], { cwd: ROOT, encoding: "utf8" }); if (b.status !== 0) { console.log("build failed, retrying once in 5s\n" + (b.stderr || b.stdout)); await sleep(5000); const b2 = spawnSync(process.execPath, [join(ROOT, "build", "build.mjs")], { cwd: ROOT, encoding: "utf8" }); if (b2.status !== 0) throw new Error("build failed: " + (b2.stderr || b2.stdout)); } }
  const server = await startServer(0);
  const port = server.address().port;
  const rules = HOSTS.map((h) => `MAP ${h} 127.0.0.1`).join(", ") + ", MAP * ~NOTFOUND, EXCLUDE 127.0.0.1";
  const sampler = startLoadSampler();
  const result = { startedAt: new Date().toISOString(), argv, reps: REPS, settleMs: SETTLE_MS, machine: machine(), chromium: CHROMIUM, port, hostResolverRules: rules, fixtures: {}, runs: [], notes: [] };
  for (const f of fixtureNames) result.fixtures[f] = FIXTURES[f];
  const outFile = join(HERE, "results", `${result.startedAt.replace(/[:.]/g, "-")}.json`);
  mkdirSync(dirname(outFile), { recursive: true });
  const save = () => writeFileSync(outFile, JSON.stringify(result));
  const pre = sampler.samples; await sleep(5000);
  result.idleLoadBeforeStartPct = r2(pre.length ? pre.reduce((a, s) => a + s.v, 0) / pre.length : null);
  console.log(`machine: ${result.machine.cpu}; RAM ${result.machine.ramGB} GB; background CPU load before start ~${result.idleLoadBeforeStartPct}% (browsers not yet started)`);
  console.log(`output: ${outFile}`);

  // ---- main interleaved measurement: rep -> fixture -> variants (rotating order)
  const plan = [];
  for (let rep = 0; rep < REPS; rep++) for (const fixture of fixtureNames) {
    const base = variantNames.filter((v) => ALL_VARIANTS.includes(v));
    const rot = base.map((_, i) => base[(i + rep) % base.length]);
    for (const variant of rot) plan.push({ rep, fixture, variant, throttle: 1 });
  }
  // throttled feed runs (4x, 6x): off / ext / bowser, interleaved, same repetition count
  if (fixtureNames.includes("feed")) for (const th of THROTTLES) for (let rep = 0; rep < REPS; rep++) {
    const base = THROTTLE_VARIANTS.filter((v) => variantNames.includes(v));
    for (let i = 0; i < base.length; i++) plan.push({ rep, fixture: "feed", variant: base[(i + rep) % base.length], throttle: th });
  }
  // diagnostic: the engine's own batch timings on the feed (patched bowser bundle), interleaved with off
  if (!flag("no-stats") && fixtureNames.includes("feed")) for (let rep = 0; rep < REPS; rep++) { plan.push({ rep, fixture: "feed", variant: "bowser-stats", throttle: 1 }); }
  // shuffle blocks lightly? No: order is deterministic and documented; the rotation above balances position effects.
  console.log(`${plan.length} runs planned`);
  let i = 0;
  for (const p of plan) {
    i++;
    const rec = await oneRun({ ...p, port, rules, sampler });
    rec.rep = p.rep;
    result.runs.push(rec);
    console.log(`[${i}/${plan.length}] rep${p.rep} ${p.fixture} ${p.variant} x${p.throttle}: ${rec.ok ? `load=${r2(rec.load.loadMs)}ms block=${r2(rec.lt.loadBlockMs)}+${r2(rec.lt.scrollBlockMs)}ms p95=${r2(rec.frames.p95)}ms priv=${rec.memEnd && r2(rec.memEnd.pagePrivMB)}MB shielded=${rec.activity.shieldedEnd} cpu=${rec.hostCpuPct}%` : "ERROR " + rec.error}`);
    save();
  }

  // ---- CDP profiles (engine functions only); separate runs because the profiler itself adds overhead
  result.profiles = {};
  if (!flag("no-profile") && fixtureNames.includes("feed")) {
    const bundleText = bowserBundle(false);
    const extText = readFileSync(join(DIST_CHROME, "content.js"), "utf8");
    for (const [variant, text, urlTest] of [["bowser", bundleText, (u) => u.includes("sfw-bowser-bundle.js")], ["ext", extText, (u) => /chrome-extension:\/\/.*\/content\.js/.test(u)]]) {
      const profs = [];
      for (let rep = 0; rep < 3; rep++) { const rec = await oneRun({ variant, fixture: "feed", throttle: 1, port, rules, profile: true, sampler }); if (rec.profile) profs.push(rec.profile); else console.log("profile run failed", rec.error); }
      if (profs.length) result.profiles[variant] = aggregateProfiles(profs, text, urlTest);
      console.log(`profile ${variant}:`, profs.length ? JSON.stringify({ engineSelfMsPerRun: r2(result.profiles[variant].engineSelfMsPerRun), totalSelfMsPerRun: r2(result.profiles[variant].totalSelfMsPerRun), top: result.profiles[variant].top.slice(0, 5) }) : "none");
      save();
    }
  }
  sampler.stop();
  const all = sampler.samples.map((s) => s.v);
  result.hostCpuPct = { mean: r2(all.reduce((a, b) => a + b, 0) / (all.length || 1)), median: r2(med(all)), p10: r2(quant(all, 0.1)), p90: r2(quant(all, 0.9)), max: r2(Math.max(...all, 0)), n: all.length };
  result.summary = summarize(result.runs);
  result.finishedAt = new Date().toISOString();
  save();
  server.close();
  console.log(tables(result));
  console.log(`\nhost CPU during whole run: ${JSON.stringify(result.hostCpuPct)}\nresults: ${outFile}`);
  process.exit(0);
}
main().catch((e) => { console.error(e); process.exit(1); });
