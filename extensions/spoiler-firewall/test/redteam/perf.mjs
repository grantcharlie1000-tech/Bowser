// Performance abuse. (1) Node: pathological text straight into SFW.classify.text (regex backtracking, 1MB homoglyph / zero-width /
// combining-mark floods). (2) Browser: giant/deep DOM insertions; main-thread blocking per batch is measured as
// (time from the inserting script start to the next macrotask) with the engine ON minus the same page with the engine OFF,
// plus the engine's own batch stats. Anything > 100 ms is a finding.
import { createRequire } from "node:module";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { instrumentedContext } from "../e2e/lib.mjs";
import { chromium } from "../e2e/node_modules/playwright/index.mjs";
import { CHROMIUM, BASE_ARGS } from "../e2e/env.mjs";
// NOTE: the brief asked for 2000 nested levels; Chromium itself (engine off, no scripts injected) crashes the renderer at depth 2000, so 1000 is the deepest testable here.
// chrome-headless-shell crashes on DOM depth >= 200 even with the engine off (harness limit), so perf uses the full Chromium build, still headless + muted.

const HERE = dirname(fileURLToPath(import.meta.url));
const SFW = createRequire(import.meta.url)(join(HERE, "..", "..", "dist", "node", "engine.cjs"));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const rep = (s, n) => s.repeat(n);
const LIMIT = 100;

export function runPerfNode({ log = console.log } = {}) {
  const cases = [
    ["gta-gta repeated 200KB", rep("g t a ", 33000)],
    ["'gta' repeated 1MB no space", rep("gta", 350000)],
    ["'GTA ' repeated 400KB", rep("GTA ", 100000)],
    ["'GTA 6 ' repeated 600KB", rep("GTA 6 ", 100000)],
    ["g.t.a. repeated 300KB", rep("g.t.a.", 50000)],
    ["spoiler+GTA 6 x 50k", rep("spoiler GTA 6 ending ", 50000)],
    ["1MB Cyrillic homoglyph soup", rep("аеорсху ", 130000)],
    ["1MB homoglyph 'gta 6' variants", rep("gтa 6 ", 170000)],
    ["1MB zero-width joiners", rep("‍", 1000000)],
    ["1MB zero-width between letters of gta", rep("g​t​a​", 170000)],
    ["1 letter + 100k combining marks", "g" + rep("́҉̸", 100000)],
    ["100k letters each with 10 marks (1MB)", rep("á̂̃̄̅̆̇̈̉̊", 90000)],
    ["gta + 200k spaces + 6", "gta" + rep(" ", 200000) + "6"],
    ["gta + 200k newlines + 6 ending", "gta" + rep("\n", 200000) + "6 ending spoiler"],
    ["digits 1MB", rep("0123456789", 100000)],
    ["leet soup 1MB", rep("g7a 6 3nd1ng ", 77000)],
    ["emoji keycaps 600KB", rep("6️⃣", 100000)],
    ["10k hashtags", rep("#Gta6Ending ", 10000)],
    ["fullwidth 1MB", rep("ＧＴＡ６ ", 100000)],
    ["a*b*c wildcard soup 500KB", rep("j*s*n ", 80000)],
    ["spoiler word repeated 1MB", rep("spoiler ", 125000)],
    ["'Lucia ' repeated 600KB", rep("Lucia ", 100000)],
    ["'Jason dies ' repeated 800KB", rep("Jason dies ", 70000)],
    ["nested brackets 200k", rep("[", 100000) + rep("]", 100000)],
    ["alternating script 1MB", rep("aа", 500000)],
    ["long single token 1MB", rep("a", 1000000)],
    ["gt+a x many with tag chars", rep("g\u{e0067}t\u{e0074}a\u{e0061} ", 100000)],
    ["reversed 'ATG ' 400KB", rep("ATG ", 100000)],
    ["rot13 style 'TGN 6 ' 600KB", rep("TGN 6 ", 100000)],
    ["emoji letters 1MB", rep("🅶🆃🅰 ", 100000)],
    ["braille 600KB", rep("⠛⠞⠁ ", 150000)],
    ["bidi controls 1MB", rep("‮‬", 500000)],
    ["variation selectors 1MB", rep("a️", 500000)],
  ];
  const rows = [];
  for (const [name, text] of cases) {
    const t0 = performance.now();
    let err = null, level = null;
    try { level = SFW.classify.text(text, { hot: 0, source: "text" }).level; } catch (e) { err = String(e && e.message).slice(0, 80); }
    const ms = performance.now() - t0;
    rows.push({ name, chars: text.length, ms: +ms.toFixed(1), level, err, over: ms > LIMIT });
    log(`${ms > LIMIT ? "SLOW" : "ok  "} ${name.padEnd(42)} ${String(text.length).padStart(8)} chars ${ms.toFixed(1).padStart(9)} ms ${level || ""}${err ? " ERR " + err : ""}`);
  }
  return rows;
}

