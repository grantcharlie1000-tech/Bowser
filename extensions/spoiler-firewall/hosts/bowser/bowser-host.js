// Bowser host: boots the engine in the main world of every document (normal, private, guest; top frame and
// child frames) via AddScriptToExecuteOnDocumentCreated. Bowser's native code replaces the literal placeholder
// below with a JSON object {enabled, mode, reducedEffects, version, private, ocr[, ocrHook, ocrKey, debugReports][, feed]}
// before injection (feed: the verified term-update lexicon object, see below).
// Exposes nothing on the page except one non-enumerable idempotency guard and, only when native OCR is on, one
// non-enumerable, non-writable, non-configurable property named config.ocrHook (a frozen {drain, deliver}).
const CONFIG = /*__SFW_CONFIG__*/null;
{
  const win = globalThis;
  const GUARD = Symbol.for("sfw.bowser.guard");
  let already = false;
  try {
    already = !!win[GUARD];
    if (!already) Object.defineProperty(win, GUARD, { value: true, enumerable: false, configurable: false, writable: false });
  } catch (e) { /* a frozen window cannot be marked; fall through and start once per closure */ }
  if (!already) {
    const config = CONFIG && typeof CONFIG === "object" ? CONFIG : { enabled: true, mode: "strict" };
    const debug = !!config.debugReports;
    const noop = function () {};

    // ---- live term update ----------------------------------------------------------------------------------
    // A term update verified after this document loaded is pushed by Bowser through the per-tab hook (feed(key, json)):
    // extend() (adds or raises only) and then the runtime's config-change path, which rescans when the lexicon version
    // changed. The same feed (compared as normalised JSON) is never applied twice, so repeated pushes cannot fill the term cap.
    const appliedFeeds = new Set();
    let configCallback = null;
    const normFeed = function (obj) { try { return JSON.stringify(obj); } catch (e) { return null; } };
    const applyFeed = function (obj) {
      const sig = normFeed(obj);
      if (sig === null || appliedFeeds.has(sig)) return false;
      appliedFeeds.add(sig);
      try { SFW.lexicon.extend(obj); } catch (e) { return false; }
      try { if (configCallback) configCallback(config); } catch (e) { /* the next mutation still classifies with the new terms */ }
      return true;
    };

    // ---- native OCR bridge ------------------------------------------------------------------------------
    // Bowser's native side pulls requests with ExecuteScript(window[hook].drain(key)) and pushes answers with
    // window[hook].deliver(key, json). The hook name and the key are per-tab random (128 bits each) and the key
    // must accompany every call, so a page that lists its own property names still cannot drain requests or
    // forge answers. Page-facing surface is nothing but that one property. Never auto-reveals: a request that
    // is not answered settles "error" after 30 s and the runtime decides how to show that.
    const makeOcr = function () {
      const hook = config.ocrHook;
      const key = config.ocrKey;
      const ocrOn = config.ocr === true;
      if (typeof hook !== "string" || typeof key !== "string") return null;
      if (!/^__sfw_[0-9a-f]{32}$/.test(hook) || !/^[0-9a-f]{32}$/.test(key)) return null;
      const MAX_PENDING = 256, MAX_DRAIN = 64, MAX_URL = 2048, MAX_TEXT = 4096, MAX_JSON = 262144, TIMEOUT_MS = 30000;
      const STATUSES = { ok: 1, unavailable: 1, error: 1, skipped: 1 };
      // Originals are captured now, before any page script can replace them.
      const parseJson = JSON.parse.bind(JSON);
      const stringify = JSON.stringify.bind(JSON);
      const MapCtor = Map;
      const UrlCtor = win.URL;
      const setTimer = win.setTimeout.bind(win);
      const clearTimer = win.clearTimeout.bind(win);
      const isArray = Array.isArray;
      const pending = new MapCtor();   // id -> { resolve, timer, url, handedOut }
      const queue = [];                // ids not yet handed to native
      let counter = 0;

      const settle = function (id, result) {
        const entry = pending.get(id);
        if (!entry) return;
        pending.delete(id);
        try { clearTimer(entry.timer); } catch (e) { /* ignore */ }
        try { entry.resolve(result); } catch (e) { /* ignore */ }
      };

      const request = function (req) {
        return new Promise(function (resolve) {
          let url = "";
          try {
            const raw = req && req.url;
            if (typeof raw !== "string" || raw.length === 0 || raw.length > MAX_URL * 4) { resolve({ status: "unavailable" }); return; }
            // Only absolute http(s) is ever sent: data:, blob:, javascript: and the like are answered here.
            const parsed = new UrlCtor(raw, win.location.href);
            if (parsed.protocol !== "http:" && parsed.protocol !== "https:") { resolve({ status: "unavailable" }); return; }
            parsed.hash = "";
            url = parsed.href;
          } catch (e) { resolve({ status: "unavailable" }); return; }
          if (url.length > MAX_URL || pending.size >= MAX_PENDING) { resolve({ status: "unavailable" }); return; }
          counter += 1;
          const id = "r" + counter;
          const timer = setTimer(function () { settle(id, { status: "error" }); }, TIMEOUT_MS);
          pending.set(id, { resolve: resolve, timer: timer, url: url, handedOut: false });
          queue.push(id);
        });
      };

      // Up to 64 not-yet-handed-out requests as a JSON string, oldest first.
      const drain = function (k) {
        if (k !== key) return null;
        const items = [];
        while (queue.length > 0 && items.length < MAX_DRAIN) {
          const id = queue.shift();
          const entry = pending.get(id);
          if (!entry || entry.handedOut) continue;
          entry.handedOut = true;
          items.push({ id: id, url: entry.url });
        }
        return stringify(items);
      };

      // Answers from native: [{id, status, text?}]. Everything is validated; unknown ids are ignored.
      const deliver = function (k, json) {
        if (k !== key || typeof json !== "string" || json.length > MAX_JSON) return;
        let list;
        try { list = parseJson(json); } catch (e) { return; }
        if (!isArray(list)) return;
        const n = list.length < MAX_PENDING ? list.length : MAX_PENDING;
        for (let i = 0; i < n; i++) {
          const item = list[i];
          if (!item || typeof item !== "object" || typeof item.id !== "string" || !pending.has(item.id)) continue;
          const status = item.status;
          if (typeof status !== "string" || STATUSES[status] !== 1) { settle(item.id, { status: "error" }); continue; }
          if (status === "ok") {
            const text = item.text;
            if (typeof text !== "string" || text.length > MAX_TEXT) { settle(item.id, { status: "error" }); continue; }
            settle(item.id, { status: "ok", text: text });
          } else {
            settle(item.id, { status: status });
          }
        }
      };

      // Bowser pushes a term update here: a JSON text of at most 256 KB holding the feed's lexicon object.
      const feed = function (k, json) {
        if (k !== key || typeof json !== "string" || json.length > MAX_JSON) return;
        let obj;
        try { obj = parseJson(json); } catch (e) { return; }
        if (!obj || typeof obj !== "object" || isArray(obj)) return;
        applyFeed(obj);
      };

      try {
        const api = Object.freeze({ drain: drain, deliver: deliver, feed: feed });
        Object.defineProperty(win, hook, { value: api, enumerable: false, writable: false, configurable: false });
      } catch (e) { return null; } // a frozen window or a taken name: no hook, so no OCR
      // flight: native keeps its own queue and drains up to MAX_DRAIN per round, so the runtime may keep 12 requests
      // in flight per frame instead of its default 3.
      return ocrOn ? Object.freeze({ request: request, flight: 12 }) : null; // hook only (feed pushes) when OCR is off
    };
    let ocr = null;
    try { ocr = makeOcr(); } catch (e) { ocr = null; }
    // ------------------------------------------------------------------------------------------------------

    // Signed term update (src/spoiler_feed.rs): the native side verified the Ed25519 signature and embedded only the
    // feed's `lexicon` object as config.feed. extend() can only add terms or raise a term's category, validates every
    // entry, and ignores anything malformed, so a missing, stale or odd value leaves the built-in list protecting.
    try {
      if (config.feed && typeof config.feed === "object") {
        const sig = normFeed(config.feed);
        if (sig !== null) appliedFeeds.add(sig);
        SFW.lexicon.extend(config.feed);
      }
    } catch (e) { /* built-in list only */ }

    try { SFW.hooks.install(win); } catch (e) { /* hooks are best effort */ }
    SFW.runtime.start({
      kind: "bowser",
      config: config,
      configReady: null,
      ocr: ocr,
      report: debug ? function (code, n) { try { console.debug("[sfw]", String(code), n | 0); } catch (e) { /* ignore */ } } : noop,
      onConfigChange: function (cb) { configCallback = cb; },
      privileged: null,
      frameIsTop: win.top === win,
    });
  }
}
