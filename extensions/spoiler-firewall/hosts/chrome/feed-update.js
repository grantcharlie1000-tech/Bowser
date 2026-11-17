// Term-update feed: download, verify, store. Classic script, importScripts()-ed by service-worker.js after
// vendor/noble-ed25519/noble-ed25519.js, feed-config.js and feed.js.
//
// What leaves the browser: one anonymous GET of a small static file (no cookies, no referrer, no query string,
// no identifiers). What comes back is only ever applied after SFWFeed.verifyEnvelope accepted it; anything else is
// ignored, and the last good feed (or the built-in list alone) keeps protecting. Nothing here can remove a term.
//
// Storage keys (chrome.storage.local):
//   feedEnabled  boolean, default true: the "Download spoiler term updates" switch
//   feed         the last verified feed {seq, issued, expires, lexicon, fetchedAt}; content scripts apply it
//   feedStatus   {state: "ok"|"none"|"unavailable"|"rejected", code, at} describing the latest attempt
//   feedDev      TEST ONLY {url, key}: honoured only in an unpacked build and only for a loopback http URL (see devOverride)
var SFWFeedUpdate = (function () {
  "use strict";

  var cfg = SFWFeedConfig;
  var F = SFWFeed;
  var ALARM = "sfw-feed";
  var inflight = null;

  function loopbackUrl(text) {
    try {
      var u = new URL(text);
      return u.protocol === "http:" && (u.hostname === "127.0.0.1" || u.hostname === "localhost");
    } catch (e) { return false; }
  }

  // A development build (loaded unpacked) has no update_url in its manifest; a Chrome Web Store install always does.
  // Only then, and only for a loopback http URL, may the test override replace the URL and the trusted key. The value
  // lives in chrome.storage.local, which web pages cannot reach, and the content scripts never write it.
  function isDevBuild() {
    try { return !("update_url" in chrome.runtime.getManifest()); } catch (e) { return false; }
  }
  function devOverride(stored) {
    var dev = stored && stored.feedDev;
    if (!isDevBuild() || !dev || typeof dev !== "object" || typeof dev.url !== "string" || typeof dev.key !== "string") return null;
    if (!loopbackUrl(dev.url)) return null;
    return { url: dev.url, keys: [dev.key] };
  }

  function setStatus(state, code) {
    return chrome.storage.local.set({ feedStatus: { state: state, code: code || "", at: Date.now() } });
  }

  // Reads the body with a hard byte cap. Resolves the text, or throws Error("size").
  async function readCapped(res) {
    var declared = Number(res.headers.get("content-length"));
    if (isFinite(declared) && declared > F.MAX_BYTES) throw new Error("size");
    var reader = res.body && res.body.getReader ? res.body.getReader() : null;
    if (!reader) {
      var whole = await res.text();
      if (whole.length > F.MAX_BYTES) throw new Error("size");
      return whole;
    }
    var chunks = [];
    var total = 0;
    for (;;) {
      var step = await reader.read();
      if (step.done) break;
      total += step.value.length;
      if (total > F.MAX_BYTES) {
        try { await reader.cancel(); } catch (e) { /* already closing */ }
        throw new Error("size");
      }
      chunks.push(step.value);
    }
    var bytes = new Uint8Array(total);
    var at = 0;
    for (var i = 0; i < chunks.length; i++) { bytes.set(chunks[i], at); at += chunks[i].length; }
    return new TextDecoder("utf-8").decode(bytes);
  }

  async function download(url, allowLoopback) {
    var ctl = new AbortController();
    var timer = setTimeout(function () { ctl.abort(); }, cfg.FETCH_TIMEOUT_MS);
    try {
      var res = await fetch(url, { cache: "no-store", credentials: "omit", redirect: "follow", referrerPolicy: "no-referrer", signal: ctl.signal });
      var secure = /^https:/i.test(res.url || url) || (allowLoopback && loopbackUrl(res.url || url));
      if (!secure) return { kind: "rejected", code: "insecure" };
      if (res.status === 404) return { kind: "none", code: "http_404" };
      if (!res.ok) return { kind: "unavailable", code: "http_" + res.status };
      return { kind: "body", text: await readCapped(res) };
    } catch (e) {
      if (e && e.message === "size") return { kind: "rejected", code: "size" };
      return { kind: "unavailable", code: ctl.signal.aborted ? "timeout" : "network" };
    } finally {
      clearTimeout(timer);
    }
  }

  async function run() {
    var stored = await chrome.storage.local.get(["feedEnabled", "feed", "feedDev"]);
    if (stored.feedEnabled === false) return { state: "off" };
    var dev = devOverride(stored);
    var url = dev ? dev.url : cfg.FEED_URL;
    var keys = dev ? dev.keys : cfg.PUBLIC_KEYS;
    var got = await download(url, !!dev);
    if (got.kind !== "body") { await setStatus(got.kind, got.code); return { state: got.kind, code: got.code }; }
    var highest = stored.feed && typeof stored.feed.seq === "number" ? stored.feed.seq : 0;
    var result = await F.verifyEnvelope(got.text, keys, { highestSeq: highest });
    if (!result.ok) { await setStatus("rejected", result.code); return { state: "rejected", code: result.code }; }
    if (!result.current) {
      await chrome.storage.local.set({ feed: { seq: result.feed.seq, issued: result.feed.issued, expires: result.feed.expires, lexicon: result.feed.lexicon, fetchedAt: Date.now() } });
    }
    await setStatus("ok", result.current ? "current" : "updated");
    return { state: "ok", seq: result.feed.seq, updated: !result.current };
  }

  // One check at a time; never rejects.
  function check() {
    if (!inflight) {
      inflight = run().catch(function () { return { state: "unavailable", code: "error" }; }).then(function (r) { inflight = null; return r; });
    }
    return inflight;
  }

  function ensureAlarm() {
    try {
      if (!chrome.alarms) return Promise.resolve();
      return chrome.alarms.get(ALARM).then(function (existing) {
        if (!existing) return chrome.alarms.create(ALARM, { delayInMinutes: cfg.CHECK_EVERY_MINUTES, periodInMinutes: cfg.CHECK_EVERY_MINUTES });
      }).catch(function () { /* alarms unavailable: the startup check still runs */ });
    } catch (e) { return Promise.resolve(); }
  }

  function start() {
    try {
      if (chrome.alarms && chrome.alarms.onAlarm) chrome.alarms.onAlarm.addListener(function (a) { if (a && a.name === ALARM) check(); });
      chrome.runtime.onStartup.addListener(function () { ensureAlarm(); check(); });
      chrome.runtime.onInstalled.addListener(function () { ensureAlarm(); check(); });
    } catch (e) { /* listeners are best effort */ }
    ensureAlarm();
  }

  return { check: check, start: start, ensureAlarm: ensureAlarm, devOverride: devOverride, ALARM: ALARM };
})();