const SCEN = [
  { id: "nested-1000-levels", build: `var root=document.getElementById('h');var d=root;for(var i=0;i<1000;i++){var c=document.createElement('div');d.appendChild(c);d=c}var a=document.createElement('article');a.innerHTML='<span class="sentinel">GTA 6 ending spoiler: he dies</span>';d.appendChild(a)` },
  { id: "nested-1000-built-detached-then-attached", build: `var tp=document.createElement('div');var d=tp;for(var i=0;i<1000;i++){var c=document.createElement('div');d.appendChild(c);d=c}var a=document.createElement('article');a.textContent='GTA 6 ending spoiler: he dies';d.appendChild(a);document.getElementById('h').appendChild(tp)` },
  { id: "100k-nodes-one-task", build: `var f=document.createDocumentFragment();for(var i=0;i<100000;i++){var li=document.createElement('li');li.textContent=(i%5000===777)?'GTA 6 ending spoiler: he dies #'+i:'ordinary list item number '+i;f.appendChild(li)}document.getElementById('h').appendChild(f)` },
  { id: "100k-nodes-all-ordinary", build: `var f=document.createDocumentFragment();for(var i=0;i<100000;i++){var li=document.createElement('li');li.textContent='ordinary list item number '+i+' about the weather and sports';f.appendChild(li)}document.getElementById('h').appendChild(f)` },
  { id: "20k-articles-1-percent-spoilers", build: `var f=document.createDocumentFragment();for(var i=0;i<20000;i++){var a=document.createElement('article');a.innerHTML='<p>'+(i%100===5?'GTA 6 ending spoiler leak: Jason dies #'+i:'post '+i+' lorem ipsum dolor')+'</p><img alt="x">';f.appendChild(a)}document.getElementById('h').appendChild(f)` },
  { id: "1mb-single-text-node", build: `var t='';var w='ordinary words about the weather and the news of the day ';while(t.length<1000000)t+=w;var a=document.createElement('article');a.textContent=t+' GTA 6 ending spoiler: he dies';document.getElementById('h').appendChild(a)` },
  { id: "1mb-homoglyph-text-node", build: `var a=document.createElement('article');var t='';while(t.length<1000000)t+='аеорсху ';a.textContent=t;document.getElementById('h').appendChild(a)` },
  { id: "50k-characterdata-updates-one-task", pre: `var tn=document.createTextNode('x');var a=document.createElement('article');a.appendChild(tn);document.getElementById('h').appendChild(a);window.__tn=tn;`, build: `for(var i=0;i<50000;i++)window.__tn.data='update '+i` },
  { id: "20k-attribute-changes-one-task", pre: `for(var i=0;i<2000;i++){var a=document.createElement('article');a.textContent='post '+i;document.getElementById('h').appendChild(a)}`, build: `var as=document.querySelectorAll('#h article');for(var k=0;k<10;k++)for(var i=0;i<as.length;i++)as[i].setAttribute('title','t'+k+'-'+i)` },
  { id: "500-nested-shadow-roots", build: `var d=document.getElementById('h');for(var i=0;i<500;i++){var r=d.attachShadow({mode:'open'});var c=document.createElement('div');r.appendChild(c);d=c}var a=document.createElement('article');a.textContent='GTA 6 ending spoiler: he dies';d.appendChild(a)` },
  { id: "spoiler-flood-5000-shielded", build: `var f=document.createDocumentFragment();for(var i=0;i<5000;i++){var a=document.createElement('article');a.style.cssText='height:40px;border:1px solid #ccc';a.textContent='GTA 6 ending spoiler leak '+i+': he dies';f.appendChild(a)}document.getElementById('h').appendChild(f)` },
  { id: "table-10k-rows", build: `var t=document.createElement('table');for(var i=0;i<10000;i++){var r=t.insertRow();r.insertCell().textContent=(i%1000===3)?'GTA 6 ending spoiler: he dies':'row '+i;r.insertCell().textContent='b'}document.getElementById('h').appendChild(t)` },
  { id: "innerhtml-3mb-string", build: `var s='';for(var i=0;i<30000;i++)s+='<div class="row"><span>item '+i+'</span> <a href="/x/'+i+'">link text '+i+'</a></div>';document.getElementById('h').innerHTML=s` },
];

