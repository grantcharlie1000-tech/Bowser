// Spoiler-leak audit fixes that need no DOM: high-risk adapters for the places GTA VI content lands, hot-by-URL rules
// (Wikipedia, the GTA wiki), and the cover theme maths. They run against the shipped engine (dist/node/engine.cjs).
import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const SFW = require("../../dist/node/engine.cjs");
const A = SFW.adapters;

const HIGH_RISK = {
  "imgur.com": "imgur", "i.imgur.com": "imgur", "boards.4chan.org": "fourchan", "i.4cdn.org": "fourchan", "boards.4channel.org": "fourchan",
  "www.gtaforums.com": "gtaforums", "gtaforums.com": "gtaforums", "www.resetera.com": "resetera", "web.telegram.org": "telegram", "t.me": "telegram",
  "kick.com": "kick", "files.kick.com": "kick", "www.tumblr.com": "tumblr", "64.media.tumblr.com": "tumblr", "foo.tumblr.com": "tumblr",
  "steamcommunity.com": "steam", "steamuserimages-a.akamaihd.net": "steam", "www.rockstargames.com": "rockstar", "gta.fandom.com": "fandom",
  "www.ign.com": "gamespress", "www.gamespot.com": "gamespress", "kotaku.com": "gamespress", "www.polygon.com": "gamespress",
  "www.eurogamer.net": "gamespress", "www.gamesradar.com": "gamespress", "www.pcgamer.com": "gamespress", "www.vg247.com": "gamespress",
  "www.pushsquare.com": "gamespress", "www.purexbox.com": "gamespress", "gamefaqs.gamespot.com": "gamespress",
};

test("adapters: every audited site is high-risk and resolves to its own adapter", () => {
  for (const [host, id] of Object.entries(HIGH_RISK)) {
    const a = A.forLocation({ hostname: host });
    assert.equal(a.highRisk, true, `${host} is high-risk`);
    assert.equal(a.id, id, `${host} adapter`);
  }
});

test("adapters: lookalike hosts stay generic and cold", () => {
  for (const host of ["notimgur.com", "imgur.com.evil.example", "fakeign.com", "ign.com.attacker.net", "kick.com.evil.example", "gtaforums.com.evil.test", "example.com"]) {
    const a = A.forLocation({ hostname: host });
    assert.equal(a.highRisk, false, host);
  }
});

test("hot by URL: a Wikipedia article about GTA VI is hot; an unrelated one is cold", () => {
  const generic = A.forLocation({ hostname: "en.wikipedia.org" });
  assert.equal(generic.highRisk, false);
  assert.equal(generic.hotUrl("https://en.wikipedia.org/wiki/Grand_Theft_Auto_VI"), 2);
  assert.equal(generic.hotUrl("https://en.wikipedia.org/wiki/Grand_Theft_Auto_VI#Plot"), 2);
  assert.equal(generic.hotUrl("https://en.wikipedia.org/wiki/Grand_Theft_Auto_V"), 0);
  assert.equal(generic.hotUrl("https://en.wikipedia.org/wiki/Bread"), 0);
});

test("hot by URL: the GTA wiki is hot for GTA VI pages and at least warm elsewhere; other wikis follow the URL rules", () => {
  const f = A.forLocation({ hostname: "gta.fandom.com" });
  assert.equal(f.hotUrl("https://gta.fandom.com/wiki/Grand_Theft_Auto_VI"), 2);
  assert.equal(f.hotUrl("https://gta.fandom.com/wiki/GTA_VI"), 2);
  assert.equal(f.hotUrl("https://gta.fandom.com/wiki/Lucia"), 2 * 0 + Math.max(1, f.hotUrl("https://gta.fandom.com/wiki/Lucia")), "warm or hot");
  assert.ok(f.hotUrl("https://gta.fandom.com/wiki/Los_Santos") >= 1, "never cold on the GTA wiki");
  assert.equal(f.hotUrl("https://gta.fandom.com/wiki/Vice_City_Stories") >= 1, true);
  const other = A.forLocation({ hostname: "elderscrolls.fandom.com" });
  assert.equal(other.hotUrl("https://elderscrolls.fandom.com/wiki/Skyrim"), 0);
});

test("cover theme: relative luminance above 0.5 is light, otherwise dark; transparent and unreadable are unknown", () => {
  const S = SFW.shield;
  assert.equal(S.themeFromColor("rgb(255, 255, 255)"), "light");
  assert.equal(S.themeFromColor("rgb(250, 250, 250)"), "light");
  assert.equal(S.themeFromColor("rgb(16, 16, 24)"), "dark");
  assert.equal(S.themeFromColor("rgb(0, 0, 0)"), "dark");
  assert.equal(S.themeFromColor("rgba(255, 255, 255, 0.9)"), "light");
  assert.equal(S.themeFromColor("rgb(255 255 255 / 0.8)"), "light");
  assert.equal(S.themeFromColor("rgba(0, 0, 0, 0)"), null, "transparent");
  assert.equal(S.themeFromColor("rgba(255, 255, 255, 0.4)"), null, "mostly transparent");
  assert.equal(S.themeFromColor("transparent"), null);
  assert.equal(S.themeFromColor(""), null);
  assert.equal(S.themeFromColor(undefined), null);
  // the threshold is relative luminance, not the average: pure red (0.2126) is dark, a light yellow is light
  assert.equal(S.themeFromColor("rgb(255, 0, 0)"), "dark");
  assert.equal(S.themeFromColor("rgb(255, 255, 160)"), "light");
});

test("cover theme: color-scheme counts only when it names exactly one scheme", () => {
  const S = SFW.shield;
  assert.equal(S.themeFromScheme("dark"), "dark");
  assert.equal(S.themeFromScheme("only dark"), "dark");
  assert.equal(S.themeFromScheme("light"), "light");
  assert.equal(S.themeFromScheme("light dark"), "auto");
  assert.equal(S.themeFromScheme("normal"), "auto");
  assert.equal(S.themeFromScheme(""), "auto");
});

test("styles: the theme attribute overrides the OS scheme, and attribute-only covers have light variants", () => {
  const css = SFW.styles.SHIELD_CSS;
  assert.match(css, /:host\(\[data-sfw-theme="light"\]\)\{[^}]*--fg:#1c1e29/);
  assert.match(css, /:host\(\[data-sfw-theme="dark"\]\)\{[^}]*--fg:#f4f2fa/);
  assert.ok(css.indexOf('[data-sfw-theme="light"]') > css.indexOf("@media (prefers-color-scheme:light)"), "attribute rules come after the media query so they win");
  assert.match(SFW.styles.PAGE_CSS, /\[data-sfw-fb\]\[data-sfw-theme="light"\]::after/);
  assert.match(SFW.styles.QUARANTINE_CSS, /\[data-sfw-mc\]\[data-sfw-theme="light"\]::after/);
});
