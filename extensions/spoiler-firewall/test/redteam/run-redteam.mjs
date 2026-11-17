// Red-team runner: builds the engine, runs every attack suite, prints a summary table, writes results.json and exits
// non-zero when any suite has a failing (non-"limit", non-"arguable", non-"extreme") case.
//   node test/redteam/run-redteam.mjs [--only=text,fp,zf,reveal,hostile,perf] [--zf-only=id,id]
// Headless only (chrome-headless-shell), muted, below-normal priority; no OS input. Reuses test/e2e harness read-only.
import os from "node:os";
import { execFileSync } from "node:child_process";
import { writeFileSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

try { os.setPriority(process.pid, os.constants.priority.PRIORITY_BELOW_NORMAL); } catch (e) { /* ignore */ }
const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..", "..");
const arg = (k) => { const a = process.argv.find((x) => x.startsWith(`--${k}=`)); return a ? a.split("=")[1] : null; };
const only = arg("only") ? arg("only").split(",") : ["text", "fp", "zf", "reveal", "hostile", "perf"];
const run = (k) => only.includes(k);

// Rebuild first (other agents are editing engine/hosts concurrently; retry once if the build is momentarily broken).
for (let i = 0; i < 3; i++) {
  try { execFileSync(process.execPath, [join(ROOT, "build", "build.mjs")], { stdio: "pipe", cwd: ROOT }); break; } catch (e) {
    if (i === 2) { console.error("engine build failed 3 times:\n" + String(e.stderr || e.message).slice(0, 600)); process.exit(2); }
    await new Promise((r) => setTimeout(r, 15000));
  }
}

let prior = null;
if (arg("only")) { try { prior = JSON.parse(readFileSync(join(HERE, "results.json"), "utf8")); } catch (e) { /* none */ } }
const results = { startedAt: new Date().toISOString(), suites: prior ? prior.suites : {}, mergedFromPrior: !!prior };
const table = [];
const sevOf = (s) => s || "medium";

if (run("text")) {
  console.log("\n######## 1. TEXT EVASION ########");
  const { runText } = await import("./text.mjs");
  const r = runText();
  results.suites.text = r;
  table.push(["text: plausible positives missed (strict)", r.strict.misses, r.strict.positives, r.strict.misses > 0]);
  table.push(["text: clear negatives shielded (strict, launch prior)", r.strict.falsePositives, r.strict.negatives, r.strict.falsePositives > 0]);
  table.push(["text: extreme-tier positives missed (info)", r.extremeMisses, r.extremeTotal, false]);
  table.push(["text: arguable negatives shielded (info)", r.arguableFps, r.counts.negativesArguable, false]);
}
if (run("fp")) {
  console.log("\n######## 4. FALSE-POSITIVE STORM ########");
  const { runFpStorm, runFpStormBrowser } = await import("./fpstorm.mjs");
  const a = runFpStorm();
  console.log("  browser warm-feed cascade (x.com adapter):");
  const b = await runFpStormBrowser();
  results.suites.fpStorm = { node: a, browser: b };
  table.push(["fp storm: ordinary units shielded, strict (launch)", a.totals.strictLaunch, a.totals.units, a.strictLaunchRatePct > 1]);
  table.push(["fp storm: ordinary units shielded, lockdown (info)", a.totals.lockdownLaunch, a.totals.units, false]);
  const casc = b.filter((x) => x.ordinaryShielded > 0);
  table.push(["fp storm: warm-feed cascade (ordinary posts shielded)", casc.reduce((s, x) => s + x.ordinaryShielded, 0), b.reduce((s, x) => s + x.ordinary, 0), casc.length > 0]);
}
if (run("zf")) {
  console.log("\n######## 2. ZERO-FLASH / DOM ATTACKS (frame-exact) ########");
  const { runZeroFlash } = await import("./zf.mjs");
  const rows = await runZeroFlash({ only: arg("zf-only") ? arg("zf-only").split(",") : null });
  results.suites.zeroFlash = rows;
  const must = rows.filter((r) => r.scope === "must");
  const lim = rows.filter((r) => r.scope === "limit");
  table.push(["zero-flash: must-fixtures leaking", must.filter((r) => r.leaked).length, must.length, must.some((r) => r.leaked)]);
  table.push(["zero-flash: limit-fixtures leaking (by-design/page-author, info)", lim.filter((r) => r.leaked).length, lim.length, false]);
  table.push(["zero-flash: invalid fixtures (control shows no pixels)", rows.filter((r) => !r.fixtureValid).length, rows.length, rows.some((r) => !r.fixtureValid)]);
  table.push(["zero-flash: engine errors on fixtures", rows.filter((r) => r.engineErrors).length, rows.length, rows.some((r) => r.engineErrors)]);
}
if (run("reveal")) {
  console.log("\n######## 3. REVEAL SEMANTICS ########");
  const { runReveal } = await import("./reveal.mjs");
  const rows = await runReveal();
  results.suites.reveal = rows;
  const bad = rows.filter((r) => !r.ok && r.severity !== "low");
  table.push(["reveal: failing tests (critical/high)", bad.length, rows.length, bad.length > 0]);
  table.push(["reveal: failing tests (low, info)", rows.filter((r) => !r.ok && r.severity === "low").length, rows.length, false]);
}
if (run("hostile")) {
  console.log("\n######## 5. HOSTILE MAIN WORLD (Bowser bundle) ########");
  const { runHostile } = await import("./hostile.mjs");
  const rows = await runHostile();
  results.suites.hostile = rows;
  // sabotage scenarios (RegExp.exec -> null, JSON/Object.keys/Symbol.iterator broken, ...) are not what real polyfill libraries do; reported, not gating.
  const bad = rows.filter((r) => !r.ok && !r.sabotage);
  table.push(["hostile: failing polyfill-style scenario runs (pre+post)", bad.length, rows.filter((r) => !r.sabotage).length, bad.length > 0]);
  table.push(["hostile: polyfill-style runs with UNSHIELDED pixels (critical)", bad.filter((r) => r.severity === "critical").length, rows.filter((r) => !r.sabotage).length, bad.some((r) => r.severity === "critical")]);
  table.push(["hostile: sabotage runs failing (info)", rows.filter((r) => !r.ok && r.sabotage).length, rows.filter((r) => r.sabotage).length, false]);
}
if (run("perf")) {
  console.log("\n######## 6. PERFORMANCE ABUSE ########");
  const { runPerfNode, runPerfBrowser } = await import("./perf.mjs");
  const a = runPerfNode();
  const b = await runPerfBrowser();
  results.suites.perf = { node: a, browser: b };
  table.push(["perf (node): pathological texts > 100 ms", a.filter((x) => x.over).length, a.length, a.some((x) => x.over)]);
  table.push(["perf (browser): batches blocking > 100 ms", b.filter((x) => x.over).length, b.length, b.some((x) => x.over)]);
}

results.finishedAt = new Date().toISOString();
results.summary = (prior && prior.summary ? prior.summary.filter((p) => !table.some((t) => t[0] === p.name)) : []).concat(table.map(([name, bad, total, fail]) => ({ name, bad, total, fail })));
results.failed = results.summary.some((t) => t.fail);
writeFileSync(join(HERE, "results.json"), JSON.stringify(results, null, 1));
console.log("\n==================== RED TEAM SUMMARY ====================");
for (const [name, bad, total, fail] of table) console.log(`${fail ? "FAIL" : "ok  "}  ${name.padEnd(66)} ${String(bad).padStart(5)} / ${total}`);
console.log(`\nresults: ${join(HERE, "results.json")}`);
process.exit(results.failed ? 1 : 0);
