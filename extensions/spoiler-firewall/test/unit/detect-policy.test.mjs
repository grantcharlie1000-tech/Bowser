import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const SFW = createRequire(import.meta.url)("../../dist/node/engine.cjs");
const { classify: C, policy: P } = SFW;
const all = (text, ctx, opts) => P.decideAll(C.text(text, ctx), Object.assign({ hot: (ctx && ctx.hot) || 0, kind: "item" }, opts));

test("constants and decideAll shape", () => {
  assert.deepEqual(P.MODES, ["lockdown", "strict", "balanced"]);
  assert.equal(P.DEFAULT_MODE, "strict");
  const d = all("GTA 6 is coming");
  assert.deepEqual(Object.keys(d), ["lockdown", "strict", "balanced"]);
  // default mode and invalid mode behave as strict
  const r = C.text("everyone at the office keeps talking about GTA 6 all day long and I cannot concentrate on my work at all today");
  assert.equal(P.decide(r, {}), P.decide(r, { mode: "strict" }));
  assert.equal(P.decide(r, { mode: "nonsense" }), P.decide(r, { mode: "strict" }));
  assert.equal(P.decide(r), "shield");
});

test("franchise mention alone: Lockdown shield, Strict shield, Balanced clear", () => {
  const d = all("everyone at the office keeps talking about GTA 6 all day long and I cannot concentrate on my work at all today");
  assert.deepEqual(d, { lockdown: "shield", strict: "shield", balanced: "clear" });
});

test("short safe-logistics talk: Strict clears, Lockdown still shields, Balanced clears", () => {
  const d = all("GTA 6 release date and price?");
  assert.deepEqual(d, { lockdown: "shield", strict: "clear", balanced: "clear" });
  // long text, or a cue/entity, defeats the exemption
  const long = "GTA 6 release date and price? " + "waiting for launch so much, ".repeat(8);
  assert.ok(long.length > 140);
  assert.equal(all(long).strict, "shield");
  assert.equal(all("GTA 6 release date, Lucia trailer").strict, "shield");
});

test("franchise/entity + cue: every mode shields", () => {
  for (const t of ["GTA 6 ending explained", "Lucia Caminos dies in the final mission", "spoiler alert GTA 6", "[spoilers] Leonida"]) {
    assert.deepEqual(all(t), { lockdown: "shield", strict: "shield", balanced: "shield" }, t);
  }
});

test("hot context rows", () => {
  // hot + cue
  assert.deepEqual(all("he dies at the end", { hot: 1 }), { lockdown: "shield", strict: "shield", balanced: "shield" });
  // hot + weak entity
  const w = all("Jason is great", { hot: 1 });
  assert.equal(w.lockdown, "shield");
  assert.equal(w.strict, "shield");
  assert.equal(w.balanced, "clear");
  // hot=2: Lockdown shields every item; Strict/Balanced need a signal
  assert.deepEqual(all("good morning everyone", { hot: 2 }), { lockdown: "shield", strict: "clear", balanced: "clear" });
  // hot=1: nothing => clear
  assert.deepEqual(all("good morning everyone", { hot: 1 }), { lockdown: "clear", strict: "clear", balanced: "clear" });
  // media in hot context shields in all modes
  for (const m of P.MODES) assert.equal(P.decide(C.text("pic", { hot: 1 }), { mode: m, hot: 1, hasMedia: true }), "shield", m);
  for (const m of P.MODES) assert.equal(P.decide(C.text("pic", { hot: 2 }), { mode: m, hot: 2, kind: "media" }), "shield", m);
});

test("cold weak signals are not shielded in Strict/Balanced; Lockdown needs a game signal for weak names", () => {
  // M-3: a bare weak name + ordinary story word on a cold page is not GTA evidence, even in Lockdown
  assert.deepEqual(all("Jason dies at the end"), { lockdown: "clear", strict: "clear", balanced: "clear" });
  assert.deepEqual(all("Police say Lucia Santos betrayed the trust of investors"), { lockdown: "clear", strict: "clear", balanced: "clear" });
  assert.deepEqual(all("Jason Miller announced as new coach"), { lockdown: "clear", strict: "clear", balanced: "clear" });
  // with a game-register word Lockdown shields it (Strict still waits for more)
  assert.deepEqual(all("Jason dies in the last mission"), { lockdown: "shield", strict: "clear", balanced: "clear" });
  assert.deepEqual(all("Jason Voorhees dies at the end"), { lockdown: "clear", strict: "clear", balanced: "clear" });
});

test("media on a high-risk host: hold in Lockdown/Strict until OCR, metadata only in Balanced", () => {
  const r = C.text("a picture");
  const o = { hot: 0, kind: "media", hasMedia: true, highRisk: true };
  assert.equal(P.decide(r, Object.assign({ mode: "lockdown" }, o)), "hold");
  assert.equal(P.decide(r, Object.assign({ mode: "strict" }, o)), "hold");
  assert.equal(P.decide(r, Object.assign({ mode: "balanced" }, o)), "clear");
  assert.equal(P.decide(r, Object.assign({ mode: "strict", ocrDone: true }, o)), "clear");
  assert.equal(P.decide(r, Object.assign({ mode: "strict" }, o, { highRisk: false })), "clear");
  // flagged unit shields instead of holding
  assert.equal(P.decide(C.text("GTA 6 picture"), Object.assign({ mode: "balanced" }, o)), "shield");
});

test("classification failure fails closed", () => {
  for (const bad of [null, undefined, {}, { level: "none" }, { flags: {} }, "x"]) {
    for (const m of P.MODES) assert.equal(P.decide(bad, { mode: m }), "shield");
  }
});
