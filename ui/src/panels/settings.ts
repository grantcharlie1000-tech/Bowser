import { sendPanel, type Inbound, type PanelFields } from '../protocol';
import { el, icon, type IconName } from '../dom';
import { parseSettingsSnapshot, settingsCommand, type EfficiencyMode, type SearchEngine, type SettingsSnapshot, type TrackingPrevention } from './settings-logic';
import { browserProbes, codecDetail, drmText, guidance, runMediaCheck, verdictText, type MediaReport } from './media-logic';
import { enter, moveFocus, pageSection, panelHeader, segmented, settingRow, textButton, toggleSwitch } from './kit';

function emit(fields: ReturnType<typeof settingsCommand>): boolean {
  return fields ? sendPanel('set', fields as PanelFields) : false;
}

function numberField(label: string, value: number, min: number, max: number): HTMLInputElement {
  const input = el('input', 'bw-input pg-number');
  input.type = 'number';
  input.value = String(value);
  input.min = String(min);
  input.max = String(max);
  input.step = '1';
  input.autocomplete = 'off';
  input.setAttribute('aria-label', label);
  return input;
}

type SectionKey = 'search' | 'tabs' | 'privacy' | 'media';
const SECTIONS: ReadonlyArray<readonly [SectionKey, string, IconName]> = [
  ['search', 'Search', 'search'],
  ['tabs', 'Tabs', 'sidebar'],
  ['privacy', 'Blocking', 'shield'],
  ['media', 'Media support', 'volume'],
];

