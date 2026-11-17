// Shared UI logic for popup.html and options.html. Classic script, no dependencies.
// All text is set with textContent / createElement; nothing dynamic goes through innerHTML.
// This UI never displays page text; it only knows settings and numeric counters.
"use strict";

var SFWUI = (function () {
  var DEFAULTS = { enabled: true, mode: "strict", reducedEffects: false, ocr: false, feedEnabled: true, feed: null, feedStatus: null };
  var KEYS = ["enabled", "mode", "reducedEffects", "ocr", "feedEnabled", "feed", "feedStatus"];
  // The one optional permission behind the image-text scan (a hidden page that runs the bundled OCR engine). It is
  // requested only when the user switches the scan on (a click, which Chrome requires) and removed again when it is
  // switched off. No host permission is needed: the content scripts already match <all_urls>, which is what lets the
  // extension download the images it reads (verified by test/chrome/ocr-extension.mjs).
  var OCR_PERMS = { permissions: ["offscreen"] };
  var MODES = [
    { id: "lockdown", name: "Lockdown", desc: "Hides anything that could be about GTA VI, and holds images on social sites until they are checked. Most cautious; also hides some harmless posts." },
    { id: "strict", name: "Strict", tag: "Recommended", desc: "Hides GTA VI posts unless they are clearly release news such as dates or prices, and hides likely spoilers in GTA conversations." },
    { id: "balanced", name: "Balanced", desc: "Hides likely spoilers only. Plain mentions of GTA VI stay visible, so a little more can slip through." },
  ];

  function el(tag, attrs, children) {
    var node = document.createElement(tag);
    if (attrs) {
      for (var k in attrs) {
        if (k === "text") node.textContent = attrs[k];
        else if (k === "class") node.className = attrs[k];
        else if (attrs[k] === true) node.setAttribute(k, "");
        else if (attrs[k] !== false && attrs[k] != null) node.setAttribute(k, String(attrs[k]));
      }
    }
    if (children) for (var i = 0; i < children.length; i++) if (children[i]) node.appendChild(children[i]);
    return node;
  }

  function normalize(values) {
    var v = values || {};
    return {
      enabled: v.enabled !== false,
      mode: MODES.some(function (m) { return m.id === v.mode; }) ? v.mode : DEFAULTS.mode,
      reducedEffects: v.reducedEffects === true,
      ocr: v.ocr === true,
      feedEnabled: v.feedEnabled !== false,
      feed: v.feed && typeof v.feed === "object" ? v.feed : null,
      feedStatus: v.feedStatus && typeof v.feedStatus === "object" ? v.feedStatus : null,
    };
  }

  // The scan counts as on only while the permissions are really held (Chrome lets users revoke them on the
  // extensions page); a stale "on" is corrected here.
  function load() {
    return chrome.storage.local.get(KEYS).then(normalize).then(function (v) {
      if (!v.ocr) return v;
      return chrome.permissions.contains(OCR_PERMS).then(function (ok) {
        if (!ok) { v.ocr = false; chrome.storage.local.set({ ocr: false }); }
        return v;
      }, function () { return v; });
    });
  }

  function save(patch) {
    return chrome.storage.local.set(patch);
  }

  var SVG_NS = "http://www.w3.org/2000/svg";
  function statusIcon(off) {
    var svg = document.createElementNS(SVG_NS, "svg");
    svg.setAttribute("viewBox", "0 0 24 24");
    svg.setAttribute("aria-hidden", "true");
    svg.setAttribute("fill", "none");
    svg.setAttribute("stroke", "currentColor");
    svg.setAttribute("stroke-width", "2");
    svg.setAttribute("stroke-linecap", "round");
    svg.setAttribute("stroke-linejoin", "round");
    var shield = document.createElementNS(SVG_NS, "path");
    shield.setAttribute("d", "M12 3l7 3v5c0 4.6-3 8.3-7 10-4-1.7-7-5.4-7-10V6z");
    svg.appendChild(shield);
    var mark = document.createElementNS(SVG_NS, "path");
    mark.setAttribute("d", off ? "M9 9l6 6M15 9l-6 6" : "M9 12l2.2 2.2L15.5 9.8");
    svg.appendChild(mark);
    return svg;
  }

  // Builds the status + mode + effects + off/on controls into `root` and keeps them in sync.
  // opts.ocr: "request" (default; the switch asks for the permissions itself) or "redirect" (popup: a permission prompt
  // would close the popup, so turning the scan on opens the settings page where the same switch lives).
  function mountControls(root, onChange, opts) {
    var ocrMode = (opts && opts.ocr) || "request";
    var state = Object.assign({}, DEFAULTS);

    var statusIconBox = el("div", { class: "status-icon" });
    var statusTitle = el("h1", { id: "status-title" });
    var statusText = el("p", { id: "status-text" });
    var status = el("section", { class: "card status", id: "status", "aria-labelledby": "status-title" }, [
      statusIconBox,
      el("div", null, [statusTitle, statusText]),
    ]);

    var radios = {};
    var modeBoxes = MODES.map(function (m) {
      var input = el("input", { type: "radio", name: "mode", id: "mode-" + m.id, value: m.id });
      radios[m.id] = input;
      input.addEventListener("change", function () { if (input.checked) change({ mode: m.id }); });
      var nameNode = el("span", { class: "name" }, [document.createTextNode(m.name)]);
      if (m.tag) nameNode.appendChild(el("span", { class: "tag", text: m.tag }));
      var label = el("label", { for: "mode-" + m.id }, [
        el("span", { class: "dot", "aria-hidden": "true" }),
        el("span", null, [nameNode, el("span", { class: "desc", text: m.desc })]),
      ]);
      return el("div", { class: "mode" }, [input, label]);
    });
    var modeSet = el("fieldset", { id: "modes" }, [el("legend", { text: "How cautious should it be?" }), el("div", { class: "modes" }, modeBoxes)]);
    var modeCard = el("section", { class: "card" }, [modeSet]);

    var effects = el("input", { type: "checkbox", id: "reduced", role: "switch" });
    effects.addEventListener("change", function () { change({ reducedEffects: effects.checked }); });
    var effectsCard = el("section", { class: "card toggle" }, [
      el("div", null, [
        el("label", { class: "name", for: "reduced", text: "Reduced visual effects" }),
        el("div", { class: "desc", text: "Uses a plain solid cover instead of blur and animation. Lighter on slow computers." }),
      ]),
      el("span", { class: "switch" }, [effects, el("span", { class: "track", "aria-hidden": "true" })]),
    ]);

    var ocrBox = el("input", { type: "checkbox", id: "ocr", role: "switch", "aria-describedby": "ocr-desc ocr-note" });
    var ocrNote = el("p", { class: "desc note-line", id: "ocr-note", role: "status", hidden: true });
    var ocrCard = el("section", { class: "card toggle", id: "ocr-card" }, [
      el("div", null, [
        el("label", { class: "name", for: "ocr", text: "Scan images for text (local OCR)" }),
        el("div", { class: "desc", id: "ocr-desc", text: "Reads text inside pictures and memes so a spoiler written in an image can be covered. It runs on your device and nothing is uploaded. Turning it on asks Chrome to let this extension run a hidden background page that does the reading. Off until you allow it." }),
        ocrNote,
      ]),
      el("span", { class: "switch" }, [ocrBox, el("span", { class: "track", "aria-hidden": "true" })]),
    ]);
    function ocrSay(message) { ocrNote.textContent = message || ""; ocrNote.hidden = !message; }
    ocrBox.addEventListener("change", function () {
      if (ocrBox.checked) {
        if (ocrMode === "redirect") {
          ocrBox.checked = false;
          try { chrome.tabs.create({ url: chrome.runtime.getURL("options.html?ocr=1#ocr-card") }); } catch (e) { /* ignore */ }
          ocrSay("Opening settings: Chrome asks for permission there.");
          return;
        }
        var asked;
        try { asked = chrome.permissions.request(OCR_PERMS); } catch (e) { asked = Promise.reject(e); } // called inside the click, as Chrome requires
        asked.then(function (granted) {
          if (!granted) { ocrBox.checked = false; ocrSay("Permission was not granted, so image scanning stays off."); return; }
          ocrSay("");
          return change({ ocr: true });
        }, function () { ocrBox.checked = false; ocrSay("Chrome could not show the permission request. Image scanning stays off."); });
      } else {
        ocrSay("");
        change({ ocr: false }).then(function () { return chrome.permissions.remove(OCR_PERMS); }).catch(function () { /* nothing to remove */ });
      }
    });

    // Spoiler term updates: a small signed word list that can only ADD protection, downloaded twice a day.
    var feedBox = el("input", { type: "checkbox", id: "feed", role: "switch", "aria-describedby": "feed-desc feed-status" });
    var feedStatusLine = el("p", { class: "desc note-line", id: "feed-status", role: "status" });
    var feedCard = el("section", { class: "card toggle", id: "feed-card" }, [
      el("div", null, [
        el("label", { class: "name", for: "feed", text: "Download spoiler term updates" }),
        el("div", { class: "desc", id: "feed-desc", text: "Fetches a small signed word list about every 12 hours so new names and missions are covered before the extension itself is updated. It sends no browsing data and can only add protection. Turning it off keeps the built-in list; an update already downloaded stays in use." }),
        feedStatusLine,
      ]),
      el("span", { class: "switch" }, [feedBox, el("span", { class: "track", "aria-hidden": "true" })]),
    ]);
    feedBox.addEventListener("change", function () { change({ feedEnabled: feedBox.checked }); });

    var offBtn = el("button", { type: "button", class: "btn danger full", id: "off-btn", text: "Turn off protection…", "aria-expanded": "false", "aria-controls": "confirm" });
    var onBtn = el("button", { type: "button", class: "btn primary full", id: "on-btn", text: "Turn protection back on", hidden: true });
    var keepBtn = el("button", { type: "button", class: "btn", id: "keep-btn", text: "Keep protection on" });
    var confirmBtn = el("button", { type: "button", class: "btn danger solid", id: "confirm-btn", text: "Turn off" });
    var confirmBox = el("div", { class: "confirm", id: "confirm", role: "group", "aria-labelledby": "confirm-title", hidden: true }, [
      el("p", { id: "confirm-title", text: "Spoilers will no longer be hidden on any page until you turn protection back on. Turn it off?" }),
      el("div", { class: "row" }, [keepBtn, confirmBtn]),
    ]);
    var errorBox = el("p", { class: "error", id: "error", role: "alert", hidden: true });
    var offCard = el("section", { class: "card" }, [offBtn, onBtn, confirmBox, errorBox]);

    function openConfirm() {
      confirmBox.hidden = false;
      offBtn.setAttribute("aria-expanded", "true");
      keepBtn.focus();
    }
    function closeConfirm(focusOff) {
      confirmBox.hidden = true;
      offBtn.setAttribute("aria-expanded", "false");
      if (focusOff) offBtn.focus();
    }
    offBtn.addEventListener("click", openConfirm);
    keepBtn.addEventListener("click", function () { closeConfirm(true); });
    confirmBox.addEventListener("keydown", function (e) { if (e.key === "Escape") { e.stopPropagation(); closeConfirm(true); } });
    confirmBtn.addEventListener("click", function () {
      closeConfirm(false);
      change({ enabled: false }).then(function () { onBtn.focus(); });
    });
    onBtn.addEventListener("click", function () { change({ enabled: true }).then(function () { offBtn.focus(); }); });

    function showError(message) {
      errorBox.textContent = message || "";
      errorBox.hidden = !message;
    }

    function change(patch) {
      return save(patch).then(
        function () { showError(""); Object.assign(state, patch); render(); if (onChange) onChange(state); },
        function () { showError("Could not save the setting. Please try again."); render(); }
      );
    }

    function render() {
      var on = state.enabled;
      status.setAttribute("data-state", on ? "on" : "off");
      statusIconBox.textContent = "";
      statusIconBox.appendChild(statusIcon(!on));
      statusTitle.textContent = on ? "Protection on" : "Protection OFF";
      statusText.textContent = on
        ? "GTA VI spoilers are hidden on the pages you open."
        : "Spoilers are NOT being hidden. Turn protection back on to stay safe.";
      for (var id in radios) radios[id].checked = state.mode === id;
      modeSet.disabled = !on;
      effects.checked = state.reducedEffects;
      ocrBox.checked = state.ocr;
      ocrBox.disabled = !on;
      feedBox.checked = state.feedEnabled;
      var line = typeof SFWFeed !== "undefined" && typeof SFWFeedConfig !== "undefined"
        ? SFWFeed.describe({ builtin: SFWFeedConfig.BUILTIN_VERSION, enabled: state.feedEnabled, feed: state.feed, status: state.feedStatus, now: Date.now() })
        : null;
      feedStatusLine.textContent = line ? line.text + (line.notes.length ? ". " + line.notes.join(" ") : "") : "";
      feedStatusLine.classList.toggle("warn", !!(line && line.warn));
      offBtn.hidden = !on;
      onBtn.hidden = on;
      if (!on) closeConfirm(false);
    }

    root.appendChild(status);
    root.appendChild(modeCard);
    root.appendChild(effectsCard);
    root.appendChild(ocrCard);
    root.appendChild(feedCard);
    root.appendChild(offCard);

    function setState(values) { state = normalize(values); render(); }
    render();
    return { setState: setState, el: { status: status, modeSet: modeSet } };
  }

  return { el: el, load: load, save: save, normalize: normalize, mountControls: mountControls, KEYS: KEYS };
})();
