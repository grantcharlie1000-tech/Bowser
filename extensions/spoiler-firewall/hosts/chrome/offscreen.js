// Offscreen document: local OCR (Tesseract.js + WebAssembly, all files bundled under vendor/tesseract).
//
// The service worker creates this document lazily (reason WORKERS) and sends it one message per image:
//     { t: "ocr-run", url | dataUrl | b64+mime, kind, width, height, naturalWidth, naturalHeight }
// The reply is { status: "ok", text } | { status: "skipped" } | { status: "error" }. Page text is never logged,
// stored, or sent anywhere: it only goes back to the service worker, which hands it to the content script.
//
// Pipeline: cache lookup -> fetch bytes (credentials omitted, 8 MB cap, image/* only, timeout) -> SHA-256 ->
// cache lookup -> decode (createImageBitmap) -> scale (long edge <= 1600 px, x2 when the short edge < 300 px) ->
// Tesseract worker (eng first; Latin-script and Cyrillic models only for low-confidence results) -> text.
// The document closes itself after an idle period; the worker pools are rebuilt on the next request.
"use strict";

(function () {
  var MAX_BYTES = 8 * 1024 * 1024;
  var MAX_PIXELS = 48e6;
  var MAX_EDGE = 1600;
  var SMALL_EDGE = 300;
  var FETCH_TIMEOUT_MS = 10000;
  var INIT_TIMEOUT_MS = 20000;
  var RECOGNIZE_TIMEOUT_MS = 20000;
  var LADDER_BUDGET_MS = 30000;
  var IDLE_CLOSE_MS = 120000;
  var SECONDARY_IDLE_MS = 30000;
  var CACHE_MAX = 500;
  var MAX_TEXT = 20000;
  var MASK_BELOW = 82; // plain pass less sure than this: also read the white-text mask
  var LOW_CONFIDENCE = 70; // other-language passes only for confidence in [NOISE_CONFIDENCE, LOW_CONFIDENCE)
  var NOISE_CONFIDENCE = 45; // below this the "text" is almost certainly picture noise, not a language to retry
  var RETRY_MIN_CHARS = 12;
  var MAX_ACTIVE_JOBS = 3;
  var PRIMARY = "eng";
  var SECONDARY = ["spa+fra+deu+por+ita", "rus"];

  var cb = chrome;
  var cores = (typeof navigator === "object" && navigator.hardwareConcurrency) || 4;
  var PRIMARY_SLOTS = cores >= 8 ? 2 : 1;

  var stats = { requests: 0, ok: 0, skipped: 0, error: 0, cacheHits: 0, hashHits: 0, workersCreated: 0, workersDropped: 0, timeouts: 0, ladder: 0, maskPasses: 0 };
  var urlCache = new Map();
  var hashCache = new Map();
  var sets = Object.create(null);
  var active = 0;
  var activeWaiters = [];
  var lastActivity = Date.now();
  var relaxedSimd = null;

  function touch() { lastActivity = Date.now(); }
  function ext(path) { return cb.runtime.getURL(path); }
  function sleepFail(ms, what) {
    return new Promise(function (_, reject) { setTimeout(function () { reject(new Error(what)); }, ms); });
  }
  function SkipError(reason) { this.skip = reason; }

  // ---- LRU ------------------------------------------------------------------------------------
  function lruGet(map, key) {
    if (!map.has(key)) return null;
    var v = map.get(key);
    map.delete(key);
    map.set(key, v);
    return v;
  }
  function lruSet(map, key, value) {
    if (map.has(key)) map.delete(key);
    map.set(key, value);
    while (map.size > CACHE_MAX) map.delete(map.keys().next().value);
  }
  function urlKey(url) {
    if (url.length <= 300) return url;
    var h = 0x811c9dc5;
    for (var i = 0; i < url.length; i++) { h ^= url.charCodeAt(i); h = Math.imul(h, 0x01000193); }
    return url.slice(0, 40) + "#" + url.length + "." + (h >>> 0).toString(36);
  }

  function sha256(buf) {
    return crypto.subtle.digest("SHA-256", buf).then(function (d) {
      var a = new Uint8Array(d);
      var s = "";
      for (var i = 0; i < a.length; i++) s += (a[i] < 16 ? "0" : "") + a[i].toString(16);
      return s;
    });
  }

  // ---- job admission (bounds memory held by decoded images) ------------------------------------
  function admit() {
    if (active < MAX_ACTIVE_JOBS) { active++; return Promise.resolve(); }
    return new Promise(function (res) { activeWaiters.push(res); });
  }
  function leave() {
    var next = activeWaiters.shift();
    if (next) next(); else active--;
  }

  // ---- fetching ---------------------------------------------------------------------------------
  function b64ToBytes(b64) {
    var bin = atob(b64);
    var out = new Uint8Array(bin.length);
    for (var i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  }

  function readCapped(res) {
    var len = Number(res.headers.get("content-length"));
    if (isFinite(len) && len > MAX_BYTES) { try { res.body && res.body.cancel(); } catch (e) { /* ignore */ } return Promise.reject(new SkipError("too-large")); }
    if (!res.body || !res.body.getReader) {
      return res.arrayBuffer().then(function (b) { if (b.byteLength > MAX_BYTES) throw new SkipError("too-large"); return new Uint8Array(b); });
    }
    var reader = res.body.getReader();
    var chunks = [];
    var total = 0;
    function pump() {
      return reader.read().then(function (r) {
        if (r.done) {
          var out = new Uint8Array(total);
          var off = 0;
          for (var i = 0; i < chunks.length; i++) { out.set(chunks[i], off); off += chunks[i].length; }
          return out;
        }
        total += r.value.length;
        if (total > MAX_BYTES) { try { reader.cancel(); } catch (e) { /* ignore */ } throw new SkipError("too-large"); }
        chunks.push(r.value);
        return pump();
      });
    }
    return pump();
  }

  function fetchBytes(url) {
    var ctl = new AbortController();
    var timer = setTimeout(function () { ctl.abort(); }, FETCH_TIMEOUT_MS);
    return fetch(url, { credentials: "omit", redirect: "follow", referrerPolicy: "no-referrer", cache: "force-cache", signal: ctl.signal }).then(function (res) {
      if (!res.ok) throw new Error("http " + res.status);
      var type = (res.headers.get("content-type") || "").split(";")[0].trim().toLowerCase();
      if (type.indexOf("image/") !== 0) { try { res.body && res.body.cancel(); } catch (e) { /* ignore */ } throw new SkipError("not-image"); }
      return readCapped(res).then(function (bytes) { return { bytes: bytes, type: type }; });
    }).then(function (r) { clearTimeout(timer); return r; }, function (e) { clearTimeout(timer); throw e; });
  }

  function loadBytes(msg) {
    if (typeof msg.b64 === "string") {
      if (msg.b64.length > Math.ceil(MAX_BYTES * 4 / 3)) return Promise.reject(new SkipError("too-large"));
      var mime = typeof msg.mime === "string" ? msg.mime.toLowerCase() : "";
      if (mime.indexOf("image/") !== 0) return Promise.reject(new SkipError("not-image"));
      return Promise.resolve({ bytes: b64ToBytes(msg.b64), type: mime });
    }
    return fetchBytes(msg.url);
  }

  // ---- decoding ---------------------------------------------------------------------------------
  function svgToBitmap(blob) {
    return new Promise(function (resolve, reject) {
      var u = URL.createObjectURL(blob);
      var img = new Image();
      img.onload = function () {
        var w = img.naturalWidth || 600, h = img.naturalHeight || 400;
        createImageBitmap(img, { resizeWidth: Math.min(w, MAX_EDGE), resizeHeight: Math.min(h, MAX_EDGE) }).then(function (b) { URL.revokeObjectURL(u); resolve(b); }, function (e) { URL.revokeObjectURL(u); reject(e); });
      };
      img.onerror = function () { URL.revokeObjectURL(u); reject(new Error("decode")); };
      img.src = u;
    });
  }

  function decode(bytes, type) {
    var blob = new Blob([bytes], { type: type });
    return createImageBitmap(blob).catch(function (e) {
      if (type === "image/svg+xml") return svgToBitmap(blob);
      throw e;
    });
  }

  // The scaled raster that goes to Tesseract. A white backdrop makes transparent PNGs readable.
  function raster(bitmap) {
    var w = bitmap.width, h = bitmap.height;
    if (!w || !h || w * h > MAX_PIXELS) throw new SkipError("dimensions");
    var s = Math.min(1, MAX_EDGE / Math.max(w, h));
    if (Math.min(w, h) * s < SMALL_EDGE) s = Math.min(s * 2, 3200 / Math.max(w, h));
    var tw = Math.max(1, Math.round(w * s)), th = Math.max(1, Math.round(h * s));
    var c = new OffscreenCanvas(tw, th);
    var g = c.getContext("2d", { willReadFrequently: true });
    g.fillStyle = "#fff";
    g.fillRect(0, 0, tw, th);
    g.imageSmoothingQuality = "high";
    g.drawImage(bitmap, 0, 0, tw, th);
    return c;
  }

  // Meme text is white with a dark outline over a busy picture: keep only near-white, low-saturation pixels and
  // paint them black on white. Used as a second attempt when the plain pass found (almost) nothing.
  function whiteTextMask(canvas) {
    var w = canvas.width, h = canvas.height;
    var g = canvas.getContext("2d", { willReadFrequently: true });
    var img = g.getImageData(0, 0, w, h);
    var d = img.data;
    for (var i = 0; i < d.length; i += 4) {
      var r = d[i], gg = d[i + 1], b = d[i + 2];
      var mx = r > gg ? (r > b ? r : b) : (gg > b ? gg : b);
      var mn = r < gg ? (r < b ? r : b) : (gg < b ? gg : b);
      var v = mn >= 205 && mx - mn <= 40 ? 0 : 255;
      d[i] = d[i + 1] = d[i + 2] = v;
      d[i + 3] = 255;
    }
    var out = new OffscreenCanvas(w, h);
    out.getContext("2d").putImageData(img, 0, 0);
    return out;
  }

  function toPng(canvas) {
    return canvas.convertToBlob({ type: "image/png" }).then(function (b) { return b.arrayBuffer(); }).then(function (a) { return new Uint8Array(a); });
  }

  // ---- Tesseract worker pools ---------------------------------------------------------------------
  function detectRelaxedSimd() {
    if (relaxedSimd !== null) return relaxedSimd;
    // module using i8x16.relaxed_swizzle: valid only when relaxed SIMD is supported (same probe as wasm-feature-detect)
    var bytes = new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0, 1, 5, 1, 96, 0, 1, 123, 3, 2, 1, 0, 10, 15, 1, 13, 0, 65, 1, 253, 15, 65, 2, 253, 15, 253, 128, 2, 11]);
    try { relaxedSimd = WebAssembly.validate(bytes); } catch (e) { relaxedSimd = false; }
    return relaxedSimd;
  }

  function createWorker(langs) {
    stats.workersCreated++;
    var core = detectRelaxedSimd() ? "tesseract-core-relaxedsimd-lstm.wasm.js" : "tesseract-core-simd-lstm.wasm.js";
    return Tesseract.createWorker(langs, 1, {
      workerPath: ext("vendor/tesseract/worker.min.js"),
      corePath: ext("vendor/tesseract/core/" + core),
      langPath: ext("vendor/tesseract/lang"),
      workerBlobURL: false,
      cacheMethod: "none",
      gzip: true,
      logger: function () {},
    });
  }

  function setFor(langs) {
    return sets[langs] || (sets[langs] = { slots: [], waiters: [], max: langs === PRIMARY ? PRIMARY_SLOTS : 1 });
  }

  function acquire(langs) {
    var set = setFor(langs);
    for (var i = 0; i < set.slots.length; i++) if (!set.slots[i].busy) { set.slots[i].busy = true; return Promise.resolve(set.slots[i]); }
    if (set.slots.length < set.max) {
      var slot = { langs: langs, busy: true, worker: null, init: null, last: Date.now() };
      set.slots.push(slot);
      return Promise.resolve(slot);
    }
    return new Promise(function (res) { set.waiters.push(res); });
  }

  function release(slot) {
    slot.last = Date.now();
    var set = setFor(slot.langs);
    var next = set.waiters.shift();
    if (next) next(slot); else slot.busy = false;
  }

  function dropWorker(slot) {
    var w = slot.worker;
    slot.worker = null;
    slot.init = null;
    if (w) { stats.workersDropped++; try { w.terminate(); } catch (e) { /* ignore */ } }
  }

  function workerOf(slot) {
    if (slot.worker) return Promise.resolve(slot.worker);
    if (!slot.init) {
      slot.init = createWorker(slot.langs).then(function (w) { slot.worker = w; return w; }, function (e) { slot.init = null; throw e; });
    }
    return Promise.race([slot.init, sleepFail(INIT_TIMEOUT_MS, "init-timeout")]).catch(function (e) {
      if (/init-timeout/.test(String(e && e.message))) dropWorker(slot);
      throw e;
    });
  }

  // One recognition on a free slot of `langs`; the slot is torn down on timeout so a wedged worker never sticks.
  function recognize(langs, pngBytes) {
    return acquire(langs).then(function (slot) {
      return workerOf(slot).then(function (w) {
        return Promise.race([w.recognize(pngBytes, {}, { text: true }), sleepFail(RECOGNIZE_TIMEOUT_MS, "recognize-timeout")]);
      }).then(function (r) {
        release(slot);
        var data = (r && r.data) || {};
        return { text: String(data.text || ""), confidence: typeof data.confidence === "number" ? data.confidence : 0 };
      }, function (e) {
        if (/timeout/.test(String(e && e.message))) stats.timeouts++;
        dropWorker(slot);
        release(slot);
        throw e;
      });
    });
  }

  function alnum(s) { return s.replace(/[^\p{L}\p{N}]/gu, "").length; }

  function scan(bytes, type, debug) {
    var t0 = Date.now();
    var dbg = debug ? [] : null; // test-only diagnostics (confidence per pass); never produced for content-script requests
    return decode(bytes, type).then(function (bm) {
      var canvas = raster(bm);
      try { bm.close(); } catch (e) { /* ignore */ }
      return canvas;
    }).then(function (canvas) {
      return toPng(canvas).then(function (png) { return recognize(PRIMARY, png); }).then(function (first) {
        var best = first;
        var texts = [first.text];
        var chain = Promise.resolve();
        if (dbg) dbg.push({ pass: "eng", conf: Math.round(first.confidence), chars: alnum(first.text), ms: Date.now() - t0 });
        // plain pass found little, or read it unsure: also read the white-text mask (outlined captions on busy pictures)
        if (alnum(first.text) < 8 || first.confidence < MASK_BELOW) {
          chain = chain.then(function () {
            stats.maskPasses++;
            return toPng(whiteTextMask(canvas)).then(function (m) { return recognize(PRIMARY, m); }).then(function (r) {
              texts.push(r.text);
              if (dbg) dbg.push({ pass: "mask", conf: Math.round(r.confidence), chars: alnum(r.text), ms: Date.now() - t0 });
              if (r.confidence > best.confidence && alnum(r.text) >= 4) best = r;
            }, function () { /* keep the plain result */ });
          });
        }
        // enough glyphs read with middling confidence: probably another language or script than English
        chain = chain.then(function () {
          if (alnum(best.text) < RETRY_MIN_CHARS || best.confidence >= LOW_CONFIDENCE || best.confidence < NOISE_CONFIDENCE) return null;
          var p = Promise.resolve();
          SECONDARY.forEach(function (langs) {
            p = p.then(function () {
              if (Date.now() - t0 > LADDER_BUDGET_MS) return null;
              stats.ladder++;
              return toPng(canvas).then(function (png) { return recognize(langs, png); }).then(function (r) {
                texts.push(r.text);
                if (dbg) dbg.push({ pass: langs, conf: Math.round(r.confidence), chars: alnum(r.text), ms: Date.now() - t0 });
              }, function () { /* secondary models are best effort */ });
            });
          });
          return p;
        });
        return chain.then(function () {
          var text = texts.join("\n").replace(/[ \t]+/g, " ").replace(/\n\s*\n+/g, "\n").trim();
          var out = { status: "ok", text: text.length > MAX_TEXT ? text.slice(0, MAX_TEXT) : text };
          if (dbg) out.dbg = dbg;
          return out;
        });
      });
    });
  }

  // ---- request handling ---------------------------------------------------------------------------
  function isValid(msg) {
    if (!msg || msg.t !== "ocr-run") return false;
    if (typeof msg.b64 === "string") return typeof msg.mime === "string";
    return typeof msg.url === "string" && /^(?:https?:|data:)/i.test(msg.url);
  }

  function handle(msg) {
    stats.requests++;
    var key = typeof msg.b64 === "string" ? null : urlKey(msg.url);
    var hit = key && lruGet(urlCache, key);
    if (hit) { stats.cacheHits++; return Promise.resolve(hit); }
    return admit().then(function () {
      return loadBytes(msg).then(function (r) {
        return sha256(r.bytes).then(function (h) {
          var again = lruGet(hashCache, h);
          if (again) { stats.hashHits++; if (key) lruSet(urlCache, key, again); return again; }
          return scan(r.bytes, r.type, msg.debug === true).then(function (res) {
            if (!res.dbg) { lruSet(hashCache, h, res); if (key) lruSet(urlCache, key, res); }
            return res;
          });
        });
      });
    }).then(function (res) { leave(); return res; }, function (e) { leave(); throw e; });
  }

  function reply(promise, send, debug) {
    promise.then(function (res) {
      if (res.status === "ok") stats.ok++;
      send(res);
    }, function (e) {
      if (e instanceof SkipError) { stats.skipped++; send({ status: "skipped" }); return; }
      stats.error++;
      send(debug ? { status: "error", why: String((e && e.message) || e).slice(0, 120) } : { status: "error" });
    }).then(touch, touch);
  }

  cb.runtime.onMessage.addListener(function (msg, sender, send) {
    // Only the service worker (no sender.tab) of this extension may drive the scanner.
    if (!msg || typeof msg !== "object" || sender.id !== cb.runtime.id || sender.tab) return false;
    if (msg.t === "ocr-stats") {
      var heap = 0;
      try { heap = performance.memory ? performance.memory.usedJSHeapSize : 0; } catch (e) { /* ignore */ }
      var slots = 0;
      for (var k in sets) slots += sets[k].slots.filter(function (s) { return s.worker; }).length;
      send({ stats: stats, urlCache: urlCache.size, hashCache: hashCache.size, workers: slots, primarySlots: PRIMARY_SLOTS, heap: heap, relaxedSimd: detectRelaxedSimd() });
      return false;
    }
    if (!isValid(msg)) return false;
    touch();
    reply(handle(msg), send, msg.debug === true);
    return true;
  });

  // Idle management: drop secondary-language workers quickly, close the whole document when nothing happens.
  setInterval(function () {
    var now = Date.now();
    for (var langs in sets) {
      if (langs === PRIMARY) continue;
      sets[langs].slots.forEach(function (s) { if (s.worker && !s.busy && now - s.last > SECONDARY_IDLE_MS) dropWorker(s); });
    }
    var busy = active > 0;
    for (var l in sets) sets[l].slots.forEach(function (s) { if (s.busy) busy = true; });
    if (!busy && now - lastActivity > IDLE_CLOSE_MS) {
      for (var l2 in sets) sets[l2].slots.forEach(dropWorker);
      window.close();
    }
  }, 10000);
})();
