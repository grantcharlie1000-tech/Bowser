// The build-time lexicon snapshot (engine/lexicon.js "build-time snapshot", build/build.mjs withSnapshot) must be an exact
// stand-in for compile(): same terms with the same fields, the same tries (same insertion order), the same Sets and RegExp
// sources, and therefore the same verdict for every text. Two engines are compared: the shipped one (snapshot) and the same
// bundle with the snapshot line removed (compiled from the word lists).
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { generate } from "../corpus/generate.mjs";

const ENGINE = fileURLToPath(new URL("../../dist/node/engine.cjs", import.meta.url));
const SNAP_LINE = /\nSFW\.lexicon\.setSnapshot\(.*\);\n/;
const load = (text) => { const m = { exports: {} }; new Function("module", "exports", text)(m, m.exports); return m.exports; };
const shipped = readFileSync(ENGINE, "utf8");

test("the shipped engine carries a snapshot and uses it", () => {
  assert.match(shipped, SNAP_LINE);
  const S = load(shipped);
  S.lexicon.compile();
  assert.equal(S.lexicon.fromSnapshot(), true);
});

function same(a, b, path) {
  if (a === b) return;
  assert.equal(typeof a, typeof b, path + ": type");
  if (a instanceof RegExp) { assert.ok(b instanceof RegExp, path); assert.equal(a.source, b.source, path); assert.equal(a.flags, b.flags, path); return; }
  if (a instanceof Set) { assert.ok(b instanceof Set, path); assert.deepEqual(Array.from(a), Array.from(b), path); return; }
  if (a === null || typeof a !== "object") { assert.equal(a, b, path); return; }
  assert.equal(Array.isArray(a), Array.isArray(b), path + ": array");
  assert.equal(Object.getPrototypeOf(a), Object.getPrototypeOf(b), path + ": prototype");
  const ka = Object.keys(a), kb = Object.keys(b);
  assert.deepEqual(ka, kb, path + ": keys (and their order)");
  for (const k of ka) same(a[k], b[k], path + "." + k);
}

test("the revived lexicon is identical to a fresh compile", () => {
  const A = load(shipped);
  const B = load(shipped.replace(SNAP_LINE, "\n"));
  const a = A.lexicon.compile(), b = B.lexicon.compile();
  assert.equal(A.lexicon.fromSnapshot(), true);
  assert.equal(B.lexicon.fromSnapshot(), false);
  same(a, b, "lexicon");
});

test("snapshot and compiled engines give the same verdict on the whole corpus and the real-text negatives", () => {
  const A = load(shipped);
  const B = load(shipped.replace(SNAP_LINE, "\n"));
  const texts = generate().map((e) => [e.text, e.ctx || {}]);
  for (const lang of ["en", "de", "es", "fr", "ja", "ko", "ru", "zh"]) {
    const d = JSON.parse(readFileSync(new URL(`../corpus/real-negatives/data/${lang}.json`, import.meta.url), "utf8"));
    for (const a of d.articles) for (const p of a.paragraphs.slice(0, 6)) texts.push([p, {}]);
  }
  assert.ok(texts.length > 8000);
  let diff = 0;
  for (const [t, ctx] of texts) {
    const c = { hot: ctx.hot || 0, reply: ctx.reply, launchWindow: ctx.launchWindow };
    const ra = A.classify.text(t, c), rb = B.classify.text(t, c);
    if (JSON.stringify(ra) !== JSON.stringify(rb)) diff++;
  }
  assert.equal(diff, 0);
  assert.equal(A.lexicon.fromSnapshot(), true);
});

test("a snapshot is not used when the page's normaliser disagrees, for another version, or with feed terms", () => {
  // normaliser drift (Unicode / ICU differences): sample keys no longer match -> compiled from the word lists
  const drift = load(shipped);
  const orig = drift.normalize.term;
  drift.normalize.term = (s) => { const r = orig(s); return /[^\x00-\x7f]/.test(s) ? Object.assign({}, r, { c: r.c + "x" }) : r; };
  drift.lexicon.compile();
  assert.equal(drift.lexicon.fromSnapshot(), false);
  // another version
  const other = load(shipped.replace(/(SFW\.lexicon\.setSnapshot\(".*?\\"v\\":\\")([^\\]+)/, "$1old"));
  other.lexicon.compile();
  assert.equal(other.lexicon.fromSnapshot(), false);
  // feed terms: extend() drops the cache; the next compile includes the new term (from the word lists)
  const fed = load(shipped);
  fed.lexicon.compile();
  assert.equal(fed.lexicon.fromSnapshot(), true);
  assert.equal(fed.lexicon.extend({ version: "x1", add: { franchise: ["xyzzy plugh"] } }).added, 1);
  assert.equal(fed.classify.text("xyzzy plugh is wild").flags.franchise, true);
  assert.equal(fed.lexicon.fromSnapshot(), false);
  // a corrupt snapshot falls back to compiling
  const bad = load(shipped.replace(SNAP_LINE, '\nSFW.lexicon.setSnapshot("{not json");\n'));
  assert.equal(bad.classify.text("GTA 6 ending spoiler leaked").level !== "none", true);
  assert.equal(bad.lexicon.fromSnapshot(), false);
});
