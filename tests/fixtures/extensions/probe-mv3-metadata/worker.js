// Service worker for the Bowser Stage 1 probe: writes storage on install and answers one message.
chrome.runtime.onInstalled.addListener(() => {
  chrome.storage.local.set({ probe: 'installed', variant: 'probe-mv3-metadata' });
});

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (!message || message.type !== 'bowser-probe') {
    return false;
  }
  chrome.storage.local
    .get(['probe', 'variant'])
    .then((stored) => sendResponse({ ok: true, from: 'service-worker', id: chrome.runtime.id, stored }))
    .catch((error) => sendResponse({ ok: false, error: String(error) }));
  return true;
});
