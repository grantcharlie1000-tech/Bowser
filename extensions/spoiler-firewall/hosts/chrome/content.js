// Chrome host shim (isolated world). Concatenated by build/build.mjs after the engine inside one
// closure, so `SFW` is in scope. Everything here is wrapped in a function: the engine and this
// file share one scope and must not collide on top-level names.
//
// Rules this file enforces:
//   * Config is usable synchronously (defaults: enabled, strict); chrome.storage only refines it.
//   * Nothing here can unshield: failures keep the last known config and keep protecting.
//   * report() sends category counters only. Page text never reaches the service worker.
function startSpoilerFirewallChromeHost() {
  var DEFAULTS = { enabled: true, mode: "strict", reducedEffects: false, ocr: false };
  var MODES = ["lockdown", "strict", "balanced"];
  var KEYS = ["enabled", "mode", "reducedEffects", "ocr", "feed"];
  var OCR_MAX_BLOB = 4 * 1024 * 1024; // blob: images are read here, in the page's origin, and sent as base64
  var OCR_BLOB_TIMEOUT_MS = 8000;
  var FLUSH_MS = 1000;
  var CONFIG_TIMEOUT_MS = 2500;
  var MAX_RETRIES = 5;

  var cb = typeof chrome === "object" && chrome ? chrome : null;
  var version = "0";
  try { version = cb.runtime.getManifest().version; } catch (e) { /* keep default */ }

  var config = { enabled: DEFAULTS.enabled, mode: DEFAULTS.mode, reducedEffects: DEFAULTS.reducedEffects, ocr: DEFAULTS.ocr, version: version };
  var configSettled = false;
  var listeners = [];
  var feedSeq = 0; // highest term-update seq applied in this page
  var contextAlive = true;

  function alive() {
    if (!contextAlive) return false;
    try {
      if (cb && cb.runtime && cb.runtime.id) return true;
    } catch (e) { /* fall through */ }
    contextAlive = false; // orphaned after an update/uninstall: keep protecting with last config
    return false;
  }

  // Merge stored values over the current config. Malformed values never loosen protection:
  // a bad mode keeps the current mode; "enabled" is false only when it is exactly false.
  function applyStored(values) {
    if (!values || typeof values !== "object") return false;
    var changed = false;
    if ("enabled" in values) {
      var enabled = values.enabled !== false;
      if (enabled !== config.enabled) { config.enabled = enabled; changed = true; }
    }
    if ("mode" in values && MODES.indexOf(values.mode) >= 0 && values.mode !== config.mode) {
      config.mode = values.mode; changed = true;
    }
    if ("reducedEffects" in values) {
      var reduced = values.reducedEffects === true;
      if (reduced !== config.reducedEffects) { config.reducedEffects = reduced; changed = true; }
    }
    // "ocr" mirrors the options-page switch, which is only turned on after the optional permissions were granted;
    // the service worker re-checks the permissions on every request, so a stale true can only produce "unavailable".
    if ("ocr" in values) {
      var ocr = values.ocr === true;
      if (ocr !== config.ocr) { config.ocr = ocr; changed = true; }
    }
    if ("feed" in values && applyFeed(values.feed)) changed = true;
    return changed;
  }

  // Term update: the service worker stores a feed only after verifying its signature (feed.js); extend() then only
  // ever ADDS terms or raises a term's category, and ignores anything malformed, so a bad or stale value here can
  // never loosen protection. Called before configReady resolves, i.e. before the first classification is released.
  function applyFeed(feed) {
    try {
      if (!feed || typeof feed !== "object" || typeof feed.seq !== "number" || !(feed.seq > feedSeq)) return false;
      if (!feed.lexicon || typeof feed.lexicon !== "object" || !SFW.lexicon || typeof SFW.lexicon.extend !== "function") return false;
      var res = SFW.lexicon.extend(feed.lexicon);
      if (res && res.ok) { feedSeq = feed.seq; return res.added > 0; }
    } catch (e) { /* keep protecting with what is loaded */ }
    return false;
  }

  function notify() {
    for (var i = 0; i < listeners.length; i++) {
      try { listeners[i](config); } catch (e) { /* one listener must not break the others */ }
    }
  }

  var configReady = new Promise(function (resolve) {
    var settled = false;
    function done() { if (!settled) { settled = true; configSettled = true; resolve(config); } }
    // Defaults are protective, so never leave units pending forever if storage stalls.
    var timer = setTimeout(done, CONFIG_TIMEOUT_MS);
    try {
      cb.storage.local.get(KEYS).then(function (values) {
        clearTimeout(timer);
        if (applyStored(values)) notify();
        done();
      }, function () { clearTimeout(timer); done(); });
    } catch (e) { clearTimeout(timer); done(); }
  });

  try {
    cb.storage.onChanged.addListener(function (changes, area) {
      if (area !== "local" || !changes) return;
      var next = {};
      for (var i = 0; i < KEYS.length; i++) {
        var key = KEYS[i];
        if (changes[key]) next[key] = changes[key].newValue === undefined ? DEFAULTS[key] : changes[key].newValue;
      }
      if (applyStored(next)) notify();
    });
  } catch (e) { /* no storage events: keep last config */ }

  // ---- counters: category code -> count, flushed in batches ------------------------------
  var pending = Object.create(null);
  var retries = 0;
  var timerId = 0;

  function schedule() {
    if (!timerId) timerId = setTimeout(flush, FLUSH_MS);
  }

  function report(code, n) {
    if (typeof code !== "string" || !code || code.length > 40) return;
    var add = typeof n === "number" && isFinite(n) && n > 0 ? Math.floor(n) : 1;
    pending[code] = (pending[code] || 0) + add;
    schedule();
  }

  function flush() {
    timerId = 0;
    var counts = pending;
    var any = false;
    for (var k in counts) { any = true; break; }
    if (!any) return;
    pending = Object.create(null);
    if (!alive()) return; // orphaned: drop counters, protection continues
    var retry = function () {
      if (retries++ >= MAX_RETRIES) { retries = 0; return; }
      for (var key in counts) pending[key] = (pending[key] || 0) + counts[key];
      schedule();
    };
    var invalidated = function (err) {
      if (/context invalidated/i.test(String((err && err.message) || err))) { contextAlive = false; return true; }
      return false;
    };
    try {
      var p = cb.runtime.sendMessage({ t: "report", counts: counts });
      if (p && p.then) p.then(function () { retries = 0; }, function (err) { if (!invalidated(err)) retry(); });
    } catch (e) {
      if (!invalidated(e)) retry();
    }
  }

  try {
    document.addEventListener("visibilitychange", function () {
      if (document.visibilityState === "hidden") flush();
    }, true);
  } catch (e) { /* ignore */ }

  // ---- local OCR (optional; see hosts/chrome/offscreen.js) -----------------------------------
  // request(req) -> Promise<{status:"ok", text} | {status:"unavailable"|"error"|"skipped"}>. http(s) URLs are sent
  // as-is (the offscreen document fetches them with credentials omitted); data: URLs too; blob: URLs only exist in
  // this origin, so their bytes are read here. Nothing is logged and no text is kept.
  function toBase64(bytes) {
    var out = "";
    for (var i = 0; i < bytes.length; i += 0x8000) out += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
    return btoa(out);
  }
  function readBlobUrl(url) {
    var ctl = new AbortController();
    var timer = setTimeout(function () { ctl.abort(); }, OCR_BLOB_TIMEOUT_MS);
    return fetch(url, { signal: ctl.signal }).then(function (res) { return res.blob(); }).then(function (blob) {
      clearTimeout(timer);
      if (blob.size > OCR_MAX_BLOB) return { skip: true };
      if (!/^image\//i.test(blob.type)) return { skip: true };
      return blob.arrayBuffer().then(function (buf) { return { mime: blob.type, b64: toBase64(new Uint8Array(buf)) }; });
    }, function (e) { clearTimeout(timer); throw e; });
  }
  function normalizeOcr(r) {
    if (r && typeof r === "object") {
      if (r.status === "ok" && typeof r.text === "string") return { status: "ok", text: r.text };
      if (r.status === "unavailable" || r.status === "skipped") return { status: r.status };
    }
    return { status: "error" };
  }
  var ocrApi = {
    request: function (req) {
      var url = req && typeof req.url === "string" ? req.url : "";
      if (!url || !alive()) return Promise.resolve({ status: "unavailable" });
      var base = { t: "ocr", url: url, kind: req.kind, width: req.width, height: req.height, naturalWidth: req.naturalWidth, naturalHeight: req.naturalHeight };
      var send = function (msg) {
        return cb.runtime.sendMessage(msg).then(normalizeOcr, function (err) {
          return /context invalidated/i.test(String((err && err.message) || err)) ? { status: "unavailable" } : { status: "error" };
        });
      };
      try {
        if (/^blob:/i.test(url)) {
          return readBlobUrl(url).then(function (r) {
            if (r.skip) return { status: "skipped" };
            base.url = "blob:"; // bytes travel in b64; the URL itself is not needed
            base.b64 = r.b64;
            base.mime = r.mime;
            return send(base);
          }, function () { return { status: "error" }; });
        }
        if (/^data:/i.test(url) && url.length > OCR_MAX_BLOB * 1.4) return Promise.resolve({ status: "skipped" });
        return send(base);
      } catch (e) {
        return Promise.resolve({ status: "error" });
      }
    },
  };

  var frameIsTop = false;
  try { frameIsTop = window.top === window; } catch (e) { frameIsTop = false; }

  var host = {
    kind: "chrome",
    config: config,
    configReady: configReady,
    // null until the user switched the scan on (options page) -> the engine behaves exactly as without OCR
    get ocr() { return config.ocr ? ocrApi : null; },
    // true while the stored settings are still loading: the engine holds media instead of releasing it early
    get ocrMaybe() { return !configSettled; },
    report: report,
    onConfigChange: function (fn) { if (typeof fn === "function") listeners.push(fn); },
    privileged: {
      openOrClosedShadowRoot: function (el) {
        try {
          if (cb && cb.dom && cb.dom.openOrClosedShadowRoot) return cb.dom.openOrClosedShadowRoot(el);
        } catch (e) { /* fall through */ }
        try { return el.shadowRoot || null; } catch (e2) { return null; }
      },
    },
    frameIsTop: frameIsTop,
  };

  // Tell the service worker a top-level page started: resets that tab's counter and doubles as a
  // "this page is protected" heartbeat for the popup. No page data is included.
  if (frameIsTop && alive()) {
    try {
      var hello = cb.runtime.sendMessage({ t: "page-start" });
      if (hello && hello.catch) hello.catch(function () { /* worker unavailable */ });
    } catch (e) { /* ignore */ }
  }

  try {
    if (!SFW || !SFW.runtime || typeof SFW.runtime.start !== "function") throw new Error("runtime missing");
    SFW.runtime.start(host);
  } catch (err) {
    // Fail closed: shield the whole page if the shield module is available, and report it.
    try { report("host_error"); } catch (e) { /* ignore */ }
    try {
      if (SFW && SFW.shield && typeof SFW.shield.apply === "function") {
        SFW.shield.apply(document.documentElement || document, { variant: "page" });
        report("page_shield_fallback");
      }
    } catch (e2) { /* nothing more can be done here */ }
  }
}

startSpoilerFirewallChromeHost();
