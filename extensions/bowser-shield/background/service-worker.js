// Bowser Shield service worker. A classic script, not a module: importScripts is the oldest and
// dullest path, and this design does not want to find out whether the WebView2 subset has module
// service workers.
//
// It probes the network layer, owns rule sync (compile, translate, apply, store the cosmetic data),
// owns the counters, and answers the content script and the panel. Listeners are registered at the
// top level, synchronously, as a service worker requires.

importScripts("../lib/shield-core.js", "../lib/shield-dnr.js");

(function () {
  "use strict";

  var Core = self.ShieldCore;
  var Dnr = self.ShieldDnr;

  var KNOWN_VERSION = 1;
  var STARTER_RULESET = "starter";
  var SITE_LIMIT = 500;
  var SEEN_LIMIT = 4096;
  var HIDDEN_BATCH_LIMIT = 1000;
  // Injected into the top frame the first time the chord opens the overlay (see "loadPanel").
  var PANEL_FILES = ["content/panel-css.js", "ui/panel.js"];
  var CHORD_MODIFIERS = { alt: "alt", shift: "shift", ctrl: "ctrl", control: "ctrl", meta: "meta" };

  var DEFAULTS = {
    "shield.version": KNOWN_VERSION,
    "shield.enabled": true,
    "shield.starterEnabled": true,
    "shield.userRules": "",
    "shield.siteOff": [],
    "shield.cosmetic": {},
    "shield.compiled": {},
    "shield.engineMode": "unknown",
    "shield.panelChord": "Alt+Shift+S"
  };

  var READ_ONLY_MESSAGE = "These settings were saved by a newer Bowser Shield, so changes are off.";

  // ---- in-memory state (counters are never persisted, section 7.4) -----------------------------

  var tabs = new Map(); // tabId -> { siteHost, blocked, hidden, seen: Set, bad: Map<frameId, count> }
  var session = { blocked: 0, hidden: 0 };
  var countsAvailable = false;
  var settingsCache = null;
  var modelCache = null;
  var starterPromise = null;
  var queue = Promise.resolve();

  function noop() {}

  // Mutations run one at a time, in order. Nothing inside a serial task may call serial() again.
  function serial(fn) {
    var run = queue.then(fn);
    queue = run.then(noop, noop);
    return run;
  }

  function idle() {
    return queue;
  }

  function UserError(message) {
    this.message = message;
  }

  // ---- helpers ---------------------------------------------------------------------------------

  function clone(value) {
    return value === undefined ? undefined : JSON.parse(JSON.stringify(value));
  }

  function hostOf(url) {
    var u;
    try {
      u = new URL(url);
    } catch (e) {
      return null;
    }
    if (u.protocol !== "http:" && u.protocol !== "https:") return null;
    var host = u.hostname.toLowerCase();
    if (host.charAt(host.length - 1) === ".") host = host.slice(0, -1);
    return host;
  }

  function topUrl(sender) {
    return (sender && sender.tab && sender.tab.url) || (sender && sender.url) || "";
  }

  // The same chords ui/panel.js parseChord accepts, which a test keeps in step: one key, any modifier
  // at most once, and at least one of Alt, Ctrl or Meta so the chord cannot be typed into a page.
  function chordAccepted(text) {
    var parts = text.split("+");
    if (parts.length < 2) return false;
    var seen = {};
    var key = false;
    for (var i = 0; i < parts.length; i++) {
      var word = parts[i].trim();
      var low = word.toLowerCase();
      if (Object.prototype.hasOwnProperty.call(CHORD_MODIFIERS, low)) {
        if (seen[CHORD_MODIFIERS[low]]) return false;
        seen[CHORD_MODIFIERS[low]] = true;
      } else if (key || !/^(?:[a-z0-9]|f(?:[1-9]|1[0-2]))$/i.test(word)) {
        return false;
      } else {
        key = true;
      }
    }
    return key && !!(seen.alt || seen.ctrl || seen.meta);
  }

  function utf8Bytes(text) {
    return new TextEncoder().encode(text).length;
  }

  // ---- settings --------------------------------------------------------------------------------

  async function readSettings() {
    if (settingsCache) return settingsCache;
    var raw = await chrome.storage.local.get(Object.keys(DEFAULTS));
    var s = { raw: raw };
    Object.keys(DEFAULTS).forEach(function (key) {
      s[key] = raw[key] === undefined ? clone(DEFAULTS[key]) : raw[key];
    });
    s.enabled = s["shield.enabled"] !== false;
    s.starterEnabled = s["shield.starterEnabled"] !== false;
    s.userRules = typeof s["shield.userRules"] === "string" ? s["shield.userRules"] : "";
    s.siteOff = Array.isArray(s["shield.siteOff"])
      ? s["shield.siteOff"].filter(function (h) {
          return typeof h === "string" && Core.isValidSite(h);
        })
      : [];
    s.engineMode = s["shield.engineMode"] === "network" || s["shield.engineMode"] === "dom" ? s["shield.engineMode"] : "unknown";
    s.chord = typeof s["shield.panelChord"] === "string" ? s["shield.panelChord"] : DEFAULTS["shield.panelChord"];
    var version = raw["shield.version"];
    s.firstRun = version === undefined;
    s.readOnly = typeof version === "number" && version > KNOWN_VERSION;
    settingsCache = s;
    return s;
  }

  async function write(values) {
    await chrome.storage.local.set(values);
    settingsCache = null;
    modelCache = null;
  }

  chrome.storage.onChanged.addListener(function () {
    settingsCache = null;
    modelCache = null;
  });

  // ---- the starter list and the compiled model ---------------------------------------------------

  function loadStarter() {
    if (!starterPromise) {
      starterPromise = fetch(chrome.runtime.getURL("rules/starter.srl"))
        .then(function (r) {
          return r.text();
        })
        .then(function (text) {
          var compiled = Core.compile(text);
          var regexRules = 0;
          compiled.rules.forEach(function (r) {
            if ((r.verb === "block" || r.verb === "allow") && r.kind === "label") regexRules++;
          });
          return { srl: text, counts: compiled.counts, regexRules: regexRules };
        })
        .catch(function () {
          starterPromise = null;
          return { srl: "", counts: { block: 0, allow: 0, hide: 0, off: 0 }, regexRules: 0 };
        });
    }
    return starterPromise;
  }

  function countList(rules, list) {
    var counts = { block: 0, allow: 0, hide: 0, off: 0 };
    rules.forEach(function (r) {
      if (r.list === list) counts[r.verb]++;
    });
    return counts;
  }

  // The compiled lists in matching order: starter (when on), the site exceptions, then the user's.
  function getModel(s, starter) {
    var key = (s.starterEnabled ? "1" : "0") + "|" + s.siteOff.join(",") + "|" + s.userRules;
    if (modelCache && modelCache.key === key) return modelCache;
    var all = Core.compileAll([
      { tag: "starter", src: s.starterEnabled ? starter.srl : "" },
      { tag: "site", src: s.siteOff.map(function (h) { return "off site:" + h; }).join("\n") },
      { tag: "user", src: s.userRules }
    ]);
    modelCache = { key: key, all: all };
    return modelCache;
  }

  // ---- the network layer -----------------------------------------------------------------------

  async function probeEngine() {
    var dnr = chrome.declarativeNetRequest;
    if (!dnr || !dnr.updateDynamicRules || !dnr.getDynamicRules) return "dom";
    try {
      await dnr.updateDynamicRules({ removeRuleIds: [Dnr.PROBE_RULE.id], addRules: [Dnr.PROBE_RULE] });
      var live = await dnr.getDynamicRules();
      await dnr.updateDynamicRules({ removeRuleIds: [Dnr.PROBE_RULE.id] });
      return live.some(function (r) {
        return r.id === Dnr.PROBE_RULE.id;
      })
        ? "network"
        : "dom";
    } catch (e) {
      return "dom";
    }
  }

  // Removes every dynamic id that is actually there, then adds the new rules in one atomic call.
  // If the browser refuses the batch (it is all or nothing), retries one rule at a time and returns
  // the ids it would not take, so they can be reported instead of silently missing.
  async function applyDynamic(rules) {
    var dnr = chrome.declarativeNetRequest;
    var existing = await dnr.getDynamicRules();
    var removeRuleIds = existing.map(function (r) {
      return r.id;
    });
    try {
      await dnr.updateDynamicRules({ removeRuleIds: removeRuleIds, addRules: rules });
      return [];
    } catch (batchError) {
      await dnr.updateDynamicRules({ removeRuleIds: removeRuleIds, addRules: [] });
      var rejected = [];
      for (var i = 0; i < rules.length; i++) {
        try {
          await dnr.updateDynamicRules({ removeRuleIds: [], addRules: [rules[i]] });
        } catch (ruleError) {
          rejected.push(rules[i].id);
        }
      }
      return rejected;
    }
  }

  async function clearDynamic() {
    var dnr = chrome.declarativeNetRequest;
    if (!dnr || !dnr.getDynamicRules || !dnr.updateDynamicRules) return;
    try {
      var existing = await dnr.getDynamicRules();
      if (existing.length === 0) return;
      await dnr.updateDynamicRules({
        removeRuleIds: existing.map(function (r) {
          return r.id;
        }),
        addRules: []
      });
    } catch (e) {
      // Nothing to clean up if the API does not work.
    }
  }

  async function setStarterRuleset(on) {
    var dnr = chrome.declarativeNetRequest;
    if (!dnr || !dnr.updateEnabledRulesets) return;
    try {
      await dnr.updateEnabledRulesets(on ? { enableRulesetIds: [STARTER_RULESET] } : { disableRulesetIds: [STARTER_RULESET] });
    } catch (e) {
      // The static ruleset is best effort in a runtime that may not support the call.
    }
  }

  // compile -> translate -> updateDynamicRules -> updateEnabledRulesets -> store (section 7.3).
  async function rebuild() {
    var s = await readSettings();
    if (s.readOnly) return;
    var starter = await loadStarter();
    var all = getModel(s, starter).all;
    var network = s.engineMode === "network" && !!chrome.declarativeNetRequest && !!chrome.declarativeNetRequest.updateDynamicRules;
    var dropped = [];

    if (network && s.enabled) {
      var regexCap = Dnr.REGEX_CAP - (s.starterEnabled ? starter.regexRules : 0);
      var translated = Dnr.translate(
        {
          rules: all.rules.filter(function (r) {
            return r.list !== "starter";
          })
        },
        { source: "user", regexCap: regexCap }
      );
      dropped = translated.dropped;
      var rejected = await applyDynamic(translated.rules);
      rejected.forEach(function (id) {
        dropped.push({ line: translated.lineById[id], reason: "rejected-by-browser", list: translated.listById[id] });
      });
      dropped.sort(function (a, b) {
        return a.line - b.line;
      });
    } else {
      await clearDynamic();
    }
    // The static ruleset is independent of the dynamic-rule probe, so it follows the switches alone.
    await setStarterRuleset(s.enabled && s.starterEnabled);

    await write({
      "shield.cosmetic": Core.buildCosmetic(all),
      "shield.compiled": {
        sourceHash: all.sourceHash,
        counts: countList(all.rules, "user"),
        errors: all.errors.filter(function (e) {
          return e.list === "user";
        }),
        warnings: all.warnings.filter(function (e) {
          return e.list === "user";
        }),
        dropped: dropped
      }
    });
  }

  // ---- start up --------------------------------------------------------------------------------

  async function init(reason) {
    var s = await readSettings();
    if (s.readOnly) return;
    var missing = {};
    var any = false;
    Object.keys(DEFAULTS).forEach(function (key) {
      if (s.raw[key] === undefined) {
        missing[key] = clone(DEFAULTS[key]);
        any = true;
      }
    });
    if (any) await write(missing);
    s = await readSettings();

    var force = reason === "installed";
    if (force || s.engineMode === "unknown") {
      await write({ "shield.engineMode": await probeEngine() });
      s = await readSettings();
    }
    try {
      if (chrome.action && chrome.action.setBadgeBackgroundColor) await chrome.action.setBadgeBackgroundColor({ color: "#1c6a7a" });
    } catch (e) {
      // The badge may not exist in this host.
    }
    var compiled = s["shield.compiled"];
    if (force || !compiled || !compiled.sourceHash) await rebuild();
  }

  serial(function () {
    return init("start");
  });

  chrome.runtime.onInstalled.addListener(function () {
    return serial(function () {
      return init("installed");
    });
  });

  // ---- counting (section 8) -------------------------------------------------------------------

  function ensureEntry(tabId, host) {
    var entry = tabs.get(tabId);
    if (!entry) {
      entry = { siteHost: host || "", blocked: 0, hidden: 0, seen: new Set(), bad: new Map() };
      tabs.set(tabId, entry);
    }
    return entry;
  }

  function updateBadge(tabId, entry, mode) {
    try {
      var n = mode === "dom" ? entry.hidden : entry.blocked;
      var result = chrome.action && chrome.action.setBadgeText({ tabId: tabId, text: n > 0 ? String(n) : "" });
      if (result && result.catch) result.catch(noop);
    } catch (e) {
      // No toolbar button in this host; the panel does not depend on the badge.
    }
  }

  function onRuleMatched(info) {
    if (!info || !info.rule || !Dnr.isBlockRuleId(info.rule.ruleId)) return;
    var request = info.request || {};
    var tabId = request.tabId;
    if (typeof tabId !== "number" || tabId < 0) return;
    var entry = ensureEntry(tabId, null);
    if (entry.seen.has(request.requestId)) return;
    entry.seen.add(request.requestId);
    if (entry.seen.size > SEEN_LIMIT) entry.seen.delete(entry.seen.values().next().value);
    entry.blocked++;
    session.blocked++;
    updateBadge(tabId, entry, "network");
  }

  try {
    var feedback = chrome.declarativeNetRequest && chrome.declarativeNetRequest.onRuleMatchedDebug;
    if (feedback && feedback.addListener) {
      feedback.addListener(onRuleMatched);
      countsAvailable = true;
    }
  } catch (e) {
    countsAvailable = false;
  }

  chrome.tabs.onUpdated.addListener(function (tabId, changeInfo) {
    if (!changeInfo || typeof changeInfo.url !== "string") return;
    var host = hostOf(changeInfo.url) || "";
    var entry = tabs.get(tabId);
    if (!entry) {
      ensureEntry(tabId, host);
      return;
    }
    if (entry.siteHost && host !== entry.siteHost) {
      entry.blocked = 0;
      entry.hidden = 0;
      entry.seen = new Set();
      entry.bad = new Map();
      updateBadge(tabId, entry, "network");
    }
    entry.siteHost = host;
  });

  chrome.tabs.onRemoved.addListener(function (tabId) {
    tabs.delete(tabId);
  });

  // ---- state for the panel ---------------------------------------------------------------------

  async function activeTab() {
    try {
      var found = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
      return found && found[0] ? found[0] : null;
    } catch (e) {
      return null;
    }
  }

  async function buildState(sender, extra) {
    var s = await readSettings();
    var starter = await loadStarter();
    var all = getModel(s, starter).all;
    var tab = (sender && sender.tab) || (await activeTab());
    var page = null;
    if (tab) {
      var host = hostOf(tab.url || "");
      var entry = tabs.get(tab.id);
      var siteOff = host !== null && Core.siteIsOff(all, host);
      var listed =
        host !== null &&
        s.siteOff.some(function (e) {
          return Core.hostMatches(host, e);
        });
      page = {
        supported: host !== null,
        host: host || "",
        siteOff: siteOff,
        siteOffRuleOnly: siteOff && !listed,
        canExempt: host !== null && Core.isValidSite(host),
        blocked: entry ? entry.blocked : 0,
        hidden: entry ? entry.hidden : 0
      };
    }
    var compiled = s["shield.compiled"] || {};
    var bad = 0;
    var tabEntry = tab ? tabs.get(tab.id) : null;
    if (tabEntry) tabEntry.bad.forEach(function (n) { bad += n; });
    var state = {
      enabled: s.enabled,
      starterEnabled: s.starterEnabled,
      userRules: s.userRules,
      siteOff: s.siteOff,
      engineMode: s.engineMode,
      countsAvailable: countsAvailable && s.engineMode === "network",
      readOnly: s.readOnly,
      page: page,
      session: { blocked: session.blocked, hidden: session.hidden },
      counts: { starter: starter.counts, user: compiled.counts || { block: 0, allow: 0, hide: 0, off: 0 } },
      errors: compiled.errors || [],
      warnings: compiled.warnings || [],
      dropped: compiled.dropped || [],
      badSelectors: bad,
      starterSrl: starter.srl,
      chord: s.chord
    };
    if (extra) Object.keys(extra).forEach(function (k) { state[k] = extra[k]; });
    return state;
  }

  async function getState(sender) {
    await idle();
    return buildState(sender);
  }

  // Runs a change under the queue, refuses it on a newer schema, and answers with fresh state.
  function mutate(sender, change) {
    return serial(async function () {
      var s = await readSettings();
      if (s.readOnly) return buildState(sender, { error: READ_ONLY_MESSAGE });
      try {
        await change(s);
      } catch (e) {
        return buildState(sender, { error: e instanceof UserError ? e.message : "Could not save: " + ((e && e.message) || e) });
      }
      return buildState(sender);
    });
  }

  function normaliseSiteList(list) {
    if (!Array.isArray(list)) throw new UserError("The list of sites must be a list.");
    var out = [];
    list.forEach(function (item) {
      var host = typeof item === "string" ? item.trim().toLowerCase() : "";
      if (!Core.isValidSite(host)) throw new UserError("\"" + item + "\" is not a valid site.");
      if (out.indexOf(host) < 0) out.push(host);
    });
    if (out.length > SITE_LIMIT) throw new UserError("The list is limited to " + SITE_LIMIT + " sites.");
    return out;
  }

  // ---- messages --------------------------------------------------------------------------------

  async function frameContext(sender) {
    await idle();
    var s = await readSettings();
    var starter = await loadStarter();
    var host = hostOf(topUrl(sender));
    var model = getModel(s, starter);
    var filtered = s.enabled && host !== null && !Core.siteIsOff(model.all, host);
    var reply = { siteHost: host, enabled: s.enabled, mode: s.engineMode, filtered: filtered };
    if (filtered && s.engineMode === "dom") reply.starterSrl = s.starterEnabled ? starter.srl : "";
    return reply;
  }

  async function handle(msg, sender) {
    if (!msg || typeof msg !== "object" || typeof msg.type !== "string") return { error: "unknown message" };
    if (sender && sender.id !== chrome.runtime.id) return { error: "refused" };
    var tabId = sender && sender.tab ? sender.tab.id : null;

    switch (msg.type) {
      case "frameContext":
        return frameContext(sender);
      case "getState":
        return getState(sender);

      case "hidden": {
        await idle();
        var s = await readSettings();
        if (s.engineMode !== "dom" || tabId === null || !Array.isArray(msg.urls)) return { ok: true };
        var n = Math.min(msg.urls.filter(function (u) { return typeof u === "string"; }).length, HIDDEN_BATCH_LIMIT);
        var entry = ensureEntry(tabId, hostOf(topUrl(sender)));
        entry.hidden += n;
        session.hidden += n;
        updateBadge(tabId, entry, "dom");
        return { ok: true };
      }
      case "badSelectors": {
        if (tabId !== null && typeof msg.count === "number" && msg.count >= 0) {
          ensureEntry(tabId, hostOf(topUrl(sender))).bad.set(sender.frameId || 0, Math.floor(msg.count));
        }
        return { ok: true };
      }
      case "loadPanel": {
        // Only the top frame hosts the overlay. The panel scripts go into the isolated world the
        // content script already runs in, so they see ShieldChord and each other's globals.
        if (tabId === null || sender.frameId) return { ok: false };
        try {
          await chrome.scripting.executeScript({ target: { tabId: tabId, frameIds: [0] }, files: PANEL_FILES });
          return { ok: true };
        } catch (e) {
          return { ok: false };
        }
      }
      case "chord": {
        if (tabId !== null && sender.frameId) {
          try {
            var sent = chrome.tabs.sendMessage(tabId, { type: "togglePanel" }, { frameId: 0 });
            if (sent && sent.catch) sent.catch(noop);
          } catch (e) {
            // The top frame has no listener; nothing to open.
          }
        }
        return { ok: true };
      }

      case "setEnabled":
        return mutate(sender, async function () {
          await write({ "shield.enabled": msg.value !== false });
          await rebuild();
        });
      case "setStarterEnabled":
        return mutate(sender, async function () {
          await write({ "shield.starterEnabled": msg.value !== false });
          await rebuild();
        });
      case "saveRules":
        return mutate(sender, async function () {
          if (typeof msg.text !== "string") throw new UserError("Rules must be text.");
          if (utf8Bytes(msg.text) > Core.LIMITS.bytes) throw new UserError("Rules are over the 256 KiB limit.");
          await write({ "shield.userRules": msg.text });
          await rebuild();
        });
      case "saveSiteOff":
        return mutate(sender, async function () {
          var list = normaliseSiteList(msg.list);
          await write({ "shield.siteOff": list });
          await rebuild();
        });
      case "setSiteOff":
        return mutate(sender, async function (settings) {
          var host = typeof msg.host === "string" ? msg.host.trim().toLowerCase() : "";
          var list = settings.siteOff.slice();
          if (msg.off) {
            if (!Core.isValidSite(host)) throw new UserError("\"" + host + "\" cannot be a site rule.");
            var covered = list.some(function (e) {
              return Core.hostMatches(host, e);
            });
            if (!covered) list.push(host);
            if (list.length > SITE_LIMIT) throw new UserError("The list is limited to " + SITE_LIMIT + " sites.");
          } else {
            list = list.filter(function (e) {
              return !Core.hostMatches(host, e);
            });
          }
          await write({ "shield.siteOff": list });
          await rebuild();
        });
      case "setChord":
        return mutate(sender, async function () {
          if (typeof msg.value !== "string" || !chordAccepted(msg.value.trim())) {
            throw new UserError("Use a modifier and a letter, digit or F-key, such as Alt+Shift+S.");
          }
          await write({ "shield.panelChord": msg.value.trim() });
        });
      case "recheck":
        return mutate(sender, async function () {
          await write({ "shield.engineMode": await probeEngine() });
          await rebuild();
        });

      default:
        return { error: "unknown message" };
    }
  }

  chrome.runtime.onMessage.addListener(function (msg, sender, sendResponse) {
    handle(msg, sender).then(sendResponse, function (e) {
      sendResponse({ error: String((e && e.message) || e) });
    });
    return true;
  });
})();