export function startSettingsPanel(root: HTMLElement): (message: Inbound) => void {
  let snapshot: SettingsSnapshot | null = null;
  let draft: SettingsSnapshot | null = null;
  let issue = '';
  let note = '';
  let resetConfirm = false;
  let current: SectionKey = 'search';
  let mediaRun = 0;
  let mediaStarted = false;
  let mediaRunning = false;

  const host = el('div', 'pn pg-settings');
  const nav = el('nav', 'pg-nav');
  nav.setAttribute('aria-label', 'Settings sections');
  const navTitle = el('h1', 'pg-nav-title');
  navTitle.textContent = 'Settings';
  const navList = el('div', 'pg-nav-list');
  nav.append(navTitle, navList);
  const header = panelHeader('Search', 'Close settings', 'h2');
  const main = el('main', 'pg-main');
  const scroll = el('form', 'pg-scroll bw-scroll');
  scroll.noValidate = true;
  const column = el('div', 'pg-col');
  scroll.append(column);
  const footer = el('footer', 'pg-footer');
  const message = el('p', 'pg-note');
  message.setAttribute('role', 'status');
  message.setAttribute('aria-live', 'polite');
  const reset = textButton('Reset to defaults', () => {
    if (!resetConfirm) { resetConfirm = true; paintFooter(); note = 'Choose Confirm reset to restore every setting to its default.'; paintFooter(); return; }
    resetConfirm = false;
    note = emit(settingsCommand('reset')) ? 'Reset request sent.' : 'Settings could not be sent.';
    paintFooter();
  });
  reset.addEventListener('blur', () => { if (resetConfirm) { resetConfirm = false; note = ''; paintFooter(); } });
  const save = el('button', 'bw-button is-primary');
  save.type = 'submit';
  save.textContent = 'Save changes';
  footer.append(message, reset, save);
  main.append(header.root, scroll, footer);
  host.append(nav, main);
  enter(host);
  root.replaceChildren(host);
  scroll.setAttribute('aria-labelledby', header.title.id = 'pg-section-title');

  const navButtons = new Map<SectionKey, HTMLButtonElement>();
  for (const [key, label, glyph] of SECTIONS) {
    const button = el('button', 'pg-nav-item bw-focus-inset');
    button.type = 'button';
    button.append(icon(glyph), document.createTextNode(label));
    button.addEventListener('click', () => show(key));
    navButtons.set(key, button);
    navList.append(button);
  }
  navList.addEventListener('keydown', (event) => { moveFocus(navList, '.pg-nav-item', event); });

  // The media pane is built once and survives snapshot rebuilds; a run id drops stale results.
  const media = el('div', 'pg-media');
  const mediaStatus = el('p', 'pg-note');
  mediaStatus.setAttribute('role', 'status');
  mediaStatus.setAttribute('aria-live', 'polite');
  const mediaResults = el('div', 'pg-media-results');
  const paintMedia = (report: MediaReport | null): void => {
    mediaResults.replaceChildren();
    if (!report) return;
    const decoding = pageSection('Video decoding');
    for (const codec of report.codecs) {
      const row = settingRow(codec.label, codecDetail(codec), verdictText(codec.verdict));
      row.querySelector('.pg-value')?.classList.add('pg-chip', `is-${codec.verdict}`);
      decoding.body.append(row);
    }
    const protectedContent = pageSection('Protected streaming');
    for (const system of report.drm) {
      const row = settingRow(system.label, null, drmText(system.state));
      row.querySelector('.pg-value')?.classList.add('pg-chip', `is-${system.state}`);
      protectedContent.body.append(row);
    }
    const advice = pageSection('Suggestions');
    const lines = guidance(report);
    const list = el('ul', 'pg-hints');
    for (const line of lines.length > 0 ? lines : ['Nothing needs attention.']) {
      const item = el('li');
      item.textContent = line;
      list.append(item);
    }
    advice.body.append(list);
    mediaResults.append(decoding.root, protectedContent.root, advice.root);
  };
  const checkMedia = (): void => {
    if (mediaRunning) return;
    mediaStarted = true;
    mediaRunning = true;
    const run = ++mediaRun;
    recheck.disabled = true;
    mediaStatus.textContent = 'Checking...';
    paintMedia(null);
    void runMediaCheck(browserProbes()).then((report) => {
      if (run !== mediaRun) return;
      mediaRunning = false;
      recheck.disabled = false;
      paintMedia(report);
      mediaStatus.textContent = 'Check complete.';
    }, () => {
      if (run !== mediaRun) return;
      mediaRunning = false;
      recheck.disabled = false;
      mediaStatus.textContent = 'The check could not run.';
    });
  };
  const recheck = textButton('Check again', checkMedia);
  const mediaIntro = el('p', 'pg-desc');
  mediaIntro.textContent = 'Shows which video formats and protected streaming systems this device can play. The check runs on this device only and sends nothing.';
  const mediaBar = el('div', 'pg-media-bar');
  mediaBar.append(mediaStatus, recheck);
  media.append(mediaIntro, mediaBar, mediaResults);
  media.classList.add('pg-pane');

  const panes = new Map<SectionKey, HTMLElement>();
  const show = (key: SectionKey): void => {
    current = key;
    for (const [name, button] of navButtons) {
      if (name === key) button.setAttribute('aria-current', 'page'); else button.removeAttribute('aria-current');
    }
    for (const [name, pane] of panes) pane.hidden = name !== key;
    header.title.textContent = SECTIONS.find(([name]) => name === key)?.[1] ?? '';
    footer.hidden = key === 'media';
    if (key === 'media' && !mediaStarted) checkMedia();
  };

  const dirty = (): boolean => {
    if (!snapshot || !draft) return false;
    return JSON.stringify(snapshot) !== JSON.stringify(draft);
  };
  function paintFooter(): void {
    message.textContent = issue || note || (snapshot ? 'Search and efficiency apply immediately. Other changes need a restart.' : 'Loading settings…');
    save.disabled = !dirty();
    save.setAttribute('aria-disabled', String(!dirty()));
    reset.replaceChildren(document.createTextNode(resetConfirm ? 'Confirm reset' : 'Reset to defaults'));
    reset.classList.toggle('is-primary', resetConfirm);
    reset.disabled = !snapshot;
  }

  const touch = (): void => { note = ''; paintFooter(); };

  const build = (): void => {
    panes.clear();
    column.replaceChildren();
    const mountMedia = (): void => { panes.set('media', media); column.append(media); show(current); };
    if (!snapshot || !draft) { mountMedia(); paintFooter(); return; }
    const d = draft;

    const search = pageSection('Search engine');
    const engines = [['optimised', 'Optimised'], ['google', 'Google'], ['duckduckgo', 'DuckDuckGo']] as const;
    search.body.append(
      settingRow('Default search engine', snapshot.searchFallback === 'custom' && d.searchEngine === null ? 'Using the custom provider from search-provider.txt.' : 'Optimised uses DuckDuckGo for web searches and Google for images. Applies immediately.',
        segmented(engines, d.searchEngine, (value) => { d.searchEngine = value as SearchEngine; d.searchFallback = 'default'; touch(); }, 'Default search engine')),
    );

    const tabs = el('div');
    const layout = pageSection('Layout');
    const width = numberField('Sidebar width in pixels', d.sidebarWidth, 180, 360);
    width.addEventListener('input', () => { d.sidebarWidth = Number(width.value); touch(); });
    layout.body.append(
      settingRow('Tab layout', 'Switch between a tab strip and a vertical sidebar.', segmented([['horizontal', 'Horizontal'], ['vertical', 'Vertical']] as const, d.tabLayout, (value) => { d.tabLayout = value; touch(); }, 'Tab layout')),
      settingRow('Sidebar width', 'Between 180 and 360 pixels.', width),
    );
    const sleep = pageSection('Tab sleep');
    const delay = numberField('Seconds before hidden tabs sleep', d.sleepAfterSecs, 30, 604800);
    delay.addEventListener('input', () => { d.sleepAfterSecs = Number(delay.value); touch(); });
    sleep.body.append(
      settingRow('Sleep hidden tabs', 'Hidden tabs are discarded after a delay, sooner under memory pressure.', toggleSwitch(d.sleepEnabled, (on) => { d.sleepEnabled = on; touch(); })),
      settingRow('Sleep after', 'Seconds, from 30 to 604800.', delay),
    );
    const groups = pageSection('Tab groups');
    groups.body.append(
      settingRow('Group tabs by site', 'Applies after a restart. Groups you make by hand stay.', toggleSwitch(d.automaticTabGrouping, (on) => { d.automaticTabGrouping = on; touch(); })),
    );
    const efficiency = pageSection('Efficiency');
    const modes = [['auto', 'Auto'], ['on', 'On'], ['off', 'Off']] as const;
    efficiency.body.append(
      settingRow('Efficiency mode', `Auto saves power on battery or in Battery Saver: hidden tabs sleep sooner and background tabs use less memory. Applies immediately. ${d.efficiencyActive ? 'Active now.' : 'Not active now.'}`,
        segmented(modes, d.efficiencyMode, (value) => { d.efficiencyMode = value as EfficiencyMode; touch(); }, 'Efficiency mode')),
    );
    tabs.append(layout.root, sleep.root, efficiency.root, groups.root);

    const blocking = pageSection('Ad and tracker blocker');
    const allow = el('input', 'bw-input pg-wide');
    allow.type = 'text';
    allow.value = d.blockerAllow.join(', ');
    allow.maxLength = 2048;
    allow.autocomplete = 'off';
    allow.spellcheck = false;
    allow.placeholder = 'example.com, news.example';
    allow.setAttribute('aria-label', 'Sites the blocker skips');
    allow.addEventListener('input', () => { d.blockerAllow = allow.value.split(',').map((site) => site.trim()).filter(Boolean); touch(); });
    blocking.body.append(
      settingRow('Block ads and trackers', 'Stops known trackers before they load.', toggleSwitch(d.blockerEnabled, (on) => { d.blockerEnabled = on; touch(); })),
      settingRow('Allowed sites', 'Comma separated. The blocker skips these.', allow),
    );

    const tracking = pageSection('Tracking prevention');
    const levels = [['balanced', 'Balanced'], ['strict', 'Strict']] as const;
    tracking.body.append(
      settingRow('Tracking prevention level', 'Balanced is the default. Strict blocks more trackers but can break some sites. Takes effect after a restart.',
        segmented(levels, d.trackingPrevention, (value) => { d.trackingPrevention = value as TrackingPrevention; touch(); }, 'Tracking prevention level')),
    );
    const privacy = el('div');
    privacy.append(blocking.root, tracking.root);

    const sections: Record<Exclude<SectionKey, 'media'>, HTMLElement> = { search: search.root, tabs, privacy };
    for (const [key] of SECTIONS) {
      if (key === 'media') continue;
      const pane = sections[key];
      pane.classList.add('pg-pane');
      panes.set(key, pane);
      column.append(pane);
    }
    mountMedia();
    paintFooter();
  };

  scroll.addEventListener('submit', (event) => {
    event.preventDefault();
    if (!draft || !dirty()) return;
    const command = settingsCommand('save', draft);
    if (!command) {
      note = 'Check the values. Sidebar width must be 180 to 360, sleep delay 30 to 604800 seconds, and the allow list can hold at most 500 sites without spaces.';
      paintFooter();
      return;
    }
    note = emit(command) ? 'Save request sent.' : 'Settings could not be sent.';
    paintFooter();
  });

  show(current);
  paintFooter();
  return (incoming) => {
    if (incoming.t !== 'snap' || incoming.p !== 'settings') return;
    const parsed = parseSettingsSnapshot(incoming.d);
    if (!parsed) { snapshot = null; draft = null; issue = 'Settings could not be loaded. The response was invalid.'; build(); return; }
    snapshot = parsed;
    draft = { ...parsed, blockerAllow: [...parsed.blockerAllow] };
    issue = '';
    note = '';
    resetConfirm = false;
    build();
  };
}
