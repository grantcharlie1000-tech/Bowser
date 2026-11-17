import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const SFW = createRequire(import.meta.url)("../../dist/node/engine.cjs");
const N = SFW.normalize;

test("clean: strips zero-width, soft hyphen, bidi, variation selectors and tag characters", () => {
  assert.equal(N.clean("g​t‌a⁠ 6"), "gta 6");
  assert.equal(N.clean("g­t‮a‏"), "gta");
  assert.equal(N.clean("g️t\u{e0041}a"), "gta");
  assert.equal(N.clean("G‍T﻿A"), "gta");
});

test("clean: confusables (Cyrillic, Greek, Armenian, Cherokee), fullwidth, math, small caps, superscripts", () => {
  assert.equal(N.clean("gта"), "gta"); // Cyrillic te + a
  assert.equal(N.clean("ɡτα"), "gta"); // script g, Greek tau/alpha
  assert.equal(N.clean("ＧＴＡ ６"), "gta 6"); // fullwidth
  assert.equal(N.clean("𝖠𝖣"), "ad"); // math sans-serif
  assert.equal(N.clean("ɢᴛᴀ"), "gta"); // small caps
  assert.equal(N.clean("ᵍᵗᵃ ⁶"), "gta 6"); // superscripts
  assert.equal(N.clean("Ꮐ"), "g"); // Cherokee GA looks like G
  assert.equal(N.clean("ա"), N.clean("ա")); // Armenian: no crash
  assert.equal(N.clean("մօ"), N.clean("մօ"));
});

test("clean: enclosed, squared, negative-squared and regional-indicator letters", () => {
  assert.equal(N.clean("\u{1f176}\u{1f183}\u{1f170}"), "gta"); // negative squared
  assert.equal(N.clean("\u{1f1ec}\u{1f1f9}\u{1f1e6}"), "gta"); // regional indicators
  assert.equal(N.clean("ⒼⓉⒶ"), "gta"); // circled
  assert.equal(N.clean("\u{1f130}\u{1f13f}"), "ap"); // squared A P
  assert.equal(N.clean("6️⃣"), "6"); // keycap
});

test("clean: upside-down glyphs fold to letters; diacritics are stripped on Latin/Greek/Cyrillic only", () => {
  assert.equal(N.clean("ɐʇƃ"), "atg");
  assert.equal(N.clean("Crème brûlée"), "creme brulee");
  assert.equal(N.clean("g̈ta"), "gta");
  // kana: dakuten is kept (ga != ka); katakana folds to hiragana
  assert.equal(N.clean("ガ"), "が");
  assert.notEqual(N.clean("ガ"), N.clean("カ"));
  assert.equal(N.clean("グラセフ"), "ぐらせふ");
  // non-ASCII digits
  assert.equal(N.clean("٦"), "6");
  assert.equal(N.clean("๖"), "6");
});

test("views: spaced / compact / leet / squashed / split / rot13 / reversed", () => {
  const v = N.views("G.T.A 6!!! #GtaSixEnding gtaaaa 66");
  assert.ok(v.compact.includes("gta6"));
  assert.ok(v.spaced.startsWith("g t a 6"));
  assert.ok(v.split.includes("gta six ending"));
  assert.ok(v.squashed.includes("gta 6"));
  const l = N.views("j4s0n d13s");
  assert.equal(l.leet, "jasondies");
  assert.equal(l.compact, "j4s0nd13s"); // digits preserved in the parallel view
  assert.equal(N.views("gta 6").rot13, "tgn 6");
  assert.equal(N.views("gta 6").reversed, "6 atg");
  assert.equal(N.views("").spaced, "");
});

test("token view: starts/ends and sentence ids; compact matches may not cross sentence punctuation", () => {
  const t = N.build(N.fold("GTA V. I loved it"));
  assert.equal(t.s, "gtavilovedit");
  assert.notEqual(t.sid[t.s.indexOf("v")], t.sid[t.s.indexOf("i")]);
  const t2 = N.build(N.fold("GTA V I loved"));
  assert.equal(t2.sid[3], t2.sid[4]);
  for (const sep of [".", "!", "?", ";", ":", "\n\n", "。"]) {
    const x = N.build(N.fold("alpha" + sep + " beta"));
    assert.notEqual(x.sid[0], x.sid[x.n - 1], "boundary for " + JSON.stringify(sep));
  }
  // abbreviation dots and "GTA: VI" do not split
  const a = N.build(N.fold("G.T.A. 6"));
  assert.equal(a.sid[0], a.sid[a.n - 1]);
  const c = N.build(N.fold("Grand Theft Auto: VI"));
  assert.equal(c.sid[0], c.sid[c.n - 1]);
  // token flags
  const f = N.build(N.fold("ab cd"));
  assert.equal(f.f[0] & N.FL_START, N.FL_START);
  assert.equal(f.f[1] & N.FL_END, N.FL_END);
  assert.equal(f.f[2] & N.FL_START, N.FL_START);
});

test("fold is idempotent-safe on hostile input and never throws", () => {
  const nasty = ["", null, undefined, 42, "\u0000\u0001\u0002", "\ud800", "a".repeat(100000), "‮".repeat(1000) + "x", "\u{1f600}".repeat(500)];
  for (const s of nasty) assert.doesNotThrow(() => N.views(s).compact);
  assert.equal(N.clean(null), "");
});
