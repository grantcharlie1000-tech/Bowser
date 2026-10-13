// The toolbar popup: the summary view, in a host that gives the extension a toolbar button.
// Bowser may not show one, which is why the same panel also opens in the page (content script).
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

  ShieldPanel.mount(document.getElementById("root"), { view: "summary", send: send, core: ShieldCore, doc: document });
})();
