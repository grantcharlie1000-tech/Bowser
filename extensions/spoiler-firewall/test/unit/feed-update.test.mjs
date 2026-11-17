// The service worker's download/verify/store logic (hosts/chrome/feed-update.js) against a fake `chrome` and a
// fake `fetch`: request options, byte and time caps, status handling, who may enable the test override, and
// that a rejected download never touches the stored feed. The real browser path is test/chrome/feed-extension.mjs.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import vm from "node:vm";
import { loadPrivateKey, makeEnvelope, publicKeyBase64, FEED_DIR, SFW_ROOT } from "../../feed/tools/lib.mjs";

const testKey = loadPrivateKey(join(FEED_DIR, "test-keys", "test-feed-key.pem"));
const TEST_PUB = publicKeyBase64(testKey);
const feedText = (seq, expires = "2099-01-01") => makeEnvelope(JSON.stringify({ format: 1, seq, issued: "2026-11-17", expires, lexicon: { add: { entityStrong: ["zz fake name"] } } }), testKey);

// A fresh sandbox per test: classic scripts loaded exactly as the service worker's importScripts does.
function world({ manifest = {}, stored = {}, respond }) {
  const store = { ...stored };
  const calls = { fetch: [], timers: [], alarms: [] };
  const listeners = {};
  const chrome = {
    runtime: { getManifest: () => manifest, onStartup: { addListener: (f) => (listeners.startup = f) }, onInstalled: { addListener: (f) => (listeners.installed = f) } },
    storage: {
      local: {
        get: async (keys) => { const out = {}; for (const k of [].concat(keys)) if (k in store) out[k] = store[k]; return JSON.parse(JSON.stringify(out)); },
        set: async (o) => { Object.assign(store, JSON.parse(JSON.stringify(o))); },
      },
    },
    alarms: {
      get: async (name) => calls.alarms.find((a) => a.name === name) || null,
      create: async (name, info) => { calls.alarms.push({ name, ...info }); },
      onAlarm: { addListener: (f) => (listeners.alarm = f) },
    },
  };
  const sandbox = {
    chrome, TextEncoder, TextDecoder, atob, btoa, URL, AbortController, Promise, console, crypto: globalThis.crypto, Uint8Array,
    fetch: (url, init) => { calls.fetch.push({ url, init }); return respond(url, init); },
    setTimeout: (fn, ms) => { const t = { fn, ms, live: true }; calls.timers.push(t); return t; },
    clearTimeout: (t) => { if (t) t.live = false; },
  };
  sandbox.self = sandbox;
  sandbox.globalThis = sandbox;
  vm.createContext(sandbox);
  for (const rel of ["vendor/noble-ed25519/noble-ed25519.js", "hosts/chrome/feed-config.js", "hosts/chrome/feed.js", "hosts/chrome/feed-update.js"]) {
    vm.runInContext(readFileSync(join(SFW_ROOT, rel), "utf8"), sandbox, { filename: rel });
  }
  return { U: sandbox.SFWFeedUpdate, C: sandbox.SFWFeedConfig, store, calls, listeners, sandbox };
}

const ok = (text, extra = {}) => ({ ok: true, status: 200, url: extra.url, headers: { get: (h) => (h.toLowerCase() === "content-length" && extra.length !== undefined ? String(extra.length) : null) }, body: null, text: async () => text });
const streamOf = (chunks, extra = {}) => ({
  ok: true, status: 200, url: extra.url,
  headers: { get: () => null },
  body: { getReader: () => { let i = 0; let cancelled = false; return { read: async () => (i < chunks.length ? { done: false, value: chunks[i++] } : { done: true }), cancel: async () => { cancelled = true; extra.onCancel?.(); } }; } },
});
const bad = (status) => ({ ok: false, status, url: undefined, headers: { get: () => null }, body: null, text: async () => "" });

