import { sendPanel, type Inbound, type PanelFields } from '../protocol';
import { parseSettingsSnapshot, settingsCommand, type SearchEngine, type SettingsSnapshot } from './settings-logic';

function emit(fields: ReturnType<typeof settingsCommand>): boolean {
  return fields ? sendPanel('set', fields as PanelFields) : false;
}

function fieldLabel(text: string, control: HTMLElement): HTMLLabelElement {
  const label = document.createElement('label');
  const caption = document.createElement('span');
  caption.textContent = text;
  label.append(caption, control);
  return label;
}

function input(value: string, type: 'text' | 'number' = 'text'): HTMLInputElement {
  const result = document.createElement('input');
  result.type = type;
  result.value = value;
  result.autocomplete = 'off';
  return result;
}

function checkbox(checked: boolean): HTMLInputElement {
  const result = document.createElement('input');
  result.type = 'checkbox';
  result.checked = checked;
  return result;
}

function button(label: string, action: () => void): HTMLButtonElement {
  const result = document.createElement('button');
  result.type = 'button';
  result.textContent = label;
  result.addEventListener('click', action);
  return result;
}

const heading = (tag: 'h1' | 'h2', text: string): HTMLElement => {
  const result = document.createElement(tag);
  result.textContent = text;
  return result;
};

