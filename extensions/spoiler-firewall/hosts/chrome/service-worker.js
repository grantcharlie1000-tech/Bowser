// Spoiler Firewall service worker (classic, not a module).
//
// Responsibilities, deliberately small:
//   * write default settings on first install,
//   * keep per-tab shield counters (counts only, never page text) in chrome.storage.session,
//   * show the count / "OFF" on the toolbar badge.
//   * (optional, user-enabled) coordinate local image-text detection: relay a content script's OCR
//     request to the offscreen document that hosts Tesseract (see offscreen.js).
//   * (default on, switchable) download the signed spoiler-term feed and keep the last verified copy in
//     chrome.storage.local (feed-update.js). The only network request this worker makes: one anonymous GET of a
//     small static file, every 12 hours, no cookies and no identifiers. It can only add protection.
// Detection runs entirely in the content scripts, so this worker may be suspended at any time
// without affecting protection. Images are fetched by the offscreen document, only after the user
// turned the scan on and granted the optional permissions.

"use strict";

importScripts("vendor/noble-ed25519/noble-ed25519.js", "feed-config.js", "feed.js", "feed-update.js");

var DEFAULT_SETTINGS = { enabled: true, mode: "strict", reducedEffects: false, ocr: false, feedEnabled: true };
var MODES = ["lockdown", "strict", "balanced"];
var TAB_KEY_PREFIX = "tab:";
var SHIELD_CODE = /^(shield|page_shield)/; // report codes counted as "shields on this page"
var CODE_PATTERN = /^[A-Za-z0-9_.:-]{1,40}$/;
var MAX_CODES = 64;
var MAX_COUNT = 1000000;
var BADGE_COLOR = "#5b3fd1";
var BADGE_OFF_COLOR = "#b3261e";

// Serialise read-modify-write cycles on session storage within this worker lifetime.
var queue = Promise.resolve();
function enqueue(task) {
  var run = queue.then(task, task);
  queue = run.catch(function () {});
  return run;
}

function tabKey(tabId) { return TAB_KEY_PREFIX + tabId; }

function shieldTotal(counts) {
  var total = 0;
  for (var code in counts) if (SHIELD_CODE.test(code)) total += counts[code];
  return total;
}

function badgeText(n) { return n <= 0 ? "" : n > 999 ? "999+" : String(n); }

function sanitizeCounts(raw) {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null;
  var out = {};
  var keys = Object.keys(raw);
  if (keys.length > MAX_CODES) return null;
  for (var i = 0; i < keys.length; i++) {
    var code = keys[i];
    var n = raw[code];
    if (!CODE_PATTERN.test(code) || typeof n !== "number" || !isFinite(n) || n < 1) return null;
    out[code] = Math.min(Math.floor(n), MAX_COUNT);
  }
  return out;
}

function isOwnMessage(sender) {
  if (!sender || sender.id !== chrome.runtime.id || !sender.tab || typeof sender.tab.id !== "number" || sender.tab.id < 0) return false;
  // Extension pages opened in a tab also carry sender.tab; only real content scripts (whose sender
  // URL is the web page, not chrome-extension://) may report.
  return typeof sender.url === "string" && sender.url.indexOf(chrome.runtime.getURL("")) !== 0;
}

async function isProtectionEnabled() {
  try {
    var v = await chrome.storage.local.get("enabled");
    return v.enabled !== false;
  } catch (e) {
    return true;
  }
}

async function paintBadge(tabId, text) {
  try {
    await chrome.action.setBadgeText({ tabId: tabId, text: text });
  } catch (e) { /* tab closed */ }
}

async function paintGlobalBadge() {
  var enabled = await isProtectionEnabled();
  try {
    await chrome.action.setBadgeBackgroundColor({ color: enabled ? BADGE_COLOR : BADGE_OFF_COLOR });
    if (typeof chrome.action.setBadgeTextColor === "function") await chrome.action.setBadgeTextColor({ color: "#ffffff" });
    await chrome.action.setBadgeText({ text: enabled ? "" : "OFF" });
    await chrome.action.setTitle({ title: enabled ? "Spoiler Firewall for GTA VI" : "Spoiler Firewall for GTA VI: protection is OFF" });
    // Tab-specific badge text overrides the global one, so refresh every tab too.
    var tabs = await chrome.tabs.query({});
    var session = await chrome.storage.session.get(null);
    for (var i = 0; i < tabs.length; i++) {
      var rec = session[tabKey(tabs[i].id)];
      await paintBadge(tabs[i].id, enabled ? badgeText(rec ? rec.shields : 0) : "OFF");
    }
  } catch (e) { /* ignore */ }
}

