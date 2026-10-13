// The options page: the full editor view. It opens in a tab when the host can reach it. Bowser
// blocks navigation to extension pages, so the same view is also reachable from the in-page panel.
(function () {
  "use strict";

  function send(message) {
    return new Promise(function (resolve, reject) {
      chrome.runtime.sendMessage(message, function (reply) {
        if (chrome.runtime.lastError || reply === undefined) reject(new Error("no reply"));
        else resolve(reply);
      });
    });
  }

  ShieldPanel.mount(document.getElementById("root"), { view: "options", send: send, core: ShieldCore, doc: document });
})();