export function startSettingsPanel(root: HTMLElement): (message: Inbound) => void {
  let snapshot: SettingsSnapshot | null = null;
  let issue = '';
  let resetConfirm = false;
  const host = document.createElement('main');
  host.className = 'customize-panel settings-panel';
  const status = document.createElement('p');
  status.setAttribute('role', 'status');
  status.setAttribute('aria-live', 'polite');
  root.replaceChildren(host);

  const render = (): void => {
    host.replaceChildren(heading('h1', 'Settings'));
    if (!snapshot) {
      status.textContent = issue || 'Loading settings…';
      host.append(status);
      return;
    }

    const form = document.createElement('form');
    form.addEventListener('submit', (event) => {
      event.preventDefault();
      const candidate: SettingsSnapshot = {
        tabLayout: layout.value as SettingsSnapshot['tabLayout'],
        sidebarWidth: Number(sidebarWidth.value),
        sleepEnabled: sleep.checked,
        sleepAfterSecs: Number(sleepAfter.value),
        blockerEnabled: blocker.checked,
        blockerAllow: allow.value.split(',').map((site) => site.trim()).filter(Boolean),
        searchEngine: engine.value === '' ? null : engine.value as SearchEngine,
        searchFallback: engine.value === '' ? snapshot?.searchFallback ?? 'default' : 'default',
        automaticTabGrouping: grouping.checked,
      };
      const command = settingsCommand('save', candidate);
      if (!command) {
        status.textContent = 'Check the values. Sidebar width must be 180–360, sleep delay 30–604800 seconds, and the allow list can contain at most 500 sites without spaces.';
        return;
      }
      status.textContent = emit(command) ? 'Save request sent.' : 'Settings could not be sent.';
    });

    const search = document.createElement('section');
    search.append(heading('h2', 'Search'));
    const engine = document.createElement('select');
    engine.setAttribute('aria-label', 'Search engine');
    const fallback = document.createElement('option');
    fallback.value = '';
    fallback.textContent = snapshot.searchFallback === 'custom' ? 'Custom (search-provider.txt)' : 'Optimised (default)';
    if (snapshot.searchEngine !== null) fallback.disabled = true;
    engine.append(fallback);
    for (const [value, label] of [['optimised', 'Optimised'], ['google', 'Google'], ['duckduckgo', 'DuckDuckGo']] as const) {
      const option = document.createElement('option');
      option.value = value;
      option.textContent = label;
      option.selected = snapshot.searchEngine === value;
      engine.append(option);
    }
    search.append(fieldLabel('Search engine', engine));
    const searchHelp = document.createElement('p');
    searchHelp.textContent = 'Optimised uses DuckDuckGo for web searches and Google for images. Search applies immediately.';
    search.append(searchHelp);

    const tabLayout = document.createElement('section');
    tabLayout.append(heading('h2', 'Tab layout'));
    const layout = document.createElement('select');
    layout.setAttribute('aria-label', 'Tab layout');
    for (const [value, label] of [['horizontal', 'Horizontal tabs'], ['vertical', 'Vertical tabs']] as const) {
      const option = document.createElement('option'); option.value = value; option.textContent = label; option.selected = snapshot.tabLayout === value; layout.append(option);
    }
    const sidebarWidth = input(String(snapshot.sidebarWidth), 'number');
    sidebarWidth.min = '180'; sidebarWidth.max = '360'; sidebarWidth.step = '1';
    tabLayout.append(fieldLabel('Tab layout', layout), fieldLabel('Sidebar width (180–360)', sidebarWidth));
    const layoutHelp = document.createElement('p'); layoutHelp.textContent = 'Sidebar width is measured in logical pixels.'; tabLayout.append(layoutHelp);

    const sleepSection = document.createElement('section');
    sleepSection.append(heading('h2', 'Tab sleep'));
    const sleep = checkbox(snapshot.sleepEnabled);
    const sleepAfter = input(String(snapshot.sleepAfterSecs), 'number'); sleepAfter.min = '30'; sleepAfter.max = '604800'; sleepAfter.step = '1';
    sleepSection.append(fieldLabel('Sleep hidden tabs after a delay', sleep), fieldLabel('Sleep after (seconds, 30–604800)', sleepAfter));
    const sleepHelp = document.createElement('p'); sleepHelp.textContent = 'Hidden tabs are discarded after this delay. Under memory pressure, the longest-hidden tabs may be discarded sooner.'; sleepSection.append(sleepHelp);

    const blockerSection = document.createElement('section');
    blockerSection.append(heading('h2', 'Ad and tracker blocker'));
    const blocker = checkbox(snapshot.blockerEnabled);
    const allow = input(snapshot.blockerAllow.join(', '));
    allow.maxLength = 2048;
    blockerSection.append(fieldLabel('Block ads and trackers', blocker), fieldLabel('Allowed sites (comma separated)', allow));
    const blockerHelp = document.createElement('p'); blockerHelp.textContent = 'The blocker skips sites listed here.'; blockerSection.append(blockerHelp);

    const groupingSection = document.createElement('section');
    groupingSection.append(heading('h2', 'Tab groups'));
    const grouping = checkbox(snapshot.automaticTabGrouping);
    groupingSection.append(fieldLabel('Group tabs from the same site automatically', grouping));
    const groupingHelp = document.createElement('p'); groupingHelp.textContent = 'Changes apply after a restart. Groups made by hand stay.'; groupingSection.append(groupingHelp);

    status.textContent = issue || 'Search applies immediately. Other changes need a restart.';
    const actions = document.createElement('div');
    const save = document.createElement('button'); save.type = 'submit'; save.textContent = 'Save changes';
    const reset = button(resetConfirm ? 'Confirm reset to defaults' : 'Reset to defaults', () => {
      if (!resetConfirm) { resetConfirm = true; reset.textContent = 'Confirm reset to defaults'; status.textContent = 'Reset saves default settings. Choose Confirm reset to defaults to continue.'; return; }
      resetConfirm = false;
      status.textContent = emit(settingsCommand('reset')) ? 'Reset request sent.' : 'Settings could not be sent.';
    });
    reset.addEventListener('blur', () => { resetConfirm = false; reset.textContent = 'Reset to defaults'; });
    actions.append(save, reset);
    form.append(search, tabLayout, sleepSection, blockerSection, groupingSection, status, actions);
    host.append(form);
  };

  return (message) => {
    if (message.t !== 'snap' || message.p !== 'settings') return;
    const incoming = parseSettingsSnapshot(message.d);
    if (!incoming) { snapshot = null; issue = 'Settings could not be loaded. The response was invalid.'; render(); return; }
    snapshot = incoming;
    issue = '';
    resetConfirm = false;
    render();
  };
}
