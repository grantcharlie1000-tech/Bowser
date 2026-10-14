// Bowser Shield content script. Runs at document_start in every http(s) frame, after
// lib/shield-core.js and lib/shield-chord.js (the manifest lists them in that order).
//
// It hides ad slots with CSS in both modes, strips blocked URLs out of the DOM in dom mode only,
// and hosts the in-page panel on a keyboard chord. The chord and the panel work on every page,
// including sites that are never filtered and while the shield is off, so the shield can always be
// turned back on from the page.
//
// The panel (ui/panel.js and its stylesheet, about 34 KB) is not part of this script. The first
// chord press in the top frame asks the service worker to inject them into this same isolated world
// (ensurePanel); every frame carries only the chord helpers.

(function () {
  "use strict";

  var Core = self.ShieldCore;
  var Chord = self.ShieldChord;
  if (!Core || !Chord) return;

  var STYLE_ID = "bowser-shield";
  var DEFAULT_CHORD = "Alt+Shift+S";
  var SEEN_LIMIT = 2048;
  var FLUSH_MS = 250;
  var REFRESH_MS = 50;
  var STORAGE_KEYS = [
    "shield.enabled",
    "shield.starterEnabled",
    "shield.userRules",
    "shield.siteOff",
    "shield.cosmetic",
    "shield.engineMode",
    "shield.panelChord"
  ];
  var STARTER_PATH = "rules/starter.srl";
  var HOST_LAYOUT = {
    all: "initial",
    position: "fixed",
    top: "12px",
    right: "12px",
    "z-index": "2147483647",
    display: "block"
  };
  var isTop = window === window.top;

  var style = null;
  var observer = null;
  var matcher = null;
  var scrubHost = "";
  var sweepHooked = false;
  var chord = Chord.parseChord(DEFAULT_CHORD);
  var overlay = null;
  var panelLoading = false;
  var generation = 0;
  var refreshTimer = null;
  var badReported = 0;
  var packagedStarter = null;

  var seen = new Set();
  var pending = [];
  var flushScheduled = false;
  var lastFlush = 0;

  function noop() {}

  // ---- talking to the background ----------------------------------------------------------------

  // Resolves with the reply, rejects when there is no service worker to answer.
  function ask(message) {
    return new Promise(function (resolve, reject) {
      try {
        chrome.runtime.sendMessage(message, function (reply) {
          if (chrome.runtime.lastError || reply === undefined) reject(new Error("no reply"));
          else resolve(reply);
        });
      } catch (e) {
        reject(e);
      }
    });
  }

  function notify(message) {
    try {
      chrome.runtime.sendMessage(message, function () {
        void chrome.runtime.lastError;
      });
    } catch (e) {
      // The background is gone; nothing depends on this report.
    }
  }

  async function readStorage() {
    try {
      return await chrome.storage.local.get(STORAGE_KEYS);
    } catch (e) {
      return {};
    }
  }

  // ---- the style element and the cosmetic layer (section 6.1 and 6.2) ----------------------------

  function createStyle() {
    style = document.createElement("style");
    style.setAttribute("id", STYLE_ID);
    attachStyle();
    if (!style.parentNode) document.addEventListener("DOMContentLoaded", attachStyle);
  }

  function attachStyle() {
    var root = document.documentElement;
    if (root && style && style.parentNode !== root) root.appendChild(style);
  }

  function removeStyle() {
    if (style && style.parentNode) style.parentNode.removeChild(style);
  }

  // One rule per selector, so a selector the engine refuses takes only itself down.
  function applyCosmetic(selectors) {
    var sheet = style && style.sheet;
    if (!sheet) return;
    while (sheet.cssRules.length > 0) sheet.deleteRule(sheet.cssRules.length - 1);
    var bad = 0;
    for (var i = 0; i < selectors.length; i++) {
      try {
        sheet.insertRule(selectors[i] + "{display:none!important}", sheet.cssRules.length);
      } catch (e) {
        bad++;
      }
    }
    if (bad !== badReported) {
      badReported = bad;
      notify({ type: "badSelectors", count: bad });
    }
  }

  // ---- the dom-mode scrubber (section 6.4) ------------------------------------------------------

  var URL_ATTRS = {
    IMG: ["src", "data-src"],
    SCRIPT: ["src", "data-src"],
    IFRAME: ["src", "data-src"],
    SOURCE: ["src", "data-src"],
    EMBED: ["src"],
    OBJECT: ["data"],
    VIDEO: ["src", "data-src"],
    AUDIO: ["src", "data-src"],
    TRACK: ["src"],
    LINK: ["href"]
  };
  var URL_SELECTOR = "img,script,iframe,source,embed,object,video,audio,track,link";
  var LINK_RELS = { preload: 1, prefetch: 1, preconnect: 1, stylesheet: 1 };
  var LINK_AS = { script: "script", style: "style", font: "font", image: "image", fetch: "xhr", document: "frame", iframe: "frame", audio: "media", video: "media", track: "media" };

  function relTokens(el) {
    return String(el.getAttribute("rel") || "").toLowerCase().split(/\s+/);
  }

  function linkRelevant(el) {
    var rels = relTokens(el);
    for (var i = 0; i < rels.length; i++) if (Object.prototype.hasOwnProperty.call(LINK_RELS, rels[i])) return true;
    return false;
  }

  function requestType(el, tag) {
    switch (tag) {
      case "IMG":
        return "image";
      case "SCRIPT":
        return "script";
      case "IFRAME":
        return "frame";
      case "VIDEO":
      case "AUDIO":
      case "TRACK":
        return "media";
      case "SOURCE":
        return el.parentNode && String(el.parentNode.tagName).toUpperCase() === "PICTURE" ? "image" : "media";
      case "LINK": {
        var as = String(el.getAttribute("as") || "").toLowerCase();
        if (Object.prototype.hasOwnProperty.call(LINK_AS, as)) return LINK_AS[as];
        return relTokens(el).indexOf("stylesheet") >= 0 ? "style" : "other";
      }
      default:
        return "other";
    }
  }

  function resolveUrl(raw) {
    try {
      return new URL(raw, document.baseURI).href;
    } catch (e) {
      return null;
    }
  }

  // The same URL appearing on two elements, or an attribute the page rewrites over and over,
  // is counted once. Oldest entries leave first.
  function remember(url) {
    if (seen.has(url)) return false;
    seen.add(url);
    if (seen.size > SEEN_LIMIT) seen.delete(seen.values().next().value);
    return true;
  }

  function scheduleFlush() {
    if (flushScheduled) return;
    flushScheduled = true;
    var go = function () {
      setTimeout(flush, Math.max(0, lastFlush + FLUSH_MS - Date.now()));
    };
    if (typeof window.requestAnimationFrame === "function") window.requestAnimationFrame(go);
    else go();
  }

  function flush() {
    flushScheduled = false;
    lastFlush = Date.now();
    if (pending.length === 0) return;
    var urls = pending;
    pending = [];
    notify({ type: "hidden", urls: urls });
  }

  function scrubElement(el, onlyAttr) {
    var tag = String(el.tagName || "").toUpperCase();
    var attrs = Object.prototype.hasOwnProperty.call(URL_ATTRS, tag) ? URL_ATTRS[tag] : null;
    if (!attrs) return;
    if (tag === "LINK" && !linkRelevant(el)) return;
    for (var i = 0; i < attrs.length; i++) {
      var attr = attrs[i];
      if (onlyAttr && attr !== onlyAttr) continue;
      var raw = el.getAttribute(attr);
      if (!raw) continue;
      var url = resolveUrl(raw);
      if (!url) continue;
      var verdict = Core.decide(matcher, { url: url, siteHost: scrubHost, type: requestType(el, tag) });
      if (verdict.verdict !== "block") continue;
      el.removeAttribute(attr);
      el.hidden = true;
      el.style.display = "none";
      if (remember(url)) {
        pending.push(url);
        scheduleFlush();
      }
    }
  }

  function scrubTree(node) {
    if (!node || node.nodeType === 3 || !node.tagName) return;
    scrubElement(node);
    if (node.querySelectorAll) {
      var found = node.querySelectorAll(URL_SELECTOR);
      for (var i = 0; i < found.length; i++) scrubElement(found[i]);
    }
  }

  function onMutations(records) {
    if (!matcher) return;
    for (var i = 0; i < records.length; i++) {
      var record = records[i];
      if (record.type === "attributes") {
        scrubElement(record.target, record.attributeName);
      } else {
        for (var k = 0; k < record.addedNodes.length; k++) scrubTree(record.addedNodes[k]);
      }
    }
  }

  function sweep() {
    if (observer) scrubTree(document.documentElement);
  }

  function startScrubber(compiled, siteHost) {
    stopScrubber();
    matcher = compiled;
    scrubHost = siteHost;
    observer = new MutationObserver(onMutations);
    observer.observe(document.documentElement || document, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["src", "href", "data-src"]
    });
    if (document.readyState === "loading") {
      if (!sweepHooked) {
        sweepHooked = true;
        document.addEventListener("DOMContentLoaded", sweep);
      }
    } else {
      sweep();
    }
  }

  function stopScrubber() {
    if (observer) observer.disconnect();
    observer = null;
    matcher = null;
    pending = [];
  }

  // ---- deciding what to do on this page (sections 6.2 and 6.3) -----------------------------------

  function siteList(stored) {
    var list = stored["shield.siteOff"];
    return Array.isArray(list) ? list : [];
  }

  function offLines(stored) {
    return siteList(stored)
      .filter(function (h) {
        return typeof h === "string" && Core.isValidSite(h);
      })
      .map(function (h) {
        return "off site:" + h;
      })
      .join("\n");
  }

  function userRules(stored) {
    return typeof stored["shield.userRules"] === "string" ? stored["shield.userRules"] : "";
  }

  // With no service worker to hand the list over, read the copy packaged with the extension. Kept for
  // the life of this content script; a failed read is not cached, and leaves the user's rules alone.
  async function loadPackagedStarter() {
    if (packagedStarter !== null) return packagedStarter;
    try {
      var response = await fetch(chrome.runtime.getURL(STARTER_PATH));
      if (!response || !response.ok) return "";
      var text = await response.text();
      if (typeof text !== "string") return "";
      packagedStarter = text;
      return text;
    } catch (e) {
      return "";
    }
  }

  async function refresh() {
    var gen = ++generation;
    var results = await Promise.all([
      ask({ type: "frameContext" }).catch(function () {
        return null;
      }),
      readStorage()
    ]);
    if (gen !== generation) return;
    var ctx = results[0];
    var stored = results[1];
    chord = Chord.parseChord(stored["shield.panelChord"]) || Chord.parseChord(DEFAULT_CHORD);

    var enabled, siteHost, mode, filtered;
    var declineSiteRules = false;
    var starterSrl = "";
    if (ctx) {
      enabled = ctx.enabled !== false;
      siteHost = ctx.siteHost || "";
      mode = ctx.mode === "dom" ? "dom" : "network";
      filtered = !!ctx.filtered;
      starterSrl = typeof ctx.starterSrl === "string" ? ctx.starterSrl : "";
    } else {
      // No service worker. Read storage directly. A subframe cannot see the top-level host, so it
      // cannot verify site exceptions and applies only the global cosmetic rules.
      enabled = stored["shield.enabled"] !== false;
      siteHost = String(location.hostname || "").toLowerCase();
      mode = stored["shield.engineMode"] === "network" ? "network" : "dom";
      declineSiteRules = !isTop;
      filtered = enabled;
      if (filtered && isTop) {
        var local = Core.compileAll([
          { tag: "site", src: offLines(stored) },
          { tag: "user", src: userRules(stored) }
        ]);
        filtered = !Core.siteIsOff(local, siteHost);
      }
    }

    if (!filtered) {
      removeStyle();
      stopScrubber();
      return;
    }

    attachStyle();
    var cosmetic = stored["shield.cosmetic"];
    applyCosmetic(declineSiteRules ? (cosmetic && cosmetic.global) || [] : Core.cosmeticFor(cosmetic, siteHost));

    if (mode === "dom" && !declineSiteRules) {
      if (!ctx && stored["shield.starterEnabled"] !== false) {
        starterSrl = await loadPackagedStarter();
        if (gen !== generation) return;
      }
      startScrubber(
        Core.compileAll([
          { tag: "starter", src: starterSrl },
          { tag: "site", src: offLines(stored) },
          { tag: "user", src: userRules(stored) }
        ]),
        siteHost
      );
    } else {
      stopScrubber();
    }
  }

  function scheduleRefresh() {
    clearTimeout(refreshTimer);
    refreshTimer = setTimeout(function () {
      refresh().catch(noop);
    }, REFRESH_MS);
  }

  // ---- the panel overlay and its chord (section 7.1) ---------------------------------------------

  function deepActive() {
    var el = document.activeElement;
    while (el && el.shadowRoot && el.shadowRoot.activeElement) el = el.shadowRoot.activeElement;
    return el;
  }

  // Resolves with the panel renderer once it is in this frame. Asks the service worker to inject
  // content/panel-css.js and ui/panel.js the first time; resolves null when it cannot (no worker, or
  // no scripting API in this runtime), in which case the chord does nothing.
  function ensurePanel() {
    if (self.ShieldPanel && self.SHIELD_PANEL_CSS !== undefined) return Promise.resolve(self.ShieldPanel);
    return ask({ type: "loadPanel" }).then(
      function (reply) {
        return reply && reply.ok && self.ShieldPanel && self.SHIELD_PANEL_CSS !== undefined ? self.ShieldPanel : null;
      },
      function () {
        return null;
      }
    );
  }

  // Constructable stylesheets are not subject to the page's CSP; a <style> element is the fallback.
  function adoptCss(root) {
    var css = self.SHIELD_PANEL_CSS || "";
    try {
      if (typeof CSSStyleSheet === "function" && "adoptedStyleSheets" in root) {
        var sheet = new CSSStyleSheet();
        sheet.replaceSync(css);
        root.adoptedStyleSheets = [sheet];
        return;
      }
    } catch (e) {
      // Fall through to the element.
    }
    var el = document.createElement("style");
    el.textContent = css;
    root.appendChild(el);
  }

  function closePanel() {
    if (!overlay) return;
    var closing = overlay;
    overlay = null;
    if (closing.host.parentNode) closing.host.parentNode.removeChild(closing.host);
    try {
      if (closing.previous && closing.previous.focus) closing.previous.focus();
    } catch (e) {
      // The element left the page while the panel was open.
    }
  }

  function openPanel() {
    if (overlay) {
      closePanel();
      return;
    }
    if (panelLoading) return;
    var previous = document.activeElement;
    panelLoading = true;
    ensurePanel().then(function (Panel) {
      panelLoading = false;
      if (Panel && !overlay) showPanel(Panel, previous);
    });
  }

  function showPanel(Panel, previous) {
    var host = document.createElement("div");
    // No attribute for a page to select, and important priority so a page stylesheet cannot hide it.
    for (var name in HOST_LAYOUT) host.style.setProperty(name, HOST_LAYOUT[name], "important");
    var root = host.attachShadow({ mode: "closed" });
    adoptCss(root);
    var mountPoint = document.createElement("div");
    mountPoint.className = "bs-overlay";
    mountPoint.setAttribute("role", "dialog");
    mountPoint.setAttribute("aria-label", "Bowser Shield");
    root.appendChild(mountPoint);
    (document.documentElement || document).appendChild(host);
    overlay = { host: host, root: root, previous: previous };

    Panel.mount(mountPoint, { view: "summary", send: ask, onClose: closePanel, core: Core, doc: document }).then(function () {
      var first = root.querySelector && root.querySelector("[data-key=enabled]");
      if (first && first.focus && overlay && overlay.host === host) first.focus();
    }, noop);
  }

  function onKey(e) {
    if (overlay && e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      closePanel();
      return;
    }
    if (!Chord.matchChord(chord, e)) return;
    if (Chord.isEditable(deepActive())) return;
    e.preventDefault();
    e.stopPropagation();
    if (isTop) openPanel();
    else notify({ type: "chord" });
  }

  function onOutside(e) {
    if (!overlay) return;
    var path = typeof e.composedPath === "function" ? e.composedPath() : [e.target];
    if (path.indexOf(overlay.host) < 0 && e.target !== overlay.host) closePanel();
  }

  // ---- start ------------------------------------------------------------------------------------

  createStyle();
  // On window, not document: a page listener on window in the capture phase runs before one on
  // document, and could stop the chord before it arrives.
  window.addEventListener("keydown", onKey, true);
  // Only the top frame ever shows the overlay, so only it listens for clicks outside and for the
  // worker's request to open it (a chord pressed in a subframe is forwarded to the top frame).
  if (isTop) {
    window.addEventListener("pointerdown", onOutside, true);
    chrome.runtime.onMessage.addListener(function (message) {
      if (message && message.type === "togglePanel") openPanel();
    });
  }

  // Only the settings this script reads trigger a refresh; the worker's own bookkeeping (the compiled
  // list, counters) must not make every frame ask it for context again.
  chrome.storage.onChanged.addListener(function (changes, area) {
    if (area !== "local") return;
    for (var key in changes) {
      if (STORAGE_KEYS.indexOf(key) >= 0) {
        scheduleRefresh();
        return;
      }
    }
  });

  refresh().catch(noop);
})();