test("the request is anonymous, uncached, bounded to 10 s, and goes to the one configured HTTPS URL", async () => {
  const w = world({ respond: async () => bad(404) });
  await w.U.check();
  const [call] = w.calls.fetch;
  assert.equal(call.url, w.C.FEED_URL);
  assert.match(call.url, /^https:\/\/raw\.githubusercontent\.com\/[^?#]*$/);
  assert.equal(call.init.cache, "no-store");
  assert.equal(call.init.credentials, "omit");
  assert.equal(call.init.referrerPolicy, "no-referrer");
  assert.equal(call.init.method, undefined, "plain GET");
  assert.equal(call.init.headers, undefined, "no custom headers");
  assert.equal(typeof call.init.signal.aborted, "boolean", "an AbortSignal is passed");
  const timer = w.calls.timers.find((t) => t.ms === 10000);
  assert.ok(timer, "a 10 s timeout is armed");
  assert.equal(timer.live, false, "and cleared when the request finished");
  assert.equal(w.C.CHECK_EVERY_MINUTES, 720);
});

test("the timeout aborts a request that never answers and is reported as 'timeout'", async () => {
  let abortSignal;
  const w = world({ respond: (url, init) => new Promise((_, reject) => { abortSignal = init.signal; init.signal.addEventListener("abort", () => reject(new Error("aborted"))); }) });
  const pending = w.U.check();
  await new Promise((r) => setImmediate(r));
  const timer = w.calls.timers.find((t) => t.ms === 10000 && t.live);
  assert.ok(timer);
  timer.fn(); // 10 s pass
  const r = await pending;
  assert.deepEqual([r.state, r.code], ["unavailable", "timeout"]);
  assert.equal(abortSignal.aborted, true);
  assert.equal(w.store.feedStatus.state, "unavailable");
  assert.equal(w.store.feed, undefined);
});

test("a good feed is verified and stored; the same seq is a no-op; a lower seq is rejected and nothing is overwritten", async () => {
  let body = feedText(3);
  const w = world({ manifest: {}, stored: { feedDev: { url: "http://127.0.0.1:9/feed.json", key: TEST_PUB } }, respond: async () => ok(body) });
  let r = await w.U.check();
  assert.deepEqual([r.state, r.seq, r.updated], ["ok", 3, true]);
  assert.equal(w.store.feed.seq, 3);
  assert.equal(w.store.feed.lexicon.add.entityStrong[0], "zz fake name");
  assert.equal(w.store.feedStatus.state, "ok");
  const stamp = w.store.feed.fetchedAt;
  r = await w.U.check();
  assert.deepEqual([r.state, r.updated], ["ok", false]);
  assert.equal(w.store.feed.fetchedAt, stamp);
  body = feedText(2);
  r = await w.U.check();
  assert.deepEqual([r.state, r.code], ["rejected", "rollback"]);
  assert.equal(w.store.feed.seq, 3);
  assert.equal(w.store.feedStatus.state, "rejected");
  body = feedText(4);
  assert.equal((await w.U.check()).updated, true);
  assert.equal(w.store.feed.seq, 4);
});

test("production URL and production keys are what a store build uses; a rejected feed under them leaves the store alone", async () => {
  const w = world({ manifest: { update_url: "https://clients2.google.com/service/update2/crx" }, stored: { feedDev: { url: "http://127.0.0.1:9/feed.json", key: TEST_PUB } }, respond: async () => ok(feedText(3)) });
  const r = await w.U.check();
  assert.equal(w.calls.fetch[0].url, w.C.FEED_URL, "a store build ignores the override even when one is stored");
  assert.deepEqual([r.state, r.code], ["rejected", "signature"], "the test key is not trusted by a store build");
  assert.equal(w.store.feed, undefined);
});

test("devOverride is inert unless this is an unpacked build AND the URL is plain-http loopback", () => {
  const dev = world({ respond: async () => bad(500) }).U;
  const url = (u) => dev.devOverride({ feedDev: { url: u, key: "AAAA" } });
  assert.ok(url("http://127.0.0.1:8080/feed.json"));
  assert.ok(url("http://localhost:8080/feed.json"));
  for (const u of ["https://127.0.0.1/feed.json", "http://127.0.0.1.evil.example/feed.json", "http://evil.example/feed.json", "https://raw.githubusercontent.com/x", "http://[::1]:80/feed.json", "file:///etc/passwd", "javascript:1", "", "not a url"]) {
    assert.equal(url(u), null, u);
  }
  assert.equal(dev.devOverride({}), null);
  assert.equal(dev.devOverride({ feedDev: "http://127.0.0.1/" }), null);
  assert.equal(dev.devOverride({ feedDev: { url: "http://127.0.0.1/feed.json" } }), null, "no key");
  const store = world({ manifest: { update_url: "x" }, respond: async () => bad(500) }).U;
  assert.equal(store.devOverride({ feedDev: { url: "http://127.0.0.1:8080/feed.json", key: "AAAA" } }), null, "store build");
});

test("status handling: 404 is 'none', other HTTP errors and network failures are 'unavailable', and none of them touch the stored feed", async () => {
  const mode = { v: bad(404) };
  const w = world({ stored: { feed: { seq: 5, issued: "2026-11-17", expires: "2099-01-01", lexicon: {} } }, respond: async () => { if (mode.v === "throw") throw new TypeError("Failed to fetch"); return mode.v; } });
  const run = async (m) => { mode.v = m; return w.U.check(); };
  assert.deepEqual(await run(bad(404)).then((r) => [r.state, r.code]), ["none", "http_404"]);
  assert.deepEqual(await run(bad(500)).then((r) => [r.state, r.code]), ["unavailable", "http_500"]);
  assert.deepEqual(await run(bad(429)).then((r) => [r.state, r.code]), ["unavailable", "http_429"]);
  assert.deepEqual(await run("throw").then((r) => [r.state, r.code]), ["unavailable", "network"]);
  assert.equal(w.store.feed.seq, 5);
  assert.equal(w.store.feedStatus.state, "unavailable");
});

test("byte caps: a declared or streamed body over 256 KiB is cut off and rejected, never buffered whole", async () => {
  const cap = 262144;
  let cancelled = 0;
  const w = world({ respond: async () => ok("x", { length: cap + 1 }) });
  assert.deepEqual(await w.U.check().then((r) => [r.state, r.code]), ["rejected", "size"], "Content-Length over the cap");
  const w2 = world({ respond: async () => streamOf([new Uint8Array(100000), new Uint8Array(100000), new Uint8Array(100000), new Uint8Array(100000)], { onCancel: () => cancelled++ }) });
  assert.deepEqual(await w2.U.check().then((r) => [r.state, r.code]), ["rejected", "size"], "streamed body over the cap");
  assert.equal(cancelled, 1, "the stream is cancelled as soon as the cap is crossed");
  assert.equal(w2.store.feed, undefined);
  // exactly at the cap is read in full (and then fails as an envelope, not as a size)
  const w3 = world({ respond: async () => streamOf([new Uint8Array(cap).fill(32)]) });
  assert.deepEqual(await w3.U.check().then((r) => [r.state, r.code]), ["rejected", "envelope"]);
});

test("a redirect that ends on plain http (outside the dev override) is refused", async () => {
  const w = world({ respond: async () => ok(feedText(3), { url: "http://evil.example/feed.json" }) });
  assert.deepEqual(await w.U.check().then((r) => [r.state, r.code]), ["rejected", "insecure"]);
  assert.equal(w.store.feed, undefined);
});

test("the switch: off means no request at all; on is the default; one check at a time", async () => {
  let n = 0;
  let release;
  const w = world({ stored: { feedEnabled: false }, respond: () => { n++; return new Promise((res) => (release = () => res(bad(404)))); } });
  assert.equal(JSON.stringify(await w.U.check()), '{"state":"off"}');
  assert.equal(n, 0);
  const w2 = world({ respond: () => { n++; return new Promise((res) => (release = () => res(bad(404)))); } });
  const a = w2.U.check();
  const b = w2.U.check();
  await new Promise((r) => setImmediate(r));
  assert.equal(n, 1, "overlapping triggers share one request");
  release();
  assert.equal(await a, await b);
});

test("scheduling: a 12-hour repeating alarm, created once; startup and install trigger a check", async () => {
  const w = world({ respond: async () => bad(404) });
  w.U.start();
  await new Promise((r) => setImmediate(r));
  assert.equal(w.calls.alarms.length, 1);
  assert.deepEqual({ ...w.calls.alarms[0] }, { name: "sfw-feed", delayInMinutes: 720, periodInMinutes: 720 });
  await w.U.ensureAlarm();
  assert.equal(w.calls.alarms.length, 1, "an existing alarm is kept");
  assert.equal(typeof w.listeners.startup, "function");
  assert.equal(typeof w.listeners.installed, "function");
  assert.equal(typeof w.listeners.alarm, "function");
  w.listeners.alarm({ name: "something-else" });
  assert.equal(w.calls.fetch.length, 0);
  w.listeners.alarm({ name: "sfw-feed" });
  await new Promise((r) => setImmediate(r));
  assert.equal(w.calls.fetch.length, 1);
});