function handleReport(tabId, counts) {
  return enqueue(async function () {
    var key = tabKey(tabId);
    var stored = (await chrome.storage.session.get(key))[key] || { counts: {}, shields: 0 };
    for (var code in counts) stored.counts[code] = Math.min((stored.counts[code] || 0) + counts[code], MAX_COUNT);
    stored.shields = shieldTotal(stored.counts);
    await chrome.storage.session.set({ [key]: stored });
    if (await isProtectionEnabled()) await paintBadge(tabId, badgeText(stored.shields));
  });
}

function handlePageStart(tabId) {
  return enqueue(async function () {
    await chrome.storage.session.set({ [tabKey(tabId)]: { counts: {}, shields: 0, startedAt: Date.now() } });
    if (await isProtectionEnabled()) await paintBadge(tabId, "");
  });
}

// ---- local OCR relay ---------------------------------------------------------------------
var OFFSCREEN_URL = "offscreen.html";
var OCR_KINDS = { img: 1, poster: 1, bg: 1, frame: 1 };
var OCR_MAX_B64 = Math.ceil(4 * 1024 * 1024 * 4 / 3);
var OCR_MAX_DATA_URL = 6 * 1024 * 1024;
var OCR_TIMEOUT_MS = 55000;
var offscreenCreating = null;

function ocrAllowed() {
  return Promise.all([
    chrome.storage.local.get(["enabled", "ocr"]),
    chrome.permissions.contains({ permissions: ["offscreen"] }),
  ]).then(function (r) { return r[0].enabled !== false && r[0].ocr === true && r[1] === true && !!chrome.offscreen; }, function () { return false; });
}

function hasOffscreen() {
  if (chrome.runtime.getContexts) {
    return chrome.runtime.getContexts({ contextTypes: ["OFFSCREEN_DOCUMENT"] }).then(function (c) { return c.length > 0; });
  }
  return Promise.resolve(false);
}

function ensureOffscreen() {
  if (offscreenCreating) return offscreenCreating;
  offscreenCreating = hasOffscreen().then(function (exists) {
    if (exists) return;
    return chrome.offscreen.createDocument({
      url: OFFSCREEN_URL,
      reasons: ["WORKERS"],
      justification: "Run the bundled Tesseract OCR WebAssembly worker locally to read text inside images.",
    });
  }).then(function () { offscreenCreating = null; }, function (e) { offscreenCreating = null; throw e; });
  return offscreenCreating;
}

function sendToOffscreen(payload) {
  return chrome.runtime.sendMessage(payload);
}

function runOcr(payload) {
  return ensureOffscreen().then(function () { return sendToOffscreen(payload); }).then(function (r) {
    if (r === undefined) throw new Error("no answer");
    return r;
  }).catch(function () {
    // the offscreen document may have closed itself between the check and the send: recreate once
    return ensureOffscreen().then(function () { return sendToOffscreen(payload); });
  });
}

function validOcrMessage(m) {
  if (typeof m.url !== "string" || m.url.length > OCR_MAX_DATA_URL) return false;
  if (typeof m.b64 === "string") return m.b64.length <= OCR_MAX_B64 && typeof m.mime === "string" && /^image\//i.test(m.mime) && m.mime.length < 80;
  return /^(?:https?:|data:)/i.test(m.url);
}

