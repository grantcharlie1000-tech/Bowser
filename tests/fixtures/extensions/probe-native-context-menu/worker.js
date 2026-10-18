// No hosts/tabs/content scripts. Genuine native context-menu event supplies the target.
// Negative preflight/retry calls are separate from primary gesture injection.
const state = { ready: true, workerInstance: crypto.randomUUID(), events: [], preflight: [], again: null };
let nextEvent = 0;
const inject = async (id, role = 'negative', token = 'preflight-negative') => {
  try {
    await chrome.scripting.executeScript({ target: { tabId: id }, func: (role, token) => document.documentElement.setAttribute('data-bowser-native-command-' + role, token), args: [role, token] });
    return { id, role, token, allowed: true, permissionDenied: false };
  } catch (error) { const text = String(error); return { id, role, token, allowed: false, permissionDenied: /manifest must request permission|missing host permission|cannot access contents[^\n]*permission/i.test(text) && !/no tab|no frame|closed|invalidated/i.test(text), error: text }; }
};
const itemId = 'bowser-native-context-probe';
state.creation = null;
chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({ id: itemId, title: 'Bowser native context permission probe', contexts: ['editable'] }, () => {
    const error = chrome.runtime.lastError?.message;
    state.creation = { complete: true, ok: !error, error: error || null };
  });
});
// Only a genuine extension-created native menu invocation can perform PRIMARY injection.
chrome.contextMenus.onClicked.addListener((info, tab) => {
  const event = { eventKind: 'contextMenus.onClicked', info: { menuItemId: info.menuItemId, editable: info.editable, frameId: info.frameId }, primaryToken: 'native-context-primary-' + (++nextEvent), nativeTab: tab ? { id: tab.id, windowId: tab.windowId, active: tab.active, url: tab.url } : null };
  state.events.push(event);
  if (state.events.length > 4) state.events.shift();
  (async () => { try {
    if (info.menuItemId !== itemId || info.editable !== true || !Number.isInteger(tab?.id)) {
      event.injection = { attempted: false, allowed: false, error: 'Native context event did not supply the exact item, editable context and tab identity' };
      return;
    }
    event.injection = { ...await inject(tab.id, 'primary', event.primaryToken), attempted: true };
    event.currentNativeTab = await chrome.tabs.get(tab.id);
    event.otherTabAttempts = [];
    for (const entry of state.preflight.filter((entry) => entry.id !== tab.id)) event.otherTabAttempts.push(await inject(entry.id, 'negative', 'native-context-negative-' + nextEvent));
  } catch (error) { event.error = String(error); } finally { event.complete = true; } })();
});
chrome.runtime.onMessage.addListener((message, _sender, reply) => {
  if (!message || !['native-command-state', 'native-command-preflight', 'native-command-again'].includes(message.type)) return false;
  (async () => {
    if (message.type === 'native-command-preflight') {
      const tabs = await chrome.tabs.query({});
      // Negative injection attempts against native tab IDs, including redacted browsing tabs.
      // These URLs never establish a Bowser logical-tab identity or an action target.
      state.preflight = [];
      for (const tab of tabs.filter((tab) => !(tab.url || '').startsWith('chrome-extension:')).slice(0, 16)) state.preflight.push(await inject(tab.id));
    }
    if (message.type === 'native-command-again') {
      const id = state.events.at(-1)?.nativeTab?.id;
      if (Number.isInteger(id)) { const currentNativeTab = await chrome.tabs.get(id); state.again = { ...await inject(id), currentNativeTab }; } else { state.again = { allowed: false, permissionDenied: false, error: 'No previously event-supplied native tab' }; }
    }
    reply({ ...state });
  })().catch((error) => reply({ error: String(error) }));
  return true;
});
