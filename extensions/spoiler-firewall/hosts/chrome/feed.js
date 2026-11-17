// Spoiler Firewall term-update feed: format, verification and status text. Classic script (service worker via
// importScripts, popup/options via <script>) with a CommonJS tail so the Node unit tests load the very same code.
// No network, no storage, no DOM in here: pure functions over bytes and objects.
//
// A feed is ONE JSON file, the "envelope" (see feed/schema.md):
//     { "format": 1, "payload": "<base64 of the exact payload bytes>", "sig": "<base64 Ed25519 signature, 64 bytes>" }
// and the payload is UTF-8 JSON:
//     { "format": 1, "seq": 7, "issued": "2026-11-18", "expires": "2026-12-18", "lexicon": { "version"?, "add"?, "raise"? } }
// The signature covers the payload bytes exactly as they appear (decoded from base64), so nothing is ever
// re-serialised before it is checked. `lexicon` is handed to SFW.lexicon.extend(), which can only add terms or
// raise a term's category; this file adds no way to loosen protection.
//
// Order of checks in verifyEnvelope: size, envelope shape, base64, signature, payload JSON, schema, rollback.
// Nothing from the payload is looked at before the signature verified.
var SFWFeed = (function (root) {
  "use strict";

  var MAX_BYTES = 262144; // 256 KiB: the whole downloaded body, and the decoded payload
  var MAX_LEXICON_JSON = 200000; // SFW.lexicon.limits.maxJson: a bigger object would be rejected by the engine anyway
  var MAX_TERMS = 5000; // SFW.lexicon.limits.maxTerms: extend() refuses a feed that would take the extra terms past this
  var MAX_SEQ = 2147483647;
  var LEXICON_KEYS = { version: 1, add: 1, raise: 1 };
  var ADD_CATEGORIES = { franchise: 1, entityStrong: 1, entityWeak: 1, cueStrong: 1, cueWeak: 1, marker: 1, game: 1, shapeVerb: 1, shapeEnd: 1 };
  var RAISE_TARGETS = { entityWeak: 1, entityStrong: 1, franchise: 1, cueWeak: 1, cueStrong: 1, marker: 1 };
  var hasOwn = Object.prototype.hasOwnProperty;

  function fail(code) { return { ok: false, code: code }; }
  function isPlain(v) { return v !== null && typeof v === "object" && !Array.isArray(v); }

  function utf8Length(text) { return new TextEncoder().encode(text).length; }

  // Strict standard base64 (padded, no whitespace). Returns Uint8Array or null.
  function b64decode(text) {
    if (typeof text !== "string" || text.length === 0 || text.length % 4 !== 0 || !/^[A-Za-z0-9+/]+={0,2}$/.test(text)) return null;
    try {
      var bin = root.atob(text);
      var out = new Uint8Array(bin.length);
      for (var i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
      return out;
    } catch (e) { return null; }
  }

  // "2026-11-18" or a full ISO timestamp with Z / offset. A bare date means the END of that UTC day when it is an expiry.
  function parseDate(text, endOfDay) {
    if (typeof text !== "string") return null;
    // Same grammar and the same calendar/clock range checks as src/spoiler_feed.rs (Bowser): both clients must accept
    // exactly the same dates.
    var m = /^(\d{4})-(\d{2})-(\d{2})(?:T(\d{2}):(\d{2})(?::(\d{2})(?:\.\d{1,3})?)?(?:Z|[+-](\d{2}):(\d{2})))?$/.exec(text);
    if (!m) return null;
    var dateOnly = m[4] === undefined;
    var year = +m[1], month = +m[2], day = +m[3];
    var leap = (year % 4 === 0 && year % 100 !== 0) || year % 400 === 0;
    var dim = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][month - 1];
    if (month < 1 || month > 12 || day < 1 || day > dim) return null;
    if (!dateOnly && (+m[4] > 23 || +m[5] > 59 || (m[6] !== undefined && +m[6] > 59) || (m[7] !== undefined && (+m[7] > 23 || +m[8] > 59)))) return null;
    var ms = Date.parse(text);
    if (!isFinite(ms)) return null;
    return dateOnly && endOfDay ? ms + 86400000 - 1 : ms;
  }

  function validLexicon(lex) {
    if (!isPlain(lex)) return false;
    var keys = Object.keys(lex);
    for (var i = 0; i < keys.length; i++) if (!hasOwn.call(LEXICON_KEYS, keys[i])) return false;
    if (hasOwn.call(lex, "version") && (typeof lex.version !== "string" || lex.version.length > 40 || !/^[\w.+-]+$/.test(lex.version))) return false;
    var count = 0;
    if (hasOwn.call(lex, "add")) {
      if (!isPlain(lex.add)) return false;
      var cats = Object.keys(lex.add);
      for (var c = 0; c < cats.length; c++) {
        if (!hasOwn.call(ADD_CATEGORIES, cats[c]) || !Array.isArray(lex.add[cats[c]])) return false;
        for (var t = 0; t < lex.add[cats[c]].length; t++) if (typeof lex.add[cats[c]][t] !== "string") return false;
        count += lex.add[cats[c]].length;
      }
    }
    if (hasOwn.call(lex, "raise")) {
      if (!isPlain(lex.raise)) return false;
      var terms = Object.keys(lex.raise);
      for (var r = 0; r < terms.length; r++) if (typeof lex.raise[terms[r]] !== "string" || !hasOwn.call(RAISE_TARGETS, lex.raise[terms[r]])) return false;
      count += terms.length;
    }
    if (count > MAX_TERMS) return false;
    try { return JSON.stringify(lex).length <= MAX_LEXICON_JSON; } catch (e) { return false; }
  }

  // Validates a decoded payload object. Returns {ok:true, feed} or {ok:false, code}.
  function checkPayload(p) {
    if (!isPlain(p)) return fail("payload");
    if (p.format !== 1) return fail("format");
    if (typeof p.seq !== "number" || !isFinite(p.seq) || Math.floor(p.seq) !== p.seq || p.seq < 1 || p.seq > MAX_SEQ) return fail("seq");
    var issued = parseDate(p.issued, false);
    var expires = parseDate(p.expires, true);
    if (issued === null || expires === null || expires < issued) return fail("dates");
    if (!validLexicon(p.lexicon)) return fail("lexicon");
    return { ok: true, feed: { seq: p.seq, issued: p.issued, expires: p.expires, lexicon: p.lexicon } };
  }

  // body: the downloaded text. keys: array of Uint8Array(32) or base64 strings. opts: { highestSeq }.
  // Resolves {ok:true, feed, current} (current: this exact seq is already the highest seen, so there is nothing to
  // store) or {ok:false, code} with code one of size, envelope, signature, payload, format, seq, dates, lexicon,
  // rollback, nokey, unsupported. Never throws.
  function verifyEnvelope(body, keys, opts) {
    return Promise.resolve().then(function () {
      var highest = opts && typeof opts.highestSeq === "number" ? opts.highestSeq : 0;
      if (typeof body !== "string" || body.length > MAX_BYTES || utf8Length(body) > MAX_BYTES) return fail("size");
      var env;
      try { env = JSON.parse(body); } catch (e) { return fail("envelope"); }
      if (!isPlain(env) || typeof env.payload !== "string" || typeof env.sig !== "string") return fail("envelope");
      if (env.format !== 1) return fail("format");
      var payload = b64decode(env.payload);
      var sig = b64decode(env.sig);
      if (!payload || !sig || sig.length !== 64 || payload.length > MAX_BYTES) return fail("envelope");
      var ed = root.nobleEd25519;
      if (!ed || typeof ed.verifyAsync !== "function") return fail("unsupported");
      var list = [];
      for (var i = 0; i < (keys || []).length; i++) {
        var k = typeof keys[i] === "string" ? b64decode(keys[i]) : keys[i];
        if (k && k.length === 32) list.push(k);
      }
      if (!list.length) return fail("nokey");
      var chain = Promise.resolve(false);
      list.forEach(function (key) {
        chain = chain.then(function (done) {
          if (done) return true;
          return ed.verifyAsync(sig, payload, key, { zip215: false }).then(function (ok) { return ok === true; }, function () { return false; });
        });
      });
      return chain.then(function (verified) {
        if (!verified) return fail("signature");
        var parsed;
        try { parsed = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(payload)); } catch (e) { return fail("payload"); }
        var checked = checkPayload(parsed);
        if (!checked.ok) return checked;
        if (checked.feed.seq < highest) return fail("rollback");
        return { ok: true, feed: checked.feed, current: checked.feed.seq === highest };
      });
    }).catch(function () { return fail("error"); });
  }

  function isExpired(feed, now) {
    var ms = feed ? parseDate(feed.expires, true) : null;
    return ms !== null && (typeof now === "number" ? now : Date.now()) > ms;
  }

  function dayOf(text) { return typeof text === "string" ? text.slice(0, 10) : ""; }

  // What the popup and options page say. input: { builtin, enabled, feed (stored record or null), status ({state, code} or null), now }.
  // Returns { text, notes: [..], warn }. `warn` is true when something needs the user's attention.
  function describe(input) {
    var builtin = input.builtin || "built-in";
    var feed = input.feed && typeof input.feed.seq === "number" ? input.feed : null;
    var status = input.status && typeof input.status === "object" ? input.status : null;
    var text = "Term list: built-in " + builtin;
    if (feed) text += " + update #" + feed.seq + " (" + dayOf(feed.issued) + ")";
    var notes = [];
    var warn = false;
    if (input.enabled === false) notes.push("Term updates are off.");
    if (feed && isExpired(feed, input.now)) { notes.push("Term updates out of date."); warn = true; }
    if (status && status.state === "rejected") { notes.push("Term update rejected."); warn = true; }
    else if (input.enabled !== false && status && status.state === "unavailable") notes.push("Could not check for term updates.");
    else if (input.enabled !== false && status && status.state === "none" && !feed) notes.push("No term update has been published yet.");
    return { text: text, notes: notes, warn: warn };
  }

  var api = {
    MAX_BYTES: MAX_BYTES, MAX_SEQ: MAX_SEQ, MAX_TERMS: MAX_TERMS, MAX_LEXICON_JSON: MAX_LEXICON_JSON,
    verifyEnvelope: verifyEnvelope, checkPayload: checkPayload, parseDate: parseDate, b64decode: b64decode,
    isExpired: isExpired, describe: describe,
  };
  return api;
})(typeof self !== "undefined" ? self : globalThis);

if (typeof module !== "undefined" && module.exports) module.exports = SFWFeed;
