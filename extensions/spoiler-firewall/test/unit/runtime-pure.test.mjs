// Pure-function tests for the runtime / adapters / styles pieces that need no DOM (they run against the shipped
// engine in dist/node/engine.cjs, so `node build/build.mjs` must have run; `npm test` does that first).
import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const SFW = require("../../dist/node/engine.cjs");
const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");

test("fnv1a: known vectors and stability", () => {
  assert.equal(SFW.runtime.fnv1a(""), 0x811c9dc5);
  assert.equal(SFW.runtime.fnv1a("a"), 0xe40c292c);
  assert.equal(SFW.runtime.fnv1a("foobar"), 0xbf9cf968);
  assert.equal(SFW.runtime.fnv1a("x"), SFW.runtime.fnv1a("x"));
});

test("fingerprint: differs for different content and length", () => {
  const f = SFW.runtime.fingerprint;
  assert.notEqual(f("hello"), f("hellp"));
  assert.notEqual(f("a"), f("aa"));
  assert.equal(f("same text"), f("same text"));
  assert.match(f("abc"), /^[0-9a-z]+\.[0-9a-z]+$/);
});

test("slugText: path slugs become words, hosts and unsafe schemes are dropped", () => {
  const s = SFW.runtime.slugText;
  assert.equal(s("https://www.reddit.com/r/pics/comments/abc/sunset_at_the_lake/"), "r pics comments abc sunset at the lake");
  assert.equal(s("/search?q=weather+tomorrow"), "search q weather tomorrow");
  assert.equal(s("javascript:alert(1)"), "");
  assert.equal(s("mailto:a@b.c"), "");
  assert.equal(s("https://x.com/user/status/123#frag"), "user status 123");
  assert.equal(s(""), "");
});

test("srcWords: wordy file names only", () => {
  const w = SFW.runtime.srcWords;
  assert.equal(w("https://cdn.example/img/spoiler-gta6-ending-scene.jpg?x=1"), "spoiler gta6 ending scene");
  assert.equal(w("https://cdn.example/9f8a7b6c.png"), "");
  assert.equal(w("data:image/png;base64,AAAA"), "");
});

test("isBlockTag: standard blocks and custom elements", () => {
  assert.ok(SFW.runtime.isBlockTag("P"));
  assert.ok(SFW.runtime.isBlockTag("TD"));
  assert.ok(SFW.runtime.isBlockTag("SHREDDIT-COMMENT"));
  assert.ok(!SFW.runtime.isBlockTag("SPAN"));
  assert.ok(!SFW.runtime.isBlockTag("B"));
});

test("normalizeConfig: defaults protect, garbage modes fall back to strict", () => {
  const n = SFW.runtime.normalizeConfig;
  assert.deepEqual(n(null), { enabled: true, mode: "strict", reducedEffects: false, version: "", debugReports: false });
  assert.equal(n({ mode: "nonsense" }).mode, "strict");
  assert.equal(n({ mode: "lockdown" }).mode, "lockdown");
  assert.equal(n({ mode: "balanced", reducedEffects: 1 }).reducedEffects, true);
  assert.equal(n({ enabled: false }).enabled, false);
});

test("adapters.forLocation: platform detection by hostname", () => {
  const id = (h) => SFW.adapters.forLocation({ hostname: h }).id;
  assert.equal(id("x.com"), "x");
  assert.equal(id("twitter.com"), "x");
  assert.equal(id("www.reddit.com"), "reddit");
  assert.equal(id("old.reddit.com"), "reddit");
  assert.equal(id("sh.reddit.com"), "reddit");
  assert.equal(id("www.youtube.com"), "youtube");
  assert.equal(id("m.youtube.com"), "youtube");
  assert.equal(id("www.google.com"), "google");
  assert.equal(id("www.google.co.uk"), "google");
  assert.equal(id("www.bing.com"), "bing");
  assert.equal(id("duckduckgo.com"), "ddg");
  assert.equal(id("search.yahoo.com"), "yahoo");
  assert.equal(id("search.brave.com"), "brave");
  assert.equal(id("www.startpage.com"), "startpage");
  assert.equal(id("www.tiktok.com"), "tiktok");
  assert.equal(id("www.instagram.com"), "instagram");
  assert.equal(id("www.facebook.com"), "facebook");
  assert.equal(id("www.threads.net"), "threads");
  assert.equal(id("bsky.app"), "bluesky");
  assert.equal(id("www.twitch.tv"), "twitch");
  assert.equal(id("discord.com"), "discord");
  assert.equal(id("example.org"), "generic");
  assert.equal(id(""), "generic");
});

test("adapters: shape, generic fallback always merged, high-risk flags", () => {
  for (const name of SFW.adapters.list()) {
    const def = SFW.adapters.definition(name);
    assert.ok(def, name);
  }
  for (const host of ["x.com", "www.reddit.com", "www.youtube.com", "www.google.com", "example.org"]) {
    const a = SFW.adapters.forLocation({ hostname: host });
    for (const k of ["id", "highRisk", "item", "own", "generic", "thread", "context", "media", "hotUrl"]) assert.ok(k in a, `${host}.${k}`);
    assert.ok(a.item.includes("article"), `${host}: generic items merged`);
    assert.ok(a.media.includes("img"), `${host}: generic media merged`);
    assert.equal(typeof a.hotUrl, "function");
  }
  assert.equal(SFW.adapters.forLocation({ hostname: "x.com" }).highRisk, true);
  assert.equal(SFW.adapters.forLocation({ hostname: "example.org" }).highRisk, false);
  assert.ok(SFW.adapters.forLocation({ hostname: "x.com" }).own.includes("article[data-testid=tweet]"));
  assert.ok(SFW.adapters.forLocation({ hostname: "www.reddit.com" }).own.includes("shreddit-comment"));
  assert.ok(SFW.adapters.forLocation({ hostname: "www.youtube.com" }).own.includes("ytd-comment-thread-renderer"));
});

