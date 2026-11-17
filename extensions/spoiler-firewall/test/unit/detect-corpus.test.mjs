// Fast regression gate over a fixed subset of the corpus (the full run is test/corpus/run-corpus.mjs).
import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { generate } from "../corpus/generate.mjs";

const SFW = createRequire(import.meta.url)("../../dist/node/engine.cjs");
const decide = (ex) => {
  const ctx = ex.ctx || {};
  const r = SFW.classify.text(ex.text, { hot: ctx.hot || 0, reply: ctx.reply, launchWindow: ctx.launchWindow });
  return SFW.policy.decideAll(r, { hot: ctx.hot || 0, kind: "item" });
};

const examples = generate(); // deterministic (fixed seed)

test("generator is deterministic and large enough", () => {
  const again = generate();
  assert.equal(again.length, examples.length);
  assert.equal(again[1234].text, examples[1234].text);
  assert.ok(examples.filter((e) => e.group === "pos").length >= 5000);
  assert.ok(examples.filter((e) => e.group === "neg").length >= 3000);
  assert.ok(examples.some((e) => e.heldOut) && examples.some((e) => !e.heldOut));
});

test("synthetic positives (every 7th): Strict recall >= 99%, Lockdown >= 99%", () => {
  const pos = examples.filter((e, i) => e.group === "pos" && i % 7 === 0);
  assert.ok(pos.length > 700);
  let strict = 0, lock = 0;
  for (const e of pos) {
    const d = decide(e);
    if (d.strict === "shield") strict++;
    if (d.lockdown === "shield") lock++;
  }
  assert.ok(strict / pos.length >= 0.99, `strict recall ${strict}/${pos.length}`);
  assert.ok(lock / pos.length >= 0.99, `lockdown recall ${lock}/${pos.length}`);
});

test("synthetic negatives (every 5th): Strict and Balanced never shield; labelled Lockdown shields match", () => {
  const neg = examples.filter((e, i) => e.group === "neg" && i % 5 === 0);
  assert.ok(neg.length > 500);
  let bad = 0, badLock = 0, lockLabelled = 0;
  for (const e of neg) {
    const d = decide(e);
    if (e.expect.strict === "clear" && d.strict === "shield") bad++;
    if (e.expect.strict === "shield" && d.strict !== "shield") bad++;
    if (e.expect.balanced === "clear" && d.balanced === "shield") bad++;
    if (e.expect.lockdown) { lockLabelled++; if (d.lockdown !== e.expect.lockdown) badLock++; }
  }
  assert.equal(bad, 0, "strict/balanced label mismatches");
  assert.ok(badLock / lockLabelled <= 0.12, `lockdown mismatches ${badLock}/${lockLabelled}`);
});

test("real negatives sample (English Wikipedia paragraphs): Strict FP <= 1%", () => {
  const j = JSON.parse(readFileSync(new URL("../corpus/real-negatives/data/en.json", import.meta.url), "utf8"));
  const MENTION = /\b(?:gta|grand\s+theft\s+auto)\s*:?\s*(?:6|vi|six)\b|\bleonida\b|lucia\s+caminos|jason\s+duval|vice\s+city\s+2\b/i;
  let n = 0, fp = 0;
  for (const a of j.articles) {
    for (const p of a.paragraphs) {
      if (MENTION.test(p)) continue;
      n++;
      const r = SFW.classify.text(p);
      if (SFW.policy.decide(r, { mode: "strict" }) === "shield") fp++;
    }
  }
  assert.ok(n > 1000, "enough real paragraphs: " + n);
  assert.ok(fp / n <= 0.01, `strict FP ${fp}/${n}`);
});
