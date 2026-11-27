// The Bowser bundle's engine is minified (build/minify.mjs: comments and indentation removed, every token and every line
// break between tokens kept). These tests pin what Bowser's native code and the other suites rely on, and that the minified
// engine behaves exactly like the sources it was made from.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { tokenize, minifyJs } from "../../build/minify.mjs";
import { engineSource, minifyEngine, VERSION } from "../../build/build.mjs";
import { generate } from "../corpus/generate.mjs";

const BUNDLE = readFileSync(fileURLToPath(new URL("../../dist/bowser/spoiler-firewall.js", import.meta.url)), "utf8");
const ENGINE = fileURLToPath(new URL("../../dist/node/engine.cjs", import.meta.url));
const load = (text) => { const m = { exports: {} }; new Function("module", "exports", text)(m, m.exports); return m.exports; };
const sig = (src) => tokenize(src).filter((t) => t.k !== "ws" && t.k !== "nl" && t.k !== "com").map((t) => t.k + ":" + t.v);

test("the bundle keeps what src/spoiler.rs and src/spoiler_feed.rs read from it", () => {
  // bundle_version(): the first line names the version
  assert.match(BUNDLE.split("\n")[0], new RegExp(`^/\\* Spoiler Firewall for GTA VI ${VERSION.replace(/\./g, "\\.")} `));
  // exactly one config placeholder
  assert.equal(BUNDLE.split("/*__SFW_CONFIG__*/null").length, 2);
  // builtin_lexicon_version(): this exact text precedes the lexicon version
  assert.match(BUNDLE, /SFW\.lexicon = \(function \(\) \{\n {2}const VERSION = "[^"\n]+";\n/);
  // the host lines src/spoiler.rs tests and the perf / e2e harnesses look for
  for (const s of ["SFW.lexicon.extend(config.feed)", "SFW.runtime.start({", "feed: feed", "onConfigChange: function (cb) { configCallback = cb; }", "configCallback(config)", "appliedFeeds"]) {
    assert.ok(BUNDLE.includes(s), s);
  }
  assert.ok(BUNDLE.indexOf("SFW.lexicon.extend(config.feed)") < BUNDLE.indexOf("SFW.runtime.start({"));
  // the lexicon snapshot stays on a line of its own (test/unit/lexicon-snapshot.test.mjs swaps it out)
  assert.match(BUNDLE, /\nSFW\.lexicon\.setSnapshot\(.*\);\n/);
});

test("minifying changes no token: every engine fragment re-tokenizes to the same sequence", () => {
  const src = engineSource();
  const min = minifyEngine(src);
  assert.ok(min.length < src.length * 0.85, `minified ${min.length} of ${src.length}`);
  assert.deepEqual(sig(min), sig(src));
});

test("line breaks between tokens are kept (automatic semicolon insertion is unchanged)", () => {
  const src = "let a = 1\nlet b = a\n++b\nfunction f() { return\n  a }\nconst r = /x\\/y/g, d = a / 2 / b, t = `p${a /* c */}q` // end";
  const out = minifyJs(src);
  assert.equal(out, "let a=1\nlet b=a\n++b\nfunction f(){return\na}\nconst r=/x\\/y/g,d=a/2/b,t=`p${a}q`");
  assert.deepEqual(sig(out), sig(src));
  assert.equal(minifyJs("a + +b - -c"), "a+ +b- -c");
  assert.equal(minifyJs("if (x) /re/.test(y)"), "if(x)/re/.test(y)");
  assert.throws(() => minifyJs("x = {} / 2"), /ambiguous/);
});

test("the shipped (minified) engine classifies exactly like the unminified sources", () => {
  const A = load(readFileSync(ENGINE, "utf8"));
  const B = load(`module.exports = (function () {\n"use strict";\nconst SFW = { version: ${JSON.stringify(VERSION)} };\n${engineSource()}\nreturn SFW;\n})();`);
  let diff = 0, n = 0;
  for (const e of generate()) {
    const c = { hot: (e.ctx && e.ctx.hot) || 0, reply: e.ctx && e.ctx.reply, launchWindow: e.ctx && e.ctx.launchWindow };
    const ra = A.classify.text(e.text, c), rb = B.classify.text(e.text, c);
    if (ra.truncated || rb.truncated) continue;
    n++;
    if (JSON.stringify(ra) !== JSON.stringify(rb)) diff++;
  }
  assert.ok(n > 5000);
  assert.equal(diff, 0);
});
