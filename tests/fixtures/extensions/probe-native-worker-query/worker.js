// No host permissions. Test the exact Bitwarden-style worker query BEFORE any injection.
// The event tab is comparison evidence only, never a fallback injection target.
const state = { ready: true, events: [], preflight: [], again: null };
let nextEvent = 0;
const inject = async (id, role = 'negative', token = 'preflight-negative') => {
  try {
    await chrome.scripting.executeScript({ target: { tabId: id }, func: (role, token) => document.documentElement.setAttribute('data-bowser-native-command-' + role, token), args: [role, token] });
    return { id, role, token, allowed: true, permissionDenied: false };
  } catch (error) { const text = String(error); return { id, role, token, allowed: false, permissionDenied: /manifest must request permission|missing host permission|cannot access contents[^\n]*permission/i.test(text) && !/no tab|no frame|closed|invalidated/i.test(text), error: text }; }
};
if (chrome.commands && chrome.commands.onCommand) chrome.commands.onCommand.addListener((command, tab) => {
  const event = { command, primaryToken: 'native-query-primary-' + (++nextEvent), nativeTab: tab ? { id: tab.id, windowId: tab.windowId, active: tab.active, url: tab.url } : null };
  state.events.push(event);
  if (state.events.length > 4) state.events.shift();
  (async () => { try {
    event.operationOrder = ['query-started'];
    event.workerQuery = { active: true, windowId: chrome.windows.WINDOW_ID_CURRENT };
    try { event.workerQueryTabs = await chrome.tabs.query(event.workerQuery); }
    catch (error) { event.operationOrder.push('query-failed'); event.workerQueryError = String(error); throw error; }
    event.workerQueryComplete = true;
    event.operationOrder.push('query-settled');
    const queriedId = event.workerQueryTabs.length === 1 ? event.workerQueryTabs[0].id : null;
    event.queryNativeId = Number.isInteger(queriedId) ? queriedId : null;
    if (Number.isInteger(queriedId)) {
      event.operationOrder.push('primary-attempt');
      event.injection = { ...await inject(queriedId, 'primary', event.primaryToken), attempted: true };
    } else {
      event.operationOrder.push('primary-not-attempted');
      event.injection = { attempted: false, allowed: false, error: 'Exact worker query supplied no unique native tab identity' };
    }
    event.currentNativeTab = Number.isInteger(tab?.id) ? await chrome.tabs.get(tab.id) : null;
    event.nativeQueries = { currentWindow: await chrome.tabs.query({ active: true, currentWindow: true }), lastFocusedWindow: await chrome.tabs.query({ active: true, lastFocusedWindow: true }) };
    const candidates = state.preflight.filter((entry) => entry.id !== tab?.id);
    event.otherTabAttempts = [];
    for (const entry of candidates) event.otherTabAttempts.push(await inject(entry.id, 'negative', 'native-negative-' + nextEvent));
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
      const id = state.events.at(-1)?.queryNativeId;
      if (Number.isInteger(id)) { const currentNativeTab = await chrome.tabs.get(id); state.again = { ...await inject(id), currentNativeTab }; } else { state.again = { allowed: false, permissionDenied: false, error: 'No previously query-selected native tab' }; }
    }
    reply({ ...state, commands: await chrome.commands.getAll() });
  })().catch((error) => reply({ error: String(error) }));
  return true;
});