const HANG_MS = 45000;
const raceHang = (p, label) => Promise.race([p, sleep(HANG_MS).then(() => { throw new Error(label + ": HUNG > " + HANG_MS / 1000 + "s (main thread unresponsive)"); })]);
export async function runPerfBrowser({ log = console.log, only = null } = {}) {
  const rows = [];
  for (const sc of SCEN.filter((x) => !only || only.includes(x.id))) {
    const res = {};
    for (const enabled of [false, true]) {
      // fresh browser per case so a hung/crashed renderer cannot poison the next one
      const browser = await chromium.launch({ headless: true, executablePath: CHROMIUM, args: BASE_ARGS });
      const ctx = await instrumentedContext(browser, { config: { enabled }, probe: false });
      const page = await ctx.newPage();
      const errs = [];
      page.on("pageerror", (e) => errs.push(e.message));
      await page.setContent(`<!doctype html><html><head><meta charset="utf-8"><style>.sentinel{color:#f0f}</style></head><body><div id="h"></div></body></html>`);
      await sleep(200);
      try {
        if (sc.pre) { await raceHang(page.evaluate(sc.pre), "pre"); await sleep(900); }
        const m = await raceHang(page.evaluate(async (code) => {
          const long = [];
          try { new PerformanceObserver((l) => l.getEntries().forEach((e) => long.push(Math.round(e.duration)))).observe({ entryTypes: ["longtask"] }); } catch (e) { /* ignore */ }
          const t0 = performance.now();
          (0, eval)(code);
          const tScript = performance.now() - t0;
          await new Promise((r) => setTimeout(r, 0));
          const tFirstMacro = performance.now() - t0;
          await new Promise((r) => requestAnimationFrame(() => setTimeout(r, 0)));
          const tFrame = performance.now() - t0;
          return { tScript, tFirstMacro, tFrame, long };
        }, sc.build), "build");
        await sleep(2500);
        const after = await raceHang(page.evaluate(() => ({ s: window.__sfw && window.__sfw.ctl && window.__sfw.ctl.stats(), shields: document.querySelectorAll("[data-sfw]").length, pending: document.querySelectorAll("[data-sfw-fb=pending]").length })), "after");
        res[enabled ? "on" : "off"] = { ...m, stats: after.s ? { batches: after.s.batches, maxBatchMs: after.s.maxBatchMs, p95: after.s.p95BatchMs, errors: after.s.errors, pending: after.s.pending } : null, shields: after.shields, pendingDom: after.pending, errs: errs.slice(0, 2) };
      } catch (e) { res[enabled ? "on" : "off"] = { error: String(e.message).split("\n")[0] }; }
      await Promise.race([browser.close(), sleep(10000)]).catch(() => {});
    }
    const on = res.on || {}, off = res.off || {};
    const delta = on.tFirstMacro != null && off.tFirstMacro != null ? on.tFirstMacro - off.tFirstMacro : null;
    const frameDelta = on.tFrame != null && off.tFrame != null ? on.tFrame - off.tFrame : null;
    const maxLong = Math.max(0, ...(on.long || [0]));
    const row = { id: sc.id, engineBlockMs: delta == null ? null : +delta.toFixed(0), engineToFrameMs: frameDelta == null ? null : +frameDelta.toFixed(0), offScriptMs: off.tScript && +off.tScript.toFixed(0), onLongestLongTaskMs: maxLong, engineMaxBatchMs: on.stats && on.stats.maxBatchMs, engineP95BatchMs: on.stats && on.stats.p95, shields: on.shields, pendingLeft: on.pendingDom, error: on.error || off.error || null, onError: on.error || null, offError: off.error || null, over: (delta != null && delta > LIMIT) || maxLong > LIMIT + (off.tScript || 0) || (!!on.error && !off.error) };
    rows.push(row);
    log(`${row.over ? "SLOW" : "ok  "} ${sc.id.padEnd(42)} engine adds ${String(row.engineBlockMs).padStart(6)} ms before next task, ${String(row.engineToFrameMs).padStart(6)} ms to first frame; longest longtask(on)=${maxLong} ms; engine maxBatch=${on.stats && on.stats.maxBatchMs} p95=${on.stats && on.stats.p95} shields=${row.shields} pending=${row.pendingLeft}${row.error ? " ERR " + row.error : ""}`);
  }
  return rows;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  console.log("perf (node):"); runPerfNode();
  console.log("perf (browser):"); await runPerfBrowser();
}