test("adapters.hotUrl: hot searches and communities, cold otherwise", () => {
  const g = SFW.adapters.forLocation({ hostname: "www.google.com" });
  assert.ok(g.hotUrl("https://www.google.com/search?q=gta+6+ending+spoiler") >= 1);
  assert.equal(g.hotUrl("https://www.google.com/search?q=weather+tomorrow"), 0);
  const y = SFW.adapters.forLocation({ hostname: "www.youtube.com" });
  assert.ok(y.hotUrl("https://www.youtube.com/results?search_query=gta+6+ending") >= 1);
  assert.equal(y.hotUrl("https://www.youtube.com/results?search_query=pasta"), 0);
  const x = SFW.adapters.forLocation({ hostname: "x.com" });
  assert.ok(x.hotUrl("https://x.com/hashtag/GTA6") >= 1);
  assert.ok(x.hotUrl("https://x.com/search?q=gta%206%20ending") >= 1);
  assert.equal(x.hotUrl("https://x.com/home"), 0);
  const r = SFW.adapters.forLocation({ hostname: "www.reddit.com" });
  assert.equal(r.hotUrl("https://www.reddit.com/r/pics/"), 0);
  assert.ok(r.hotUrl("https://www.reddit.com/r/GTA/") >= 1, "r/GTA is at least warm");
  assert.ok(r.hotUrl("https://www.reddit.com/r/gta6/") >= 1);
});

test("styles: CSS strings exist and encode the secrecy rules", () => {
  const { PAGE_CSS, QUARANTINE_CSS, SHIELD_CSS } = SFW.styles;
  assert.ok(PAGE_CSS.includes('[data-sfw~="shield"]'));
  assert.match(PAGE_CSS, /visibility:hidden!important/);
  assert.match(PAGE_CSS, /transition:none!important;animation:none!important/);
  assert.match(PAGE_CSS, /-webkit-text-fill-color:transparent/);
  assert.match(PAGE_CSS, /user-select:none/);
  assert.match(PAGE_CSS, /data-sfw-pos/);
  assert.ok(QUARANTINE_CSS.includes("img:not([data-sfw-m=\"ok\"])"));
  assert.ok(QUARANTINE_CSS.includes("[poster]"));
  assert.ok(QUARANTINE_CSS.includes("svg image"));
  assert.ok(QUARANTINE_CSS.includes("background-image"));
  assert.match(SHIELD_CSS, /forced-colors/);
  assert.match(SHIELD_CSS, /prefers-reduced-motion/);
  assert.match(SHIELD_CSS, /:focus-visible/);
  assert.match(SHIELD_CSS, /@container/);
  assert.match(SHIELD_CSS, /border:4px double/, "double-border stamp");
  assert.match(SHIELD_CSS, /backdrop-filter/);
  // opacity floor: the tint alpha used by the cover is >= .82
  const m = /--sfw-a:\.(\d+)/.exec(SHIELD_CSS);
  assert.ok(m && Number("0." + m[1]) >= 0.82);
  assert.ok(!/@import|url\(http/.test(SHIELD_CSS + PAGE_CSS), "no external resources");
});

test("hooks: install is idempotent and non-enumerable on a fake window", () => {
  let attach = function attachShadow(init) { return { mode: init.mode }; };
  let play = function play() { return Promise.resolve("played"); };
  const win = {
    Element: { prototype: { attachShadow: attach, setAttribute() {}, localName: "div" } },
    HTMLMediaElement: { prototype: { play } },
    History: { prototype: {} },
    DOMException: class extends Error { constructor(m, n) { super(m); this.name = n; } },
  };
  assert.equal(SFW.hooks.install(win), true);
  const wrapped = win.Element.prototype.attachShadow;
  assert.notEqual(wrapped, attach);
  assert.equal(wrapped.name, "attachShadow");
  assert.equal(wrapped.length, 1);
  assert.equal(SFW.hooks.install(win), false, "second install is a no-op");
  assert.equal(win.Element.prototype.attachShadow, wrapped);
  const d = Object.getOwnPropertyDescriptor(win.Element.prototype, "attachShadow");
  assert.equal(d.enumerable, false);
  assert.equal(win.HTMLMediaElement.prototype.play.name, "play");
});

test("source hygiene: no innerHTML / <style> creation / eval in shipped engine files I own", () => {
  for (const f of ["adapters.js", "styles.js", "shield.js", "hooks.js", "runtime.js"]) {
    const src = readFileSync(join(ROOT, "engine", f), "utf8").split("\n").filter((l) => !/^\s*\/\//.test(l)).join("\n");
    assert.ok(!/\.innerHTML\s*=/.test(src), `${f}: innerHTML`);
    assert.ok(!/createElement\(\s*["']style["']/.test(src), `${f}: <style>`);
    assert.ok(!/\beval\s*\(|new Function\s*\(/.test(src), `${f}: eval`);
    assert.ok(!/console\.(log|debug|info)/.test(src), `${f}: console`);
  }
});

test("bowser host keeps the config placeholder exactly once and exposes no globals", () => {
  const src = readFileSync(join(ROOT, "hosts", "bowser", "bowser-host.js"), "utf8");
  assert.equal(src.split("/*__SFW_CONFIG__*/null").length - 1, 1);
  assert.ok(!/window\.\w+\s*=|globalThis\.\w+\s*=/.test(src));
});
