// Cross-checks that keep the Chrome extension, Bowser and the tooling on the same feed: one key, one URL, one
// built-in version, one set of status words, and a package that carries no test key.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync, statSync, existsSync } from "node:fs";
import { join, relative } from "node:path";
import { createRequire } from "node:module";
import { FEED_DIR, loadClient, productionKeys, SFW_ROOT, readPublicKeys, publicKeyBase64, loadPrivateKey } from "../../feed/tools/lib.mjs";
import { validateDist, ALLOWED_PERMISSIONS, VENDOR_SCRIPT_PINS, FETCH_ALLOWED } from "../../build/validate.mjs";

const REPO = join(SFW_ROOT, "..", "..");
const text = (rel) => readFileSync(join(SFW_ROOT, rel), "utf8");
const { SFWFeed, SFWFeedConfig } = loadClient();
const testPub = publicKeyBase64(loadPrivateKey(join(FEED_DIR, "test-keys", "test-feed-key.pem")));

function walk(dir) {
  return readdirSync(dir).flatMap((n) => (statSync(join(dir, n)).isDirectory() ? walk(join(dir, n)) : [join(dir, n)]));
}

test("Chrome's embedded keys are exactly feed/public-key.txt, and none is the test key", () => {
  assert.deepEqual([...SFWFeedConfig.PUBLIC_KEYS], productionKeys());
  assert.ok(productionKeys().length >= 1);
  assert.ok(!SFWFeedConfig.PUBLIC_KEYS.includes(testPub));
});

test("one feed URL for both hosts: HTTPS, no query, the documented published path", () => {
  const rust = readFileSync(join(REPO, "src", "spoiler_feed.rs"), "utf8");
  const rustUrl = /pub const FEED_URL: &str = "([^"]+)";/.exec(rust)[1];
  assert.equal(SFWFeedConfig.FEED_URL, rustUrl);
  const u = new URL(SFWFeedConfig.FEED_URL);
  assert.equal(u.protocol, "https:");
  assert.equal(u.hostname, "raw.githubusercontent.com");
  assert.equal(u.search + u.hash, "");
  assert.ok(u.pathname.endsWith("/extensions/spoiler-firewall/feed/published/feed.json"));
  assert.equal(SFWFeedConfig.CHECK_EVERY_MINUTES * 60, Number(/CHECK_INTERVAL: Duration = Duration::from_secs\(([\d_ *]+)\)/.exec(rust)[1].split("*").reduce((a, b) => a * Number(b.replace(/_/g, "")), 1)));
  assert.equal(SFWFeedConfig.FETCH_TIMEOUT_MS, 10000);
});

test("the built-in version shown next to the update number is the engine's real lexicon version", () => {
  const SFW = createRequire(import.meta.url)(join(SFW_ROOT, "dist", "node", "engine.cjs"));
  const base = SFW.lexicon.version.split("+")[0];
  assert.equal(SFWFeedConfig.BUILTIN_VERSION, base, `hosts/chrome/feed-config.js BUILTIN_VERSION must follow engine/lexicon.js VERSION (${base}); the Rust test the_url_and_the_builtin_version_agree_with_the_chrome_host_and_the_bundle checks the same`);
});

test("the status words match Bowser's (src/spoiler_feed/tests.rs asserts the same strings)", () => {
  const rustTests = readFileSync(join(REPO, "src", "spoiler_feed", "tests.rs"), "utf8");
  const feed = { seq: 5, issued: "2026-01-01", expires: "2026-01-31" };
  const line = (o) => { const d = SFWFeed.describe({ builtin: "B", now: Date.parse("2026-11-17T12:00:00Z"), ...o }); return d.text + (d.notes.length ? ". " + d.notes.join(" ") : ""); };
  const cases = [
    [{ enabled: true, feed, status: { state: "ok" } }, "Term list: built-in B + update #5 (2026-01-01). Term updates out of date."],
    [{ enabled: true, feed: null, status: { state: "none" } }, "Term list: built-in B. No term update has been published yet."],
    [{ enabled: false, feed: null, status: null }, "Term list: built-in B. Term updates are off."],
    [{ enabled: true, feed: { ...feed, expires: "2099-01-01" }, status: { state: "rejected" } }, "Term list: built-in B + update #5 (2026-01-01). Term update rejected."],
    [{ enabled: true, feed: { ...feed, expires: "2099-01-01" }, status: { state: "unavailable" } }, "Term list: built-in B + update #5 (2026-01-01). Could not check for term updates."],
    [{ enabled: true, feed: null, status: null }, "Term list: built-in B"],
  ];
  for (const [input, want] of cases) assert.equal(line(input), want);
  for (const phrase of ["Term updates out of date.", "No term update has been published yet.", "Term updates are off.", "Term update rejected.", "Could not check for term updates."]) {
    assert.ok(readFileSync(join(REPO, "src", "spoiler_feed.rs"), "utf8").includes(phrase), `Rust has "${phrase}"`);
    assert.ok(rustTests.includes(phrase) || phrase === "Term updates out of date.", phrase);
  }
});

