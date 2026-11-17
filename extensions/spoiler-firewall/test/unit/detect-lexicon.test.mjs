// lexicon.extend mutates the process-wide lexicon; this file therefore runs in its own process
// (node --test runs each file separately) and keeps all extend assertions together.
import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const SFW = createRequire(import.meta.url)("../../dist/node/engine.cjs");
const { lexicon: L, classify: C } = SFW;

test("lexicon is versioned data compiled lazily through the normaliser", () => {
  assert.match(L.version, /^\d{4}\.\d{2}\.\d{2}-\d+/);
  const c = L.compile();
  assert.equal(L.compile(), c, "compiled lexicon is cached");
  assert.ok(c.byCat.franchise.length > 150);
  assert.ok(c.byCat.entity.length >= 15);
  // lexicon passes through the same fold: homoglyph/accents/fullwidth forms of the same term compile alike
  assert.equal(SFW.normalize.term("Dre'Quan Priest").c, SFW.normalize.term("DRE’QUAN priest").c);
  // CJK terms are substring terms; Latin terms are token aligned
  assert.equal(SFW.normalize.term("ネタバレ").sub, true);
  assert.equal(SFW.normalize.term("spoiler").sub, false);
});

test("extend: adds terms, tolerates junk, and validates input", () => {
  assert.equal(C.text("xyzzyplugh is wild").level, "none");
  const r = L.extend({ version: "2026.11.18-1", add: { franchise: ["xyzzy plugh"], cueStrong: ["kerfuffle"] } });
  assert.equal(r.ok, true);
  assert.equal(r.added, 2);
  assert.ok(L.version.endsWith("+2026.11.18-1"));
  assert.equal(C.text("xyzzy plugh is wild").flags.franchise, true);
  assert.equal(C.text("GTA 6 kerfuffle").level, "spoiler");
  for (const bad of [null, undefined, 5, "str", [], { add: "x" }, { add: { franchise: [5, {}, null, "", "a"] } }]) {
    assert.doesNotThrow(() => L.extend(bad));
  }
  assert.equal(L.extend({ add: { franchise: ["x".repeat(200)] } }).added, 0);
  assert.equal(L.extend({ add: { franchise: ["bad\u0000term"] } }).added, 0);
  assert.equal(L.extend({ add: { constructor: ["x"], __proto__: ["y"] } }).added, 0);
});

test("extend cannot loosen: no negatives/safe/anti additions, no removal, no demotion", () => {
  const before = C.text("Saint Lucia dies at the end"); // negative suppresses the weak name
  assert.equal(before.flags.entityWeak, false);
  // attempts to add loosening categories are ignored
  const r = L.extend({ add: { negative: ["lucia caminos"], safe: ["gta 6"], anti: ["ending"], fuzzyStop: ["luciacaminos"] } });
  assert.equal(r.added, 0);
  assert.equal(C.text("Lucia Caminos dies at the end").level, "spoiler");
  assert.equal(C.text("GTA 6 ending").level, "spoiler");
  // demoting an existing strong term via raise is rejected
  const d = L.extend({ raise: { "lucia caminos": "entityWeak", "gta 6": "entityWeak", "spoiler": "cueWeak" } });
  assert.equal(d.added, 0);
  assert.equal(C.text("Lucia Caminos").flags.entity, true);
  assert.equal(C.text("GTA 6").flags.franchise, true);
  assert.equal(C.text("GTA 6 spoiler").flags.cue, true);
  // raising a weak term to strong works (mention rather than a risk)
  assert.equal(C.text("Duval spotted").flags.entity, false);
  const up = L.extend({ raise: { duval: "entityStrong" } });
  assert.equal(up.added, 1);
  assert.equal(C.text("Duval spotted").flags.entity, true);
  // raising something already at/above the target is a no-op
  assert.equal(L.extend({ raise: { duval: "entityStrong" } }).added, 0);
  // existing detections are unchanged by additions
  assert.equal(C.text("Jason Voorhees dies at the end").flags.entityWeak, false);
});

test("extend is bounded", () => {
  const many = Array.from({ length: 6000 }, (_, i) => "term" + i + "zz");
  const r = L.extend({ add: { cueWeak: many } });
  assert.equal(r.ok, false);
  assert.equal(r.added, 0);
  const huge = { add: { cueWeak: ["a".repeat(60)] }, pad: "x".repeat(300000) };
  assert.equal(L.extend(huge).ok, false);
});
