// Options page: the same controls as the popup plus the limitations text.
"use strict";

(function () {
  var ui = SFWUI.mountControls(document.getElementById("controls"));
  if (new URLSearchParams(location.search).get("welcome") === "1") {
    document.getElementById("welcome").hidden = false;
  }
  SFWUI.load().then(function (settings) { ui.setState(settings); }).catch(function () {});
  chrome.storage.onChanged.addListener(function (changes, area) {
    if (area === "local") SFWUI.load().then(function (settings) { ui.setState(settings); });
  });
})();
