// Popup: protection status, mode, effects, and the number of shields on the active tab.
"use strict";

(function () {
  var controlsRoot = document.getElementById("controls");
  var pageCard = document.getElementById("page-card");
  var countEl = document.getElementById("count");
  var noticeEl = document.getElementById("page-notice");

  var ui = SFWUI.mountControls(controlsRoot, function (state) { refreshNotice(state.enabled); }, { ocr: "redirect" });
  controlsRoot.insertBefore(pageCard, ui.el.status.nextSibling);

  var tabId = null;
  var record = null;
  var enabled = true;

  function refreshNotice(isEnabled) {
    enabled = isEnabled;
    noticeEl.hidden = true;
    if (tabId === null) return;
    if (!enabled) {
      noticeEl.textContent = "Protection is off, so nothing is being hidden on this page.";
      noticeEl.hidden = false;
    } else if (!record) {
      noticeEl.textContent = "This page has not reported in. Reload it to be sure it is protected. Browser pages and the Chrome Web Store cannot be protected.";
      noticeEl.hidden = false;
    }
  }

  function renderCount() {
    countEl.textContent = String(record && typeof record.shields === "number" ? record.shields : 0);
    refreshNotice(enabled);
  }

  function readRecord() {
    if (tabId === null) return Promise.resolve();
    var key = "tab:" + tabId;
    return chrome.storage.session.get(key).then(function (v) { record = v[key] || null; renderCount(); });
  }

  function findTabId() {
    // ?tabId=N lets automated tests point the popup at a specific tab.
    var fromQuery = Number(new URLSearchParams(location.search).get("tabId"));
    if (Number.isInteger(fromQuery) && fromQuery >= 0 && location.search.indexOf("tabId=") >= 0) return Promise.resolve(fromQuery);
    return chrome.tabs.query({ active: true, currentWindow: true }).then(function (tabs) {
      return tabs[0] && typeof tabs[0].id === "number" ? tabs[0].id : null;
    });
  }

  SFWUI.load().then(function (settings) {
    ui.setState(settings);
    enabled = settings.enabled;
    return findTabId();
  }).then(function (id) {
    tabId = id;
    return readRecord();
  }).catch(function () { renderCount(); });

  chrome.storage.onChanged.addListener(function (changes, area) {
    if (area === "local") {
      SFWUI.load().then(function (settings) { ui.setState(settings); refreshNotice(settings.enabled); });
    } else if (area === "session" && tabId !== null && changes["tab:" + tabId]) {
      record = changes["tab:" + tabId].newValue || null;
      renderCount();
    }
  });
})();
