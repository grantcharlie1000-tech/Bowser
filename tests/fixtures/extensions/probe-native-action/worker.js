// No host permissions. Only an actual native invocation may confer activeTab access.
const state = { ready: true, events: [], preflight: [], again: null, unexpectedCommandEvents: [] };
let nextEvent = 0;
const inject = async (id, role = 'negative', token = 'preflight-negative') => {
  try {
    await chrome.scripting.executeScript({ target: { tabId: id }, func: (role, token) => document.documentElement.setAttribute('data-bowser-native-command-' + role, token), args: [role, token] });
    return { id, role, token, allowed: true, permissionDenied: false };
  } catch (error) { const text = String(error); return { id, role, token, allowed: false, permissionDenied: /manifest must request permission|missing host permission|cannot access contents[^\n]*permission/i.test(text) && !/no tab|no frame|closed|invalidated/i.test(text), error: text }; }
};
chrome.commands.onCommand.addListener(command => { state.unexpectedCommandEvents.push(command); if (state.unexpectedCommandEvents.length > 4) state.unexpectedCommandEvents.shift(); });
// Only this genuine native action event may invoke primary injection; command observations never do.
chrome.action.onClicked.addListener(tab => {
  const event = { eventKind: 'action.onClicked', manifestAssignment: '_execute_action', primaryToken: 'native-primary-' + (++nextEvent), nativeTab: tab ? { id: tab.id, windowId: tab.windowId, active: tab.active, url: tab.url } : null };
  state.events.push(event);
  if (state.events.length > 4) state.events.shift();
  (async () => { try {
    event.injection = Number.isInteger(tab?.id) ? await inject(tab.id, 'primary', event.primaryToken) : { allowed: false, error: 'Native command supplied no tab identity' };
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
      const tab = state.events.at(-1)?.nativeTab;
      if (Number.isInteger(tab?.id)) { const currentNativeTab = await chrome.tabs.get(tab.id); state.again = { ...await inject(tab.id), currentNativeTab }; } else { state.again = { allowed: false, permissionDenied: false, error: 'No previously supplied native command tab' }; }
    }
    reply({ ...state, commands: await chrome.commands.getAll() });
  })().catch((error) => reply({ error: String(error) }));
  return true;
});
