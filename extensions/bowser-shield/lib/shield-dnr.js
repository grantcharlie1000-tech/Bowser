// Bowser Shield: pure translation of a compiled SRL list into declarativeNetRequest rules.
//
// No chrome.* calls. Section 5 of DESIGN.md is the spec: id bands (5.4), the shape of each rule
// (5.2, 5.3) and the caps (5.5). The service worker loads this with importScripts() and the Node
// tests with require().

const ShieldDnr = (function () {
  "use strict";

  var DYNAMIC_BUDGET = 5000;
  var REGEX_CAP = 1000;

  // Position of each SRL type in a Rule's 9-bit `types` mask. This must stay in step with
  // ShieldCore.TYPE_NAMES; a test asserts that it does.
  var TYPE_ORDER = ["script", "image", "frame", "xhr", "media", "font", "style", "ping", "other"];
  var ALL_TYPES = 511;

  var RESOURCE_TYPES = {
    script: ["script"],
    image: ["image"],
    frame: ["sub_frame"],
    xhr: ["xmlhttprequest"],
    media: ["media"],
    font: ["font"],
    style: ["stylesheet"],
    ping: ["ping", "csp_report"],
    other: ["object", "websocket", "webtransport", "other"]
  };

  // Fixed bands: id = start + position within the band (section 5.4).
  var BANDS = {
    starter: { allow: { start: 2000, size: 1000 }, block: { start: 1000, size: 1000 } },
    user: {
      off: { start: 30000, size: 500 },
      allow: { start: 20000, size: 10000 },
      block: { start: 10000, size: 10000 }
    }
  };
  var PRIORITY = { block: 1, allow: 2, off: 3 };

  var PROBE_RULE = {
    id: 999,
    priority: 1,
    action: { type: "block" },
    condition: { requestDomains: ["probe.bowser-shield.invalid"], resourceTypes: ["image"] }
  };

  function isBlockRuleId(id) {
    return (id >= 1000 && id <= 1999) || (id >= 10000 && id <= 19999);
  }

  function escapeRegex(v) {
    return v.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  }

  function resourceTypesFor(mask) {
    var out = [];
    for (var bit = 0; bit < TYPE_ORDER.length; bit++) {
      if ((mask >> bit) & 1) Array.prototype.push.apply(out, RESOURCE_TYPES[TYPE_ORDER[bit]]);
    }
    return out;
  }

  function buildRule(rule, id, priority) {
    if (rule.verb === "off") {
      return {
        id: id,
        priority: priority,
        action: { type: "allowAllRequests" },
        condition: { requestDomains: [rule.value], resourceTypes: ["main_frame"] }
      };
    }
    var condition = {};
    if (rule.kind === "host") {
      condition.requestDomains = [rule.value];
    } else if (rule.kind === "path") {
      condition.urlFilter = rule.value;
      condition.isUrlFilterCaseSensitive = false;
    } else {
      condition.regexFilter = "^https?://([a-z0-9-]+\\.)*[a-z0-9-]*" + escapeRegex(rule.value) + "[a-z0-9-]*\\.";
      condition.isUrlFilterCaseSensitive = false;
    }
    if (rule.only) condition.initiatorDomains = rule.only.slice();
    if (rule.except) condition.excludedInitiatorDomains = rule.except.slice();
    if (rule.party === 1) condition.domainType = "firstParty";
    else if (rule.party === 2) condition.domainType = "thirdParty";
    // Every kind: leave resourceTypes out. declarativeNetRequest then covers every type except
    // main_frame, which a block or allow rule must never name.
    if (rule.types !== ALL_TYPES) condition.resourceTypes = resourceTypesFor(rule.types);
    return {
      id: id,
      priority: priority,
      action: { type: rule.verb },
      condition: condition
    };
  }

  // compiled: anything with a `rules` array of Rule records (ShieldCore.compile output, or a list
  // built by hand to go past the format's own cap).
  // opts.source: "starter" (static bands) or "user" (dynamic bands, default).
  // opts.budget: rules to keep; defaults to 5000 for the dynamic ruleset and unlimited for the static one.
  // opts.regexCap: label rules to keep; defaults to 1000.
  //
  // Allocation order is off, then allow, then block, each in file order, because dropping an
  // exception breaks a site and dropping a block only shows an ad.
  function translate(compiled, opts) {
    opts = opts || {};
    var source = opts.source === "starter" ? "starter" : "user";
    var budget = opts.budget == null ? (source === "user" ? DYNAMIC_BUDGET : Infinity) : opts.budget;
    var regexCap = opts.regexCap == null ? REGEX_CAP : opts.regexCap;
    var bands = BANDS[source];

    var groups = { off: [], allow: [], block: [] };
    for (var n = 0; n < compiled.rules.length; n++) {
      var r = compiled.rules[n];
      if (r.verb === "hide") continue;
      groups[r.verb].push(r);
    }

    var rules = [];
    var lineById = {};
    var listById = {};
    var dropped = [];
    var regexUsed = 0;

    function drop(rule, reason) {
      dropped.push({ line: rule.line, reason: reason, list: rule.list || "", order: rule.i });
    }

    ["off", "allow", "block"].forEach(function (group) {
      var band = bands[group];
      groups[group].forEach(function (rule, pos) {
        if (!band) return drop(rule, "no-band");
        if (pos >= band.size) return drop(rule, "band-full");
        if (rule.kind === "path" && /[*|^]/.test(rule.value)) return drop(rule, "path-pattern-char");
        // A urlFilter that starts with // can match the host part of a URL, not just its path.
        if (rule.kind === "path" && rule.value.slice(0, 2) === "//") return drop(rule, "path-scheme-slash");
        if (rules.length >= budget) return drop(rule, "budget");
        if (rule.kind === "label") {
          if (regexUsed >= regexCap) return drop(rule, "regex-cap");
          regexUsed++;
        }
        rules.push(buildRule(rule, band.start + pos, PRIORITY[group]));
        lineById[band.start + pos] = rule.line;
        listById[band.start + pos] = rule.list || "";
      });
    });

    dropped.sort(function (a, b) {
      return a.order - b.order;
    });
    return {
      rules: rules,
      lineById: lineById,
      listById: listById,
      dropped: dropped.map(function (d) {
        return { line: d.line, reason: d.reason, list: d.list };
      })
    };
  }

  return {
    DYNAMIC_BUDGET: DYNAMIC_BUDGET,
    REGEX_CAP: REGEX_CAP,
    TYPE_ORDER: TYPE_ORDER,
    RESOURCE_TYPES: RESOURCE_TYPES,
    BANDS: BANDS,
    PRIORITY: PRIORITY,
    PROBE_RULE: PROBE_RULE,
    isBlockRuleId: isBlockRuleId,
    translate: translate
  };
})();

if (typeof module === "object" && module.exports) module.exports = ShieldDnr; // Node tests
if (typeof self === "object") self.ShieldDnr = self.ShieldDnr || ShieldDnr; // service worker
if (typeof window === "object") window.ShieldDnr = ShieldDnr;
