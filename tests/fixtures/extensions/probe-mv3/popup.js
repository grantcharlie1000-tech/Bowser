// Page script for the Bowser Stage 2 probe's popup and options pages. Reports what the hosted page
// can see of chrome.* as JSON in data-bowser-popup on <html>, so a test can read it back. Never
// touches the network.
(() => {
  const root = document.documentElement;
  const report = { page: location.pathname, origin: location.origin };
  const publish = () => root.setAttribute('data-bowser-popup', JSON.stringify(report));

  report.chrome = typeof chrome;
  report.runtime = typeof chrome !== 'undefined' && !!chrome.runtime;
  report.runtimeId = report.runtime ? chrome.runtime.id : null;
  report.getUrl = report.runtime ? chrome.runtime.getURL('popup.html') : null;
  report.storageApi = typeof chrome !== 'undefined' && !!chrome.storage && !!chrome.storage.local;
  report.tabsApi = typeof chrome !== 'undefined' && !!chrome.tabs;
  // Bowser gives hosted pages no message channel. WebView2 may still define an object here, so
  // report what is on it instead of only whether it exists.
  const webview = typeof window.chrome !== 'undefined' ? window.chrome.webview : undefined;
  report.webview = typeof webview;
  report.webviewKeys = webview ? Object.keys(webview).sort() : [];
  report.webviewPostMessage = webview ? typeof webview.postMessage : 'n/a';
  report.webviewHostObjects = webview ? typeof webview.hostObjects : 'n/a';
  // Leak probe: does a message sent through the object reach the host, and is any host object
  // callable? The test observes the host side; this reports what the page saw happen.
  report.leak = { marker: 'BOWSER-LEAK-PROBE-7f3a91', postMessage: 'not-run', hostObjects: [] };
  if (webview) {
    try {
      webview.postMessage(report.leak.marker);
      report.leak.postMessage = 'returned';
    } catch (error) {
      report.leak.postMessage = 'threw: ' + String(error);
    }
    const hostObjects = webview.hostObjects;
    report.leak.hostObjectsKeys = hostObjects ? Object.getOwnPropertyNames(hostObjects).sort() : [];
    // Whatever name an object would be registered under, ask for it and try to call into it.
    for (const name of ['bowser', 'host', 'app', 'window', 'tab', 'tabs', 'bridge', 'native']) {
      const entry = { name };
      try {
        const sync = hostObjects && hostObjects.sync && hostObjects.sync[name];
        entry.syncType = typeof sync;
        entry.syncCall = typeof sync === 'function' ? String(sync()) : 'not-callable';
      } catch (error) {
        entry.syncCall = 'threw: ' + String(error).slice(0, 120);
      }
      report.leak.hostObjects.push(entry);
      try {
        Promise.resolve(hostObjects && hostObjects[name] && hostObjects[name].anything())
          .then((value) => { entry.asyncCall = 'resolved: ' + String(value).slice(0, 120); publish(); })
          .catch((error) => { entry.asyncCall = 'rejected: ' + String(error).slice(0, 120); publish(); });
      } catch (error) {
        entry.asyncCall = 'threw: ' + String(error).slice(0, 120);
      }
    }
  }
  publish();

  const readStorage = report.storageApi
    ? chrome.storage.local
        .get(['probe', 'variant'])
        .then((stored) => {
          report.stored = stored;
        })
        .catch((error) => {
          report.storageError = String(error);
        })
    : Promise.resolve();
  const queryTabs = report.tabsApi
    ? chrome.tabs
        .query({})
        .then((tabs) => {
          report.tabsQueryCount = tabs.length;
          // Shape only: the probe has no `tabs` permission, and no URL or title is recorded.
          report.tabsQuery = tabs.map((tab) => ({
            id: tab.id,
            windowId: tab.windowId,
            active: tab.active,
            incognito: tab.incognito,
            hasUrl: typeof tab.url,
          }));
        })
        .catch((error) => {
          report.tabsQueryError = String(error);
        })
    : Promise.resolve();
  Promise.all([readStorage, queryTabs]).then(() => {
    report.ready = true;
    publish();
  });

  document.getElementById('send').addEventListener('click', () => {
    try {
      chrome.runtime.sendMessage({ type: 'bowser-probe', variant: 'popup' }, (reply) => {
        report.sendError = chrome.runtime.lastError ? String(chrome.runtime.lastError.message) : null;
        report.sendReply = reply || null;
        report.sendDone = true;
        publish();
      });
    } catch (error) {
      report.sendError = String(error);
      report.sendDone = true;
      publish();
    }
  });
})();
