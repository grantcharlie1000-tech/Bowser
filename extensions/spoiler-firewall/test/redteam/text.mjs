// Text-evasion red team: corpus-adversarial.json against SFW.classify.text + SFW.policy.decide (dist/node/engine.cjs).
// Positives are gated in Strict with the ctx declared per entry; negatives are gated in Strict under the launch-window
// prior (what the runtime passes on a high-risk host such as X/Reddit/YouTube during the 8 weeks around launch). The
// same negatives are also measured cold (no prior). Tiers "extreme" (positives) and "arguable" (negatives) are reported,
// never gating.
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const SFW = createRequire(import.meta.url)(join(HERE, "..", "..", "dist", "node", "engine.cjs"));

export function loadCorpus() {
  execFileSync(process.execPath, [join(HERE, "build-corpus.mjs")], { stdio: "ignore" });
  return JSON.parse(readFileSync(join(HERE, "corpus-adversarial.json"), "utf8"));
}

function decide(text, ctx, mode) {
  let r;
  try { r = SFW.classify.text(text, Object.assign({ hot: 0, source: "text" }, ctx)); } catch (e) { return { d: "throw", err: String(e && e.message), level: "?" }; }
  return { d: SFW.policy.decide(r, { mode, hot: ctx.hot || 0, kind: "item" }), level: r.level, flags: r.flags };
}

export function runText({ log = console.log } = {}) {
  const corpus = loadCorpus();
  const rows = [];
  const byCat = {};
  const cat = (c) => (byCat[c] ||= { pos: 0, miss: 0, neg: 0, fp: 0, fpCold: 0, extremeTotal: 0, extremeMiss: 0, arguableTotal: 0, arguableFp: 0 });
  const misses = [], fps = [], extremeMisses = [], arguableFps = [];
  const modes = { lockdown: { pos: 0, miss: 0, neg: 0, fp: 0 }, strict: { pos: 0, miss: 0, neg: 0, fp: 0 }, balanced: { pos: 0, miss: 0, neg: 0, fp: 0 } };
  const t0 = Date.now();
  let maxMs = 0, throws = 0;
  for (const e of corpus.entries) {
    const c = cat(e.cat);
    const t1 = performance.now();
    if (e.expect === "shield") {
      const s = decide(e.text, e.ctx, "strict");
      maxMs = Math.max(maxMs, performance.now() - t1);
      if (s.d === "throw") throws++;
      const hit = s.d === "shield";
      if (e.tier === "extreme") { c.extremeTotal++; if (!hit) { c.extremeMiss++; extremeMisses.push({ id: e.id, cat: e.cat, text: e.text, level: s.level }); } continue; }
      c.pos++; modes.strict.pos++; if (!hit) { c.miss++; modes.strict.miss++; misses.push({ id: e.id, cat: e.cat, text: e.text, ctx: e.ctx, level: s.level, flags: s.flags }); }
      for (const m of ["lockdown", "balanced"]) { const x = decide(e.text, e.ctx, m); modes[m].pos++; if (x.d !== "shield") modes[m].miss++; }
    } else {
      const lw = Object.assign({}, e.ctx, { launchWindow: true });
      const s = decide(e.text, lw, "strict");
      const cold = decide(e.text, e.ctx, "strict");
      maxMs = Math.max(maxMs, performance.now() - t1);
      if (s.d === "throw") throws++;
      const hit = s.d === "shield";
      if (e.tier === "arguable") { c.arguableTotal++; if (hit) { c.arguableFp++; arguableFps.push({ id: e.id, cat: e.cat, text: e.text, level: s.level }); } continue; }
      c.neg++; modes.strict.neg++; if (cold.d === "shield") c.fpCold++;
      if (hit) { c.fp++; modes.strict.fp++; fps.push({ id: e.id, cat: e.cat, text: e.text, level: s.level, coldAlsoShields: cold.d === "shield" }); }
      for (const m of ["lockdown", "balanced"]) { const x = decide(e.text, lw, m); modes[m].neg++; if (x.d === "shield") modes[m].fp++; }
    }
  }
  const pct = (a, b) => (b ? (100 * a / b).toFixed(1) + "%" : "-");
  log("\n== TEXT EVASION (strict) ==");
  log("category".padEnd(30) + "recall".padEnd(18) + "FP(launch)".padEnd(16) + "FP(cold)");
  for (const k of Object.keys(byCat)) {
    const c = byCat[k];
    log(k.padEnd(30) + (c.pos ? `${pct(c.pos - c.miss, c.pos)} (${c.pos - c.miss}/${c.pos})` : c.extremeTotal ? `extreme ${c.extremeTotal - c.extremeMiss}/${c.extremeTotal}` : "-").padEnd(18) + (c.neg ? `${pct(c.fp, c.neg)} (${c.fp}/${c.neg})` : c.arguableTotal ? `arguable ${c.arguableFp}/${c.arguableTotal}` : "-").padEnd(16) + (c.neg ? `${pct(c.fpCold, c.neg)}` : ""));
  }
  const gatedPos = modes.strict.pos, gatedNeg = modes.strict.neg;
  log(`\nSTRICT plausible-positive recall ${modes.strict.pos - modes.strict.miss}/${gatedPos} (${pct(gatedPos - modes.strict.miss, gatedPos)}); clear-negative FP(launch) ${modes.strict.fp}/${gatedNeg} (${pct(modes.strict.fp, gatedNeg)}); extreme-tier misses ${extremeMisses.length}/${corpus.entries.filter((e) => e.tier === "extreme").length}; arguable FPs ${arguableFps.length}`);
  log(`lockdown recall ${pct(modes.lockdown.pos - modes.lockdown.miss, modes.lockdown.pos)} FP ${pct(modes.lockdown.fp, modes.lockdown.neg)} | balanced recall ${pct(modes.balanced.pos - modes.balanced.miss, modes.balanced.pos)} FP ${pct(modes.balanced.fp, modes.balanced.neg)} | worst single classify ${maxMs.toFixed(1)} ms | throws ${throws}`);
  return {
    counts: corpus.counts, byCat, modes, strict: { positives: gatedPos, misses: misses.length, negatives: gatedNeg, falsePositives: fps.length },
    extremeMisses: extremeMisses.length, extremeTotal: corpus.entries.filter((e) => e.tier === "extreme").length, arguableFps: arguableFps.length, throws, maxClassifyMs: maxMs,
    missList: misses, fpList: fps, extremeMissList: extremeMisses, arguableFpList: arguableFps, elapsedMs: Date.now() - t0,
  };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const r = runText();
  console.log("\nMISSES (plausible positives not shielded in strict):");
  for (const m of r.missList) console.log(`  ${m.id} [${m.cat}] (${m.level}) ${JSON.stringify(m.text).slice(0, 110)}`);
  console.log("\nFALSE POSITIVES (clear negatives shielded in strict, launch-window prior):");
  for (const m of r.fpList) console.log(`  ${m.id} [${m.cat}] (${m.level}${m.coldAlsoShields ? ", cold too" : ""}) ${JSON.stringify(m.text).slice(0, 110)}`);
}
