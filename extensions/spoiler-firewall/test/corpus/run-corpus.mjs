// Evaluates the shipped engine (dist/node/engine.cjs; build first) on the synthetic + real corpora.
//   node test/corpus/run-corpus.mjs [--fails N] [--all-fails]
// Prints recall / FP-rate per category and transform per mode, timing, writes results/latest.json.
// Exit code 1 if Strict recall (synthetic positives) < 99.5% or Strict FP rate (real negatives) > 2%.
import { createRequire } from "node:module";
import { readFileSync, readdirSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { generate } from "./generate.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const SFW = createRequire(import.meta.url)(join(here, "..", "..", "dist", "node", "engine.cjs"));
const MODES = SFW.policy.MODES;
const args = process.argv.slice(2);
const failN = args.includes("--fails") ? Number(args[args.indexOf("--fails") + 1]) : 15;
const showHeld = args.includes("--all-fails");

// Independent labeller for real text: does it mention GTA VI? (simple regex, not the engine's lexicon)
const MENTION = /(?:\b(?:gta|grand\s+theft\s+auto)\s*:?\s*(?:6|vi|six)\b|\bleonida\b|lucia\s+caminos|jason\s+duval|port\s+gellhorn|vice\s+city\s+2\b|ГТА\s*6|グラセフ\s*6|グランド・セフト・オート\s*(?:VI|6)|侠盗猎车手\s*6)/i;

const examples = generate();
const real = [];
const rdir = join(here, "real-negatives", "data");
for (const f of readdirSync(rdir).filter((x) => x.endsWith(".json"))) {
  const j = JSON.parse(readFileSync(join(rdir, f), "utf8"));
  for (const a of j.articles) {
    for (const p of a.paragraphs) {
      const sents = [p, ...p.split(/(?<=[.!?。！？])\s*/).filter((s) => s.length >= 25)];
      sents.forEach((t, i) => {
        const m = MENTION.test(t);
        real.push({ text: t, group: "real", category: "real-" + j.lang, transform: i === 0 ? "paragraph" : "sentence", tid: a.title, heldOut: false, ctx: null, mentions: m, expect: { lockdown: m ? "shield" : "clear", strict: m ? "shield" : "clear", balanced: m ? null : "clear" } });
      });
    }
  }
}

const all = examples.concat(real);
const times = [];
const rows = [];
function evalOne(ex) {
  const ctx = ex.ctx || {};
  const t0 = process.hrtime.bigint();
  const r = SFW.classify.text(ex.text, { hot: ctx.hot || 0, reply: ctx.reply, launchWindow: ctx.launchWindow, source: "text" });
  const t1 = process.hrtime.bigint();
  times.push(Number(t1 - t0) / 1e6);
  const d = SFW.policy.decideAll(r, { hot: ctx.hot || 0, kind: "item" });
  return { ex, r, d };
}
SFW.classify.text("warmup text with no anchors at all", {});
SFW.classify.text("warmup GTA 6 ending", {});
SFW.classify.stats.calls = SFW.classify.stats.rejected = 0;
for (const ex of all) rows.push(evalOne(ex));

function tally(sel, key) {
  const blank = () => { const m = {}; for (const mode of MODES) m[mode] = { pos: 0, hit: 0, neg: 0, fp: 0 }; return m; };
  const groups = {};
  for (const row of rows) {
    if (!sel(row.ex)) continue;
    const g = (groups[key(row.ex)] ||= blank());
    for (const mode of MODES) {
      const e = row.ex.expect[mode];
      if (e == null) continue;
      const got = row.d[mode] === "shield";
      if (e === "shield") { g[mode].pos++; if (got) g[mode].hit++; } else { g[mode].neg++; if (got) g[mode].fp++; }
    }
  }
  return groups;
}
const pct = (a, b) => (b ? ((100 * a) / b).toFixed(2) + "%" : "-");
function printTable(title, groups, mode) {
  console.log(`\n${title} [${mode}]  (recall = shielded/expected-shield, FP = shielded/expected-clear)`);
  for (const n of Object.keys(groups).sort()) {
    const g = groups[n][mode];
    console.log("  " + n.padEnd(34) + ("recall " + pct(g.hit, g.pos) + " (" + g.hit + "/" + g.pos + ")").padEnd(30) + "FP " + pct(g.fp, g.neg) + " (" + g.fp + "/" + g.neg + ")");
  }
}

const summary = {};
const sets = {
  "synthetic-positives": (e) => e.group === "pos",
  "synthetic-positives-heldout": (e) => e.group === "pos" && e.heldOut,
  "synthetic-positives-tuning": (e) => e.group === "pos" && !e.heldOut,
  "synthetic-negatives": (e) => e.group === "neg",
  "synthetic-negatives-heldout": (e) => e.group === "neg" && e.heldOut,
  "real-negatives": (e) => e.group === "real",
  "real-negatives-no-gta6-mention": (e) => e.group === "real" && !e.mentions,
};
for (const [name, sel] of Object.entries(sets)) summary[name] = tally(sel, () => "all").all;
console.log(`examples: ${examples.length} synthetic (${examples.filter((e) => e.group === "pos").length} pos, ${examples.filter((e) => e.group === "neg").length} neg), ${real.length} real units (${real.filter((r) => r.transform === "paragraph").length} paragraphs, ${real.filter((r) => r.mentions).length} labelled as GTA VI mentions)`);
console.log("\n== overall per mode ==");
for (const [name, g] of Object.entries(summary)) {
  console.log(name.padEnd(32) + MODES.map((m) => `${m}: recall ${pct(g[m].hit, g[m].pos)} FP ${pct(g[m].fp, g[m].neg)}`).join(" | "));
}
const byCat = tally((e) => e.group !== "real", (e) => e.group + ":" + e.category);
const byTr = tally((e) => e.group === "pos", (e) => e.transform);
const realByCat = tally((e) => e.group === "real", (e) => e.category + ":" + e.transform);
for (const m of MODES) printTable("by category (synthetic)", byCat, m);
printTable("positives by transform", byTr, "strict");
printTable("real negatives", realByCat, "strict");

const fails = [];
for (const row of rows) {
  if (row.ex.heldOut && !showHeld) continue;
  const e = row.ex.expect.strict;
  if (e && (row.d.strict === "shield") !== (e === "shield")) fails.push(row);
}
console.log(`\nStrict failures on tuning split: ${fails.length} (showing ${Math.min(failN, fails.length)})`);
for (const f of fails.slice(0, failN)) console.log(`  [${f.ex.group}/${f.ex.category}/${f.ex.transform}] expect ${f.ex.expect.strict} got ${f.d.strict} level=${f.r.level} :: ${JSON.stringify(f.ex.text.slice(0, 110))}`);

const sorted = times.slice().sort((a, b) => a - b);
const mean = times.reduce((s, x) => s + x, 0) / times.length;
const st = SFW.classify.stats;
const timing = { calls: times.length, meanMs: +mean.toFixed(4), p95Ms: +sorted[Math.floor(sorted.length * 0.95)].toFixed(4), p99Ms: +sorted[Math.floor(sorted.length * 0.99)].toFixed(4), maxMs: +sorted[sorted.length - 1].toFixed(3), prefilterRejectedPct: +((100 * st.rejected) / st.calls).toFixed(2) };
console.log("\ntiming (ms per classify.text call):", JSON.stringify(timing));

mkdirSync(join(here, "results"), { recursive: true });
writeFileSync(join(here, "results", "latest.json"), JSON.stringify({ lexicon: SFW.lexicon.version, counts: { synthetic: examples.length, real: real.length }, summary, byCategory: byCat, byTransform: byTr, realByCategory: realByCat, timing }, null, 1));

const sp = summary["synthetic-positives"].strict, rn = summary["real-negatives"].strict;
const recall = sp.hit / sp.pos, fp = rn.neg ? rn.fp / rn.neg : 0;
console.log(`\nGATE strict recall ${(recall * 100).toFixed(2)}% (need >= 99.5%), strict real FP ${(fp * 100).toFixed(2)}% (need <= 2%)`);
process.exit(recall >= 0.995 && fp <= 0.02 ? 0 : 1);
