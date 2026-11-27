// What every document pays before there is anything to protect (round 3): the shadow-host mark only for a runtime in
// another world, the universal descendant rules kept out of the sheets until a mark needs them, the play() guard's
// ancestor walk only while a hold is in place, and no startEngine() state declared where it is never initialised.
import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const SFW = require("../../dist/node/engine.cjs");
const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");

function selectors(css) {
  const out = [];
  css = css.replace(/\/\*[\s\S]*?\*\//g, "").replace(/@keyframes[^{]*\{(?:[^{}]*\{[^{}]*\})*[^{}]*\}/g, "").replace(/@media[^{]*\{/g, "");
  const re = /([^{}]*)\{([^{}]*)\}/g;
  let m;
  while ((m = re.exec(css))) for (const s of m[1].split(",").map((x) => x.trim()).filter(Boolean)) out.push(s);
  return out;
}
// the rightmost compound of a selector, without a trailing pseudo-element
const rightmost = (sel) => sel.replace(/:host\([^)]*\)/g, ":host").split(/\s*[\s>+~]\s*/).pop().replace(/::(before|after)$/, "");

test("styles: no rule of the page sheet has a universal rightmost compound; the descendant rules are all in DESC", () => {
  const { PAGE_CSS, DESC } = SFW.styles;
  for (const s of selectors(PAGE_CSS)) assert.notEqual(rightmost(s), "*", `PAGE_CSS: ${s}`);
  const want = {
    shield: ['[data-sfw~="shield"] *', '[data-sfw~="shield"] *::before', '[data-sfw~="shield"] *::after', ':host([data-sfw~="shield"]) *', ':host([data-sfw~="shield"]) *::before', ':host([data-sfw~="shield"]) *::after'],
    defer: ["[data-sfw-defer] *", "[data-sfw-defer] *::before", "[data-sfw-defer] *::after"],
    page: ['html[data-sfw-page~="shield"] > body *'],
  };
  for (const k of Object.keys(want)) {
    const got = selectors(DESC[k]);
    for (const s of want[k]) assert.ok(got.includes(s), `DESC.${k} has ${s}`);
    for (const s of got) if (s !== '[data-sfw~="shield"][data-sfw-fb]::after') assert.equal(rightmost(s), "*", `DESC.${k}: ${s}`);
  }
  // the fallback label is restated after the :host() descendant rule, which it followed in the page sheet
  assert.ok(DESC.shield.indexOf('[data-sfw~="shield"][data-sfw-fb]::after{visibility:visible!important}') > DESC.shield.indexOf(':host([data-sfw~="shield"]) *::after'));
  assert.match(PAGE_CSS, /\[data-sfw~="shield"\]\[data-sfw-fb\]::after,:host\(\[data-sfw~="shield"\]\)\[data-sfw-fb\]::after\{[^}]*visibility:visible!important/);
  // what stays in the page sheet still hides the marked element itself
  assert.match(PAGE_CSS, /\[data-sfw-defer\]\{visibility:hidden!important\}/);
  assert.match(PAGE_CSS, /html\[data-sfw-page~="shield"\] > body\{visibility:hidden!important\}/);
  assert.match(PAGE_CSS, /:host\(\[data-sfw~="shield"\]\)\{\s*visibility:hidden!important/);
});

function fakeWindow(log) {
  const queued = [];
  const win = {
    queueMicrotask(f) { queued.push(f); },
    Element: { prototype: { attachShadow(init) { return { mode: init.mode }; }, setAttribute(n, v) { log.push(["set", this.id, n, v]); } } },
    HTMLMediaElement: { prototype: { play() { log.push(["play", this.id]); return "played"; } } },
    History: { prototype: {} },
    DOMException: class extends Error { constructor(m, n) { super(m); this.name = n; } },
  };
  return { win, queued };
}

test("hooks: data-sfw-sr is written for a runtime in another world only", () => {
  const log = [];
  const a = fakeWindow(log);
  assert.equal(SFW.hooks.install(a.win), true);
  const h1 = { id: "h1", localName: "div" };
  const r1 = a.win.Element.prototype.attachShadow.call(h1, { mode: "closed" });
  assert.equal(SFW.hooks.rootOf(h1), r1);
  assert.equal(a.queued.length, 1, "cross-world: the host mark is queued");
  a.queued[0]();
  assert.deepEqual(log, [["set", "h1", "data-sfw-sr", "1"]]);

  const b = fakeWindow(log);
  assert.equal(SFW.hooks.install(b.win, { sameWorld: true }), true);
  const h2 = { id: "h2", localName: "div" };
  const r2 = b.win.Element.prototype.attachShadow.call(h2, { mode: "closed" });
  assert.equal(SFW.hooks.rootOf(h2), r2, "same world: the root is in the map");
  assert.equal(b.queued.length, 0, "same world: no microtask, no attribute");
  let seen = null;
  const off = SFW.hooks.subscribe((host, root) => { seen = [host, root]; });
  const h3 = { id: "h3", localName: "span" };
  const r3 = b.win.Element.prototype.attachShadow.call(h3, { mode: "open" });
  off();
  assert.deepEqual(seen, [h3, r3], "listeners are still told synchronously");
});

test("hooks: play() walks the ancestors only while a hold the shield set is in place (same world)", () => {
  const log = [];
  const { win } = fakeWindow(log);
  assert.equal(SFW.hooks.install(win, { sameWorld: true }), true);
  let reads = 0;
  const node = (id, parent, held) => ({ id, nodeType: 1, parentNode: parent, hasAttribute(n) { reads++; return held && n === "data-sfw-hold"; }, pause() {}, muted: false });
  const doc = { nodeType: 9, parentNode: null };
  const box = node("box", doc, true);
  const video = node("video", box, false);
  const play = win.HTMLMediaElement.prototype.play;
  // a hold attribute nobody reported (a page wrote it): with nothing of ours in place the walk is skipped
  assert.equal(play.call(video), "played");
  assert.equal(reads, 0);
  SFW.hooks.noteHold(box, true);
  const p = play.call(video);
  assert.ok(reads > 0, "walked");
  assert.ok(p && typeof p.then === "function", "held: rejected promise");
  p.catch(() => {});
  SFW.hooks.noteHold(box, false);
  reads = 0;
  assert.equal(play.call(video), "played");
  assert.equal(reads, 0);

  // another world (no tracking possible): always walks, as before
  const other = fakeWindow(log);
  assert.equal(SFW.hooks.install(other.win), true);
  reads = 0;
  const q = other.win.HTMLMediaElement.prototype.play.call(video);
  assert.ok(reads > 0);
  q.catch(() => {});
});

test("runtime: no startEngine() state is declared after its return (it would stay in its temporal dead zone)", () => {
  const src = readFileSync(join(ROOT, "engine", "runtime.js"), "utf8").split("\n");
  const start = src.findIndex((l) => l.startsWith("  function startEngine("));
  assert.ok(start > 0);
  let ret = -1, end = -1;
  for (let i = start + 1; i < src.length; i++) {
    if (ret < 0 && src[i] === "    return ctrl;") ret = i;
    if (src[i] === "  }") { end = i; break; }
  }
  assert.ok(ret > start && end > ret);
  const late = [];
  for (let i = ret + 1; i < end; i++) if (/^ {4}(const|let|var|class) /.test(src[i])) late.push(`${i + 1}: ${src[i].trim().slice(0, 60)}`);
  assert.deepEqual(late, []);
});