function handleOcr(message) {
  if (!validOcrMessage(message)) return Promise.resolve({ status: "error" });
  return ocrAllowed().then(function (ok) {
    if (!ok) return { status: "unavailable" };
    var payload = { t: "ocr-run", kind: OCR_KINDS[message.kind] ? message.kind : "img" };
    if (typeof message.b64 === "string") { payload.b64 = message.b64; payload.mime = message.mime; payload.url = ""; } else payload.url = message.url;
    var timeout = new Promise(function (res) { setTimeout(function () { res({ status: "error" }); }, OCR_TIMEOUT_MS); });
    var run = runOcr(payload).then(function (r) {
      if (r && r.status === "ok" && typeof r.text === "string") return { status: "ok", text: r.text };
      if (r && r.status === "skipped") return { status: "skipped" };
      return { status: "error" };
    }, function () { return { status: "unavailable" }; });
    return Promise.race([run, timeout]);
  }, function () { return { status: "unavailable" }; });
}

chrome.runtime.onMessage.addListener(function (message, sender, sendResponse) {
  // Our own extension pages (the options page) may ask for an immediate term-update check.
  if (message && typeof message === "object" && message.t === "feed-check" && sender && sender.id === chrome.runtime.id &&
      typeof sender.url === "string" && sender.url.indexOf(chrome.runtime.getURL("")) === 0) {
    SFWFeedUpdate.check().then(sendResponse, function () { sendResponse({ state: "unavailable" }); });
    return true;
  }
  // Only our own content scripts may talk to us. No externally_connectable, no onMessageExternal.
  if (!isOwnMessage(sender) || !message || typeof message !== "object") return false;
  if (message.t === "ocr") {
    handleOcr(message).then(sendResponse, function () { sendResponse({ status: "error" }); });
    return true;
  }
  if (message.t === "report") {
    var counts = sanitizeCounts(message.counts);
    if (!counts) return false;
    handleReport(sender.tab.id, counts).then(function () { sendResponse({ ok: true }); }, function () { sendResponse({ ok: false }); });
    return true;
  }
  if (message.t === "page-start" && sender.frameId === 0) {
    handlePageStart(sender.tab.id).then(function () { sendResponse({ ok: true }); }, function () { sendResponse({ ok: false }); });
    return true;
  }
  return false;
});

chrome.runtime.onInstalled.addListener(async function (details) {
  try {
    // Only fill in missing keys: an update must never reset a user's mode or turn protection back
    // on or off behind their back.
    var existing = await chrome.storage.local.get(Object.keys(DEFAULT_SETTINGS));
    var missing = {};
    for (var k in DEFAULT_SETTINGS) if (!(k in existing)) missing[k] = DEFAULT_SETTINGS[k];
    if (existing.mode !== undefined && MODES.indexOf(existing.mode) < 0) missing.mode = DEFAULT_SETTINGS.mode;
    if (Object.keys(missing).length) await chrome.storage.local.set(missing);
    await paintGlobalBadge();
    if (details.reason === "install") {
      // One-time, non-repeating: tells the user that tabs open before install need a reload.
      await chrome.tabs.create({ url: chrome.runtime.getURL("options.html?welcome=1") });
    }
  } catch (e) { /* never block install */ }
});

chrome.storage.onChanged.addListener(function (changes, area) {
  if (area !== "local") return;
  if (changes.enabled) paintGlobalBadge();
  if (changes.feedEnabled && changes.feedEnabled.newValue !== false && changes.feedEnabled.oldValue === false) SFWFeedUpdate.check();
  // scan switched off (or protection off): free the scanner's memory right away
  if ((changes.ocr && changes.ocr.newValue !== true) || (changes.enabled && changes.enabled.newValue === false)) {
    try { if (chrome.offscreen) chrome.offscreen.closeDocument().catch(function () {}); } catch (e) { /* none open */ }
  }
});

// The user revoked the scan's permission (chrome://extensions): the setting follows, so the UI and the content
// scripts never claim a scan that cannot run.
if (chrome.permissions && chrome.permissions.onRemoved) {
  chrome.permissions.onRemoved.addListener(function (removed) {
    if (removed && removed.permissions && removed.permissions.indexOf("offscreen") >= 0) chrome.storage.local.set({ ocr: false });
  });
}

chrome.tabs.onRemoved.addListener(function (tabId) {
  enqueue(function () { return chrome.storage.session.remove(tabKey(tabId)); }).catch(function () {});
});

// Re-apply the global badge whenever the worker (re)starts, e.g. after a browser restart.
paintGlobalBadge();
// Term updates: install / startup / 12-hour alarm triggers (the listeners are registered synchronously, here).
SFWFeedUpdate.start();