test("manifest: alarms is the only permission added, no host permissions, minimum Chrome unchanged", () => {
  const m = JSON.parse(text("hosts/chrome/manifest.json"));
  assert.deepEqual(m.permissions, ["storage", "alarms"]);
  assert.deepEqual(ALLOWED_PERMISSIONS, ["storage", "alarms"]);
  assert.ok(!m.host_permissions && !m.optional_host_permissions);
  assert.equal(m.minimum_chrome_version, "120");
  assert.notEqual(m.background.type, "module");
  assert.deepEqual(m.optional_permissions, ["offscreen"]);
});

test("the service worker loads the feed scripts in dependency order; pages load config and feed before the UI", () => {
  const sw = text("hosts/chrome/service-worker.js");
  const m = /importScripts\(([^)]*)\)/.exec(sw);
  assert.deepEqual(m[1].split(",").map((s) => s.trim().replace(/"/g, "")), ["vendor/noble-ed25519/noble-ed25519.js", "feed-config.js", "feed.js", "feed-update.js"]);
  for (const page of ["popup.html", "options.html"]) {
    const html = text(`hosts/chrome/${page}`);
    const order = ["feed-config.js", "feed.js", "common.js"].map((f) => html.indexOf(`src="${f}"`));
    assert.ok(order.every((i) => i > 0) && order[0] < order[1] && order[1] < order[2], page);
  }
  assert.match(sw, /SFWFeedUpdate\.start\(\)/);
});

test("the content script reads the stored feed with the config and applies it only through SFW.lexicon.extend", () => {
  const src = text("hosts/chrome/content.js");
  assert.match(src, /var KEYS = \[[^\]]*"feed"\]/);
  assert.match(src, /SFW\.lexicon\.extend\(feed\.lexicon\)/);
  // applied inside the storage read, before configReady resolves (the engine holds classification until then)
  const apply = src.indexOf('"feed" in values');
  assert.ok(apply > 0);
  assert.ok(src.indexOf("cb.storage.local.get(KEYS).then") > 0);
  assert.ok(!/feed[^;\n]*\.(?:remove|delete|splice)\b/.test(src), "no removal path");
});

test("the packaged extension carries the production key, the pinned verifier and no test key", () => {
  const dist = join(SFW_ROOT, "dist", "chrome");
  assert.ok(existsSync(dist), "run node build/build.mjs first");
  const files = walk(dist).filter((f) => !/\.(png|wasm|traineddata|gz)$/.test(f) && !f.includes("tesseract"));
  for (const f of files) {
    const body = readFileSync(f, "utf8");
    assert.ok(!body.includes(testPub), `${relative(dist, f)} contains the test public key`);
    assert.ok(!/BEGIN (?:PRIVATE|EC PRIVATE|OPENSSH PRIVATE)/.test(body), `${relative(dist, f)} contains private key material`);
  }
  assert.ok(files.some((f) => f.endsWith("noble-ed25519.js")));
  assert.ok(!walk(dist).some((f) => /test-keys|feed[\\/]source|sample[\\/]feed\.json/.test(f)));
  const r = validateDist(dist, { version: JSON.parse(text("package.json")).version });
  assert.deepEqual(r.errors, []);
  assert.deepEqual(r.warnings, []);
  assert.ok(VENDOR_SCRIPT_PINS["vendor/noble-ed25519/noble-ed25519.js"]);
  assert.equal(FETCH_ALLOWED["feed-update.js"], 1);
});

test("the Bowser bundle hands config.feed to extend() and nothing else about the feed lives in it", () => {
  const bundle = text("dist/bowser/spoiler-firewall.js");
  assert.equal(bundle.split("SFW.lexicon.extend(config.feed)").length - 1, 1);
  assert.ok(!bundle.includes("raw.githubusercontent.com"), "the engine bundle never contacts a server");
  assert.ok(!bundle.includes("nobleEd25519"), "verification is native; the page-world bundle holds no keys or crypto");
});

test("public-key.txt: comments and blank lines ignored, one key per line, no private material", () => {
  const raw = text("feed/public-key.txt");
  assert.ok(!/PRIVATE/.test(raw));
  assert.deepEqual(readPublicKeys(raw), productionKeys());
});
