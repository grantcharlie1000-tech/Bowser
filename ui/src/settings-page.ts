// The settings page (?view=settings), drawn by Bowser over a `bowser://settings` tab
// (docs/design/settings-page.md, styles/settings.css). Rust sends `state` (theme), `snap`
// (p "settings") and `settingsgo`; the page sends `{t:"set",a,...}` commands only. Every value it
// sends is checked again in Rust (chrome_host/settings.rs). Text from Rust reaches the DOM through
// textContent only. The page never changes its own URL (the bridge checks it exactly), so sections
// are buttons and scroll positions, not links or fragments.
import { onInbound, send, sendPanel, type Inbound, type PanelFields, type SettingsSection } from './protocol';
import { applyTheme } from './view';
import { el, icon, type IconName } from './dom';
import { installTooltips } from './tooltip';
import { selectField, type SelectField } from './panels/select';
import { textButton, type PageSection } from './panels/kit';
import { mountPrivacyDashboard } from './panels/privacy';
import { mountExtensionsSection } from './extensions-section';
import { browserProbes, codecDetail, drmText, guidance, runMediaCheck, verdictText, type MediaReport } from './panels/media-logic';
import {
  BOOKMARKS_BAR_MODES, DEFAULT_TOOLBAR_BUTTONS, EFFICIENCY_MODES, FONT_SIZES, LAYOUT_HINTS, MAX_SIDEBAR, MAX_UI_FONT, MAX_ZOOM, MIN_SIDEBAR, MIN_ZOOM,
  NOT_YET_GROUP_NOTE, NOT_YET_NOTE, RESTART_NOTE, ROWS, SECTIONS, SIDE_PANEL_POSITIONS, TAB_LAYOUTS, TAB_LAYOUT_LABELS, THEME_MODES, TOOLBAR_BUTTONS,
  addSite, draftOf, durationLabel, highlightRanges, isSection, matchRows, parseSettingsSnapshot, resultCountText,
  saveText, sleepOptions, toolbarButtonsList, updateView, type BookmarksBarMode, type Draft, type EfficiencyMode, type FontSize, type RowSpec,
  type SearchEngine, type SettingsSnapshot, type SidePanelPosition, type TabLayout, type ThemeMode, type ToolbarButton, type TrackingPrevention,
} from './settings-page-logic';

const SVG_NS = 'http://www.w3.org/2000/svg';
const NAV_ICONS: Readonly<Record<SettingsSection, IconName>> = {
  profile: 'person', appearance: 'sidebar', privacy: 'shield', performance: 'workspace-bolt',
  search: 'search', downloads: 'folder-download', media: 'volume', extensions: 'puzzle', updates: 'download', about: 'info',
};
const reduceMotion = (): boolean => matchMedia('(prefers-reduced-motion: reduce)').matches;

function cmd(fields: PanelFields): boolean {
  return sendPanel('set', fields);
}

// ---- Small controls with setters (a snapshot updates them in place, so focus never jumps) ----

interface Control<T> { readonly root: HTMLElement; set(value: T): void; disable(off: boolean): void }

function switchControl(onChange: (on: boolean) => void): Control<boolean> {
  const button = el('button', 'bw-toggle sp-switch');
  button.type = 'button';
  button.setAttribute('role', 'switch');
  button.setAttribute('aria-checked', 'false');
  button.addEventListener('click', () => {
    const next = button.getAttribute('aria-checked') !== 'true';
    button.setAttribute('aria-checked', String(next));
    onChange(next);
  });
  return {
    root: button,
    set: (on) => button.setAttribute('aria-checked', String(on)),
    disable: (off) => { button.disabled = off; button.setAttribute('aria-disabled', String(off)); },
  };
}

/** Radio group with roving tabindex and arrow keys; `items` build each option's face. */
function radioGroup<T extends string>(className: string, label: string, items: ReadonlyArray<readonly [T, (button: HTMLButtonElement) => void]>, onSelect: (value: T) => void): Control<T | null> {
  const group = el('div', className);
  group.setAttribute('role', 'radiogroup');
  group.setAttribute('aria-label', label);
  const buttons: Array<[T, HTMLButtonElement]> = [];
  const paint = (value: T | null): void => {
    buttons.forEach(([key, button], index) => {
      const on = key === value;
      button.setAttribute('aria-checked', String(on));
      button.tabIndex = on || (value === null && index === 0) ? 0 : -1;
    });
  };
  for (const [value, face] of items) {
    const button = el('button');
    button.type = 'button';
    button.setAttribute('role', 'radio');
    face(button);
    button.addEventListener('click', () => {
      if (button.getAttribute('aria-checked') === 'true') return;
      paint(value);
      onSelect(value);
    });
    buttons.push([value, button]);
    group.append(button);
  }
  group.addEventListener('keydown', (event) => {
    const step = event.key === 'ArrowLeft' || event.key === 'ArrowUp' ? -1 : event.key === 'ArrowRight' || event.key === 'ArrowDown' ? 1 : 0;
    if (step === 0) return;
    event.preventDefault();
    const index = buttons.findIndex(([, button]) => button === document.activeElement);
    const next = buttons[(index + step + buttons.length) % buttons.length]?.[1];
    next?.focus();
    next?.click();
  });
  paint(null);
  return {
    root: group,
    set: paint,
    disable: (off) => buttons.forEach(([, button]) => { button.disabled = off; }),
  };
}

function segmentedControl<T extends string>(label: string, options: ReadonlyArray<readonly [T, string]>, onSelect: (value: T) => void): Control<T | null> {
  return radioGroup('bw-segmented sp-seg', label, options.map(([value, text]) => [value, (button: HTMLButtonElement) => { button.textContent = text; }] as const), onSelect);
}

/** A schematic of a browser layout: window frame, toolbar, tab row or sidebar, page. */
function layoutArt(layout: TabLayout): SVGSVGElement {
  const svg = document.createElementNS(SVG_NS, 'svg');
  svg.setAttribute('viewBox', '0 0 72 46');
  svg.setAttribute('class', 'sp-art');
  svg.setAttribute('aria-hidden', 'true');
  const rect = (cls: string, x: number, y: number, w: number, h: number, r = 1.5): void => {
    const node = document.createElementNS(SVG_NS, 'rect');
    for (const [k, v] of [['x', x], ['y', y], ['width', w], ['height', h], ['rx', r]] as const) node.setAttribute(k, String(v));
    node.setAttribute('class', cls);
    svg.append(node);
  };
  rect('sp-art-frame', 1, 1, 70, 44, 5);
  const tabs = (x: number, y: number, n: number, w: number): void => {
    for (let i = 0; i < n; i++) rect(i === 0 ? 'sp-art-tab is-on' : 'sp-art-tab', x + i * (w + 2), y, w, 4.5, 1.5);
  };
  const toolbar = (y: number): void => {
    rect('sp-art-dot', 5, y + 1.25, 3, 3, 1.5);
    rect('sp-art-dot', 10, y + 1.25, 3, 3, 1.5);
    rect('sp-art-pill', 16, y, 40, 5.5, 2.75);
  };
  switch (layout) {
    case 'horizontal':
      toolbar(5); tabs(5, 13.5, 3, 14); rect('sp-art-page', 5, 21, 62, 20, 2.5);
      break;
    case 'tabs-top':
      tabs(5, 5, 3, 14); toolbar(12.5); rect('sp-art-page', 5, 21, 62, 20, 2.5);
      break;
    case 'single-row':
      rect('sp-art-dot', 5, 6.25, 3, 3, 1.5); rect('sp-art-pill', 10, 5, 22, 5.5, 2.75); tabs(35, 5.5, 3, 9.3); rect('sp-art-page', 5, 14, 62, 27, 2.5);
      break;
    case 'vertical':
      toolbar(5);
      for (let i = 0; i < 4; i++) rect(i === 0 ? 'sp-art-tab is-on' : 'sp-art-tab', 5, 14 + i * 6.5, 15, 4.5, 1.5);
      rect('sp-art-page', 23, 14, 44, 27, 2.5);
      break;
  }
  return svg;
}

// ---- The page ----

interface RowNode {
  readonly spec: RowSpec; readonly root: HTMLElement; readonly label: HTMLElement; readonly desc: HTMLElement | null; readonly card: HTMLElement;
  /** "Applies after restart" or "no effect yet", when the row has one. */
  readonly flag: HTMLElement | null;
  /** The control the label and description are wired to. */
  readonly target: HTMLElement | null;
}
/** A group: its heading (outside the card) and the card that holds its rows. */
interface Group { readonly root: HTMLElement; readonly heading: HTMLElement | null; readonly card: HTMLElement }

/** "Applies after restart" or "no effect yet" under a row's description, or `null`. */
function rowFlag(spec: RowSpec): HTMLElement | null {
  if (!spec.notYet && !spec.restart) return null;
  const flag = el('span', `sp-flag${spec.notYet ? ' is-later' : ''}`);
  flag.id = `sp-f-${spec.id}`;
  const text = el('span');
  text.textContent = spec.notYet ? NOT_YET_NOTE : RESTART_NOTE;
  flag.append(icon(spec.notYet ? 'clock' : 'reload', { size: 12, fine: true }), text);
  return flag;
}

export function startSettingsPage(root: HTMLElement): void {
  root.classList.add('sp-root');
  installTooltips(root);
  let snapshot: SettingsSnapshot | null = null;
  let draft: Draft | null = null;
  /** Changes sent and not yet answered: while any are out, a snapshot does not reset the controls. */
  let pending = 0;
  let entered = false;
  let current: SettingsSection = 'profile';
  let query = '';

  const page = el('div', 'sp');
  const side = el('aside', 'sp-side');
  const brand = el('div', 'sp-brand');
  const mark = el('span', 'sp-mark');
  mark.append(icon('gear', { size: 22 }));
  const title = el('h1', 'sp-title');
  title.textContent = 'Settings';
  brand.append(mark, title);

  const search = el('div', 'sp-search');
  search.setAttribute('role', 'search');
  const input = el('input', 'sp-search-input');
  input.type = 'search';
  input.placeholder = 'Search settings';
  input.autocomplete = 'off';
  input.spellcheck = false;
  input.maxLength = 64;
  input.setAttribute('aria-label', 'Search settings');
  input.setAttribute('aria-controls', 'sp-main');
  input.setAttribute('aria-describedby', 'sp-count');
  const clear = el('button', 'bw-icon-btn sp-search-clear');
  clear.type = 'button';
  clear.hidden = true;
  clear.setAttribute('aria-label', 'Clear search');
  clear.append(icon('close', { size: 12, fine: true }));
  search.append(icon('search', { className: 'sp-search-icon' }), input, clear);

  const nav = el('nav', 'sp-nav');
  nav.setAttribute('aria-label', 'Settings sections');
  const indicator = el('span', 'sp-ind');
  indicator.setAttribute('aria-hidden', 'true');
  const navList = el('ul', 'sp-nav-list');
  nav.append(indicator, navList);
  side.append(brand, search, nav);

  const main = el('main', 'sp-main bw-scroll');
  main.id = 'sp-main';
  main.tabIndex = -1;
  main.setAttribute('aria-label', 'Settings');
  const column = el('div', 'sp-col');
  const banner = el('p', 'sp-banner');
  banner.setAttribute('role', 'alert');
  banner.hidden = true;
  const empty = el('div', 'sp-empty');
  empty.hidden = true;
  const emptyText = el('p');
  empty.append(icon('search', { size: 24 }), emptyText);
  column.append(banner);
  main.append(column);

  const count = el('p', 'sp-sr');
  count.id = 'sp-count';
  count.setAttribute('role', 'status');
  count.setAttribute('aria-live', 'polite');
  const toast = el('div', 'sp-toast');
  toast.setAttribute('role', 'status');
  toast.setAttribute('aria-live', 'polite');
  const toastIcon = el('span', 'sp-toast-icon');
  const toastText = el('span');
  toast.append(toastIcon, toastText);
  page.append(side, main, count, toast);
  root.replaceChildren(page);

  // ---- toast: "Saved", or what went wrong ----
  let toastTimer = 0;
  const showToast = (text: string, ok: boolean): void => {
    toastIcon.replaceChildren(icon(ok ? 'check' : 'warning', { size: 14 }));
    toastText.textContent = text;
    toast.classList.toggle('is-error', !ok);
    toast.classList.remove('is-on');
    void toast.offsetWidth; // restart the entrance when a second save lands during the first toast
    toast.classList.add('is-on');
    window.clearTimeout(toastTimer);
    toastTimer = window.setTimeout(() => toast.classList.remove('is-on'), ok ? 1600 : 4200);
  };

  // ---- sending ----
  const save = (): void => {
    if (!draft || !snapshot?.readable) return;
    const text = saveText(draft);
    if (text === null) { showToast('That value is not allowed.', false); return; }
    if (cmd({ a: 'save', text })) pending += 1;
  };
  const change = (edit: (d: Draft) => void): void => {
    if (!draft) return;
    edit(draft);
    save();
  };

  // ---- sections, cards and rows ----
  const sectionNodes = new Map<SettingsSection, HTMLElement>();
  const navButtons = new Map<SettingsSection, HTMLButtonElement>();
  const rows = new Map<string, RowNode>();
  const groups: Group[] = [];
  let cardSeq = 0;

  for (const spec of SECTIONS) {
    const section = el('section', 'sp-sec');
    section.dataset.section = spec.key;
    const heading = el('h2', 'sp-h');
    heading.id = `sp-h-${spec.key}`;
    heading.textContent = spec.title;
    section.setAttribute('aria-labelledby', heading.id);
    const lead = el('p', 'sp-lead');
    lead.textContent = spec.lead;
    section.append(heading, lead);
    sectionNodes.set(spec.key, section);
    column.append(section);

    const item = el('li');
    const button = el('button', 'sp-nav-item');
    button.type = 'button';
    button.dataset.section = spec.key;
    button.append(icon(NAV_ICONS[spec.key]), document.createTextNode(spec.nav));
    button.addEventListener('click', () => go(spec.key, true, true));
    item.append(button);
    navList.append(item);
    navButtons.set(spec.key, button);
  }
  column.append(empty);
  navList.addEventListener('keydown', (event) => {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp' && event.key !== 'ArrowLeft' && event.key !== 'ArrowRight' && event.key !== 'Home' && event.key !== 'End') return;
    const items = [...navButtons.values()];
    const index = items.findIndex((button) => button === document.activeElement);
    if (index < 0) return;
    event.preventDefault();
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : (index + (event.key === 'ArrowDown' || event.key === 'ArrowRight' ? 1 : -1) + items.length) % items.length;
    items[next]?.focus();
  });

  /** A group in `section`: an optional heading above a card of rows. Returns the card. */
  const card = (section: SettingsSection, heading?: string): HTMLElement => {
    const group = el('div', 'sp-group');
    const node = el('div', 'sp-card');
    node.setAttribute('role', 'group');
    let h: HTMLElement | null = null;
    if (heading) {
      h = el('h3', 'sp-group-h');
      h.id = `sp-card-${++cardSeq}`;
      h.textContent = heading;
      node.setAttribute('aria-labelledby', h.id);
      group.append(h);
    }
    group.append(node);
    sectionNodes.get(section)?.append(group);
    groups.push({ root: group, heading: h, card: node });
    return node;
  };

  /** A row: label, description and (optionally) a control on the right, inside `into`. */
  const row = (into: HTMLElement, id: string, control?: HTMLElement, options: { stack?: boolean; label?: string } = {}): RowNode => {
    const spec = ROWS.find((r) => r.id === id);
    if (!spec) throw new Error(`no row ${id}`);
    const node = el('div', `sp-row${options.stack ? ' is-stack' : ''}`);
    node.dataset.row = id;
    const text = el('div', 'sp-text');
    const label = el('span', 'sp-label');
    label.id = `sp-l-${id}`;
    label.textContent = options.label ?? spec.label;
    text.append(label);
    let desc: HTMLElement | null = null;
    if (spec.description !== '') {
      desc = el('span', 'sp-desc');
      desc.id = `sp-d-${id}`;
      desc.textContent = spec.description;
      text.append(desc);
    }
    const flag = rowFlag(spec);
    if (flag) text.append(flag);
    node.append(text);
    let target: HTMLElement | null = null;
    if (control) {
      const wrap = el('div', 'sp-ctl');
      wrap.append(control);
      target = control.matches('button, input, [role="radiogroup"]') ? control : control.querySelector<HTMLElement>('button, input, [role="radiogroup"]');
      if (target && !target.hasAttribute('aria-label')) target.setAttribute('aria-labelledby', label.id);
      const described = [desc?.id, flag?.id].filter(Boolean).join(' ');
      if (target && described !== '') target.setAttribute('aria-describedby', described);
      node.append(wrap);
    }
    into.append(node);
    const result = { spec, root: node, label, desc, card: into, flag, target };
    rows.set(id, result);
    return result;
  };

  // You and Bowser
  const profileCard = card('profile');
  const avatar = el('span', 'sp-avatar');
  avatar.setAttribute('aria-hidden', 'true');
  const profileRow = row(profileCard, 'profile', avatar);
  profileRow.root.classList.add('is-profile');
  profileRow.root.prepend(profileRow.root.lastElementChild ?? avatar);
  const manage = textButton('Manage profiles', () => cmd({ a: 'profiles' }), { icon: 'person' });
  row(profileCard, 'profiles', manage);
  row(profileCard, 'import', textButton('Import', () => cmd({ a: 'import' }), { icon: 'download' }));

  // Appearance: the two big visual choices first, then the chrome, tabs and text.
  const layoutCard = card('appearance', 'Layout');
  const layout = radioGroup<TabLayout>('sp-tiles', 'Browser layout', TAB_LAYOUTS.map((key) => [key, (button: HTMLButtonElement) => {
    button.className = 'sp-tile';
    const name = el('span', 'sp-tile-name');
    name.textContent = TAB_LAYOUT_LABELS[key];
    const hint = el('span', 'sp-tile-hint');
    hint.textContent = LAYOUT_HINTS[key];
    button.append(layoutArt(key), name, hint);
  }] as const), (value) => change((d) => { d.tabLayout = value; }));
  row(layoutCard, 'layout', layout.root, { stack: true });
  const width = el('input', 'sp-range');
  width.type = 'range';
  width.min = String(MIN_SIDEBAR);
  width.max = String(MAX_SIDEBAR);
  width.step = '10';
  const widthValue = el('output', 'sp-range-value');
  const widthWrap = el('div', 'sp-range-wrap');
  widthWrap.append(width, widthValue);
  const paintWidth = (): void => {
    widthValue.textContent = `${width.value} px`;
    width.style.setProperty('--sp-fill', `${((Number(width.value) - MIN_SIDEBAR) / (MAX_SIDEBAR - MIN_SIDEBAR)) * 100}%`);
    width.setAttribute('aria-valuetext', `${width.value} pixels`);
  };
  width.addEventListener('input', paintWidth);
  width.addEventListener('change', () => change((d) => { d.sidebarWidth = Number(width.value); }));
  row(layoutCard, 'sidebarWidth', widthWrap);

  // Theme
  const themeCard = card('appearance', 'Theme');
  const theme = segmentedControl<ThemeMode>('Theme', THEME_MODES.map((key) => [key, key === 'helium' ? 'Helium colours' : 'System'] as const), (value) => change((d) => { d.theme = value; }));
  row(themeCard, 'theme', theme.root);
  const accentInput = el('input', 'sp-color');
  accentInput.type = 'color';
  accentInput.setAttribute('aria-label', 'Accent colour');
  accentInput.addEventListener('change', () => change((d) => { d.themeAccent = accentInput.value.toLowerCase(); }));
  const accentReset = textButton('Reset to default', () => change((d) => { d.theme = 'helium'; d.themeAccent = ''; }));
  const accentWrap = el('div', 'sp-reset');
  accentWrap.append(accentInput, accentReset);
  row(themeCard, 'themeAccent', accentWrap);

  // Toolbar
  const TOOLBAR_LABELS: Readonly<Record<ToolbarButton, string>> = {
    back: 'Back', forward: 'Forward', reload: 'Reload', 'site-info': 'Site information', bookmark: 'Bookmark', newtab: 'New tab', split: 'Split view', reader: 'Reader view',
  };
  const TOOLBAR_ICONS: Readonly<Record<ToolbarButton, IconName>> = {
    back: 'back', forward: 'forward', reload: 'reload', 'site-info': 'info', bookmark: 'bookmark', newtab: 'plus', split: 'split', reader: 'reader',
  };
  const toolbarCard = card('appearance', 'Toolbar');
  const toolbarSwitches = new Map<ToolbarButton, Control<boolean>>();
  const toolbarList = el('div', 'sp-toolbar-list');
  for (const id of TOOLBAR_BUTTONS) {
    const item = el('div', 'sp-toolbar-item');
    const label = el('span', 'sp-label');
    label.textContent = TOOLBAR_LABELS[id];
    const glyph = icon(TOOLBAR_ICONS[id], { className: 'sp-toolbar-glyph' });
    const sw = switchControl((on) => change((d) => {
      const ids = new Set(toolbarButtonsList(d.toolbarButtons));
      if (on) ids.add(id); else ids.delete(id);
      d.toolbarButtons = TOOLBAR_BUTTONS.filter((b) => ids.has(b)).join(',');
    }));
    sw.root.setAttribute('aria-label', TOOLBAR_LABELS[id]);
    toolbarSwitches.set(id, sw);
    item.append(glyph, label, sw.root);
    toolbarList.append(item);
  }
  const toolbarReset = textButton('Reset to default', () => change((d) => { d.toolbarButtons = DEFAULT_TOOLBAR_BUTTONS; }));
  const toolbarActions = el('div', 'sp-corner');
  toolbarActions.append(toolbarReset);
  const toolbarRow = row(toolbarCard, 'toolbarButtons', toolbarList, { stack: true });
  toolbarRow.root.append(toolbarActions);
  const showHome = switchControl((on) => change((d) => { d.showHomeButton = on; }));
  row(toolbarCard, 'showHomeButton', showHome.root);

  // Address bar
  const addressCard = card('appearance', 'Address bar');
  const centred = switchControl((on) => change((d) => { d.centredAddressBar = on; }));
  row(addressCard, 'centredAddressBar', centred.root);
  const minimal = switchControl((on) => change((d) => { d.minimalUrl = on; }));
  row(addressCard, 'minimalUrl', minimal.root);
  const loadProgress = switchControl((on) => change((d) => { d.showLoadProgress = on; }));
  row(addressCard, 'showLoadProgress', loadProgress.root);

  // Tabs (the rows Bowser does not act on yet sit last)
  const tabsCard = card('appearance', 'Tabs');
  const grouping = switchControl((on) => change((d) => { d.automaticTabGrouping = on; }));
  row(tabsCard, 'grouping', grouping.root);
  const besideActive = switchControl((on) => change((d) => { d.newTabBesideActive = on; }));
  row(tabsCard, 'newTabBesideActive', besideActive.root);
  const mru = switchControl((on) => change((d) => { d.ctrlTabMru = on; }));
  row(tabsCard, 'ctrlTabMru', mru.root);
  const edgeDrop = switchControl((on) => change((d) => { d.splitEdgeDrop = on; }));
  row(tabsCard, 'splitEdgeDrop', edgeDrop.root);
  const previewImages = switchControl((on) => change((d) => { d.tabPreviewImages = on; }));
  row(tabsCard, 'tabPreviewImages', previewImages.root);
  const previewMemory = switchControl((on) => change((d) => { d.tabPreviewMemory = on; }));
  row(tabsCard, 'tabPreviewMemory', previewMemory.root);
  const pinGroups = switchControl((on) => change((d) => { d.pinNewTabGroups = on; }));
  row(tabsCard, 'pinNewTabGroups', pinGroups.root);
  const vTabs = switchControl((on) => change((d) => { d.ctrlSVerticalTabs = on; }));
  row(tabsCard, 'ctrlSVerticalTabs', vTabs.root);

  // Window
  const windowCard = card('appearance', 'Window');
  const rounded = switchControl((on) => change((d) => { d.roundedContentFrame = on; }));
  row(windowCard, 'roundedContentFrame', rounded.root);
  const frameless = switchControl((on) => change((d) => { d.frameless = on; }));
  row(windowCard, 'frameless', frameless.root);

  // Bookmarks bar
  const bookmarksCard = card('appearance', 'Bookmarks bar');
  const bookmarksBarCtl = segmentedControl<BookmarksBarMode>('Bookmarks bar', BOOKMARKS_BAR_MODES.map((key) => [key, key === 'always' ? 'Always show' : key === 'never' ? 'Always hide' : 'Only on the new tab'] as const), (value) => change((d) => { d.bookmarksBar = value; }));
  row(bookmarksCard, 'bookmarksBar', bookmarksBarCtl.root);
  const bookmarksGroups = switchControl((on) => change((d) => { d.bookmarksBarTabGroups = on; }));
  row(bookmarksCard, 'bookmarksBarTabGroups', bookmarksGroups.root);

  // Helium panels
  const panelsCard = card('appearance', 'Helium panels');
  const sidePanelCtl = segmentedControl<SidePanelPosition>('Side panel position', SIDE_PANEL_POSITIONS.map((key) => [key, key === 'left' ? 'Left' : 'Right'] as const), (value) => change((d) => { d.sidePanelPosition = value; }));
  row(panelsCard, 'sidePanelPosition', sidePanelCtl.root);

  // Fonts
  const fontsCard = card('appearance', 'Fonts');
  const fontSizeCtl = segmentedControl<FontSize>('Font size', FONT_SIZES.map((key) => [key, key === 'small' ? 'Small' : key === 'medium' ? 'Medium (recommended)' : 'Large'] as const), (value) => change((d) => { d.fontSize = value; }));
  row(fontsCard, 'fontSize', fontSizeCtl.root);
  const uiFont = el('input', 'bw-input sp-ui-font-input');
  uiFont.type = 'text';
  uiFont.placeholder = 'System font';
  uiFont.maxLength = MAX_UI_FONT;
  uiFont.autocomplete = 'off';
  uiFont.spellcheck = false;
  uiFont.setAttribute('aria-label', 'Customise fonts');
  uiFont.addEventListener('change', () => change((d) => { d.uiFont = uiFont.value.trim(); }));
  row(fontsCard, 'uiFont', uiFont);

  // Zoom
  const zoomCard = card('appearance', 'Zoom');
  const zoomInput = el('input', 'bw-input sp-zoom-input');
  zoomInput.type = 'number';
  zoomInput.min = String(MIN_ZOOM);
  zoomInput.max = String(MAX_ZOOM);
  zoomInput.step = '5';
  zoomInput.setAttribute('aria-label', 'Default zoom');
  zoomInput.addEventListener('change', () => {
    const n = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, Math.round(Number(zoomInput.value)) || 100));
    zoomInput.value = String(n);
    change((d) => { d.defaultZoom = n; });
  });
  const zoomWrap = el('div', 'sp-zoom-wrap');
  const zoomSuffix = el('span', 'sp-value');
  zoomSuffix.textContent = '%';
  zoomWrap.append(zoomInput, zoomSuffix);
  row(zoomCard, 'defaultZoom', zoomWrap);
  const zoomIndicator = switchControl((on) => change((d) => { d.showZoomIndicator = on; }));
  row(zoomCard, 'showZoomIndicator', zoomIndicator.root);

  // Shortcuts
  const shortcutsCard = card('appearance', 'Shortcuts');
  const shiftRightClick = switchControl((on) => change((d) => { d.shiftRightClickContext = on; }));
  row(shortcutsCard, 'shiftRightClickContext', shiftRightClick.root);
  const copyLink = switchControl((on) => change((d) => { d.ctrlShiftCCopyLink = on; }));
  row(shortcutsCard, 'ctrlShiftCCopyLink', copyLink.root);

  // Privacy and security
  const blockCard = card('privacy', 'Blocking');
  const blocker = switchControl((on) => change((d) => { d.blockerEnabled = on; }));
  row(blockCard, 'blocker', blocker.root);
  const blockYoutube = switchControl((on) => change((d) => { d.blockYoutubeVideoAds = on; }));
  row(blockCard, 'blockYoutubeVideoAds', blockYoutube.root);
  const allowList = el('ul', 'sp-sites');
  allowList.setAttribute('aria-label', 'Sites the blocker skips');
  const allowForm = el('div', 'sp-allow-add');
  const allowInput = el('input', 'bw-input sp-allow-input');
  allowInput.type = 'text';
  allowInput.placeholder = 'example.com';
  allowInput.autocomplete = 'off';
  allowInput.spellcheck = false;
  allowInput.maxLength = 253;
  allowInput.setAttribute('aria-label', 'Add a site the blocker skips');
  const allowError = el('p', 'sp-field-error');
  allowError.setAttribute('role', 'alert');
  const addAllowed = (): void => {
    if (!draft) return;
    const result = addSite(draft.blockerAllow, allowInput.value);
    if ('error' in result) { allowError.textContent = result.error; return; }
    allowError.textContent = '';
    allowInput.value = '';
    change((d) => { d.blockerAllow = result.list; });
    paintSites();
  };
  const addButton = textButton('Add', addAllowed, { icon: 'plus' });
  allowInput.addEventListener('keydown', (event) => { if (event.key === 'Enter') { event.preventDefault(); addAllowed(); } });
  allowInput.addEventListener('input', () => { allowError.textContent = ''; });
  allowForm.append(allowInput, addButton);
  const paintSites = (): void => {
    const sites = draft?.blockerAllow ?? [];
    allowList.replaceChildren(...sites.map((site) => {
      const item = el('li', 'sp-site');
      const name = el('span', 'sp-site-name');
      name.textContent = site;
      name.dir = 'auto';
      const remove = el('button', 'bw-icon-btn sp-site-remove');
      remove.type = 'button';
      remove.setAttribute('aria-label', `Remove ${site}`);
      remove.setAttribute('data-tip', 'Remove');
      remove.append(icon('close', { size: 12, fine: true }));
      remove.addEventListener('click', () => {
        const next = (draft?.blockerAllow ?? []).filter((s) => s !== site);
        change((d) => { d.blockerAllow = next; });
        paintSites();
        allowInput.focus();
      });
      item.append(name, remove);
      return item;
    }));
    allowList.hidden = sites.length === 0;
  };
  // The field and Add sit beside the label; the skipped sites and any error wrap beneath them.
  const allowRow = row(blockCard, 'allow', allowForm);
  allowRow.root.classList.add('is-wrap', 'is-tight');
  allowRow.root.append(allowList, allowError);
  const trackCard = card('privacy', 'Tracking and history');
  const tracking = segmentedControl<TrackingPrevention>('Tracking prevention', [['balanced', 'Balanced'], ['strict', 'Strict']], (value) => change((d) => { d.trackingPrevention = value; }));
  row(trackCard, 'tracking', tracking.root);
  const history = switchControl((on) => { if (cmd({ a: 'history', on })) pending += 1; });
  const historyRow = row(trackCard, 'history', history.root);

  // The Privacy dashboard lives here, not in a window of its own: Rust sends its `privacy` snapshots
  // to this page and takes its `priv` commands. Search treats the whole block as the `dashboard` row.
  const dashHost = el('div', 'sp-dash');
  dashHost.dataset.row = 'dashboard';
  const dashStatus = el('p', 'sp-note sp-dash-status');
  dashStatus.setAttribute('role', 'status');
  dashStatus.setAttribute('aria-live', 'polite');
  const dashColumn = el('div', 'sp-dash-col');
  dashHost.append(dashStatus, dashColumn);
  sectionNodes.get('privacy')?.append(dashHost);
  const dashSection = (title: string): PageSection => {
    const root = el('div', 'sp-group');
    const heading = el('h3', 'sp-group-h');
    heading.textContent = title;
    const box = el('div', 'sp-card');
    const body = el('div', 'pg-rows');
    box.append(body);
    root.append(heading, box);
    return { root, body };
  };
  const dashboardSpec = ROWS.find((r) => r.id === 'dashboard')!;
  rows.set('dashboard', { spec: dashboardSpec, root: dashHost, label: el('span'), desc: null, card: dashHost, flag: null, target: null });
  const receiveDashboard = mountPrivacyDashboard({ column: dashColumn, status: dashStatus, scroll: main, section: dashSection });

  // Performance
  const powerCard = card('performance', 'Power');
  const efficiency = segmentedControl<EfficiencyMode>('Efficiency mode', EFFICIENCY_MODES.map((mode) => [mode, mode === 'auto' ? 'Auto' : mode === 'on' ? 'On' : 'Off'] as const), (value) => change((d) => { d.efficiencyMode = value; }));
  const efficiencyRow = row(powerCard, 'efficiency', efficiency.root);
  const activeBadge = el('span', 'sp-badge');
  activeBadge.textContent = 'Active now';
  const labelLine = el('span', 'sp-label-line');
  efficiencyRow.label.replaceWith(labelLine);
  labelLine.append(efficiencyRow.label, activeBadge);
  const memoryCard = card('performance', 'Memory');
  const sleep = switchControl((on) => change((d) => { d.sleepEnabled = on; }));
  row(memoryCard, 'sleep', sleep.root);
  const sleepHost = el('div', 'sp-select-host');
  let sleepSelect: SelectField<string> | null = null;
  let sleepChoices = '';
  const paintSleepSelect = (value: number): void => {
    const options = sleepOptions(value);
    const key = options.join(',');
    if (!sleepSelect || key !== sleepChoices) {
      sleepChoices = key;
      sleepSelect?.close();
      sleepSelect = selectField<string>({
        options: options.map((secs) => ({ value: String(secs), label: durationLabel(secs) })),
        value: String(value),
        label: 'Sleep after',
        placeholder: 'Sleep after',
        onChange: (next) => change((d) => { d.sleepAfterSecs = Number(next); }),
      });
      const rowNode = rows.get('sleepAfter');
      sleepSelect.button.setAttribute('aria-labelledby', 'sp-l-sleepAfter');
      if (rowNode?.desc) sleepSelect.button.setAttribute('aria-describedby', rowNode.desc.id);
      sleepHost.replaceChildren(sleepSelect.root);
    } else {
      sleepSelect.set(String(value));
    }
  };
  row(memoryCard, 'sleepAfter', sleepHost);

  // Search engine
  const searchCard = card('search');
  const engines: ReadonlyArray<readonly [SearchEngine, string, string]> = [
    ['optimised', 'Optimised', 'DuckDuckGo for web searches, Google for images. The default.'],
    ['google', 'Google', 'Google for web and image searches.'],
    ['duckduckgo', 'DuckDuckGo', 'DuckDuckGo for web and image searches.'],
  ];
  const engine = radioGroup<SearchEngine>('sp-engines', 'Search engine', engines.map(([key, name, about]) => [key, (button: HTMLButtonElement) => {
    button.className = 'sp-engine';
    const dot = el('span', 'sp-radio');
    dot.setAttribute('aria-hidden', 'true');
    const text = el('span', 'sp-engine-text');
    const n = el('span', 'sp-engine-name');
    n.textContent = name;
    const d = el('span', 'sp-engine-desc');
    d.textContent = about;
    text.append(n, d);
    button.append(dot, text);
  }] as const), (value) => change((d) => { d.searchEngine = value; }));
  const engineRow = row(searchCard, 'engine', engine.root, { stack: true });
  const customNote = el('p', 'sp-note');
  customNote.textContent = 'Searches use the custom provider in search-provider.txt until you choose an engine here.';
  engineRow.root.append(customNote);

  // Downloads (docs/design/downloads.md "Settings"): both controls are their own commands, never
  // part of `save`; Rust shows the native folder picker and answers with the next snapshot.
  const downloadsCard = card('downloads');
  // Not counted as pending: a cancelled picker answers with nothing, and the path is read straight
  // from each snapshot, never from the draft.
  const downloadChange = textButton('Change', () => { cmd({ a: 'downloadfolder' }); }, { icon: 'folder' });
  downloadChange.setAttribute('aria-label', 'Change download location');
  const downloadDirRow = row(downloadsCard, 'downloadDir', downloadChange);
  const downloadPath = el('span', 'sp-desc sp-path');
  downloadPath.id = 'sp-d-downloadDir';
  downloadPath.dir = 'ltr';
  downloadDirRow.label.after(downloadPath);
  downloadChange.setAttribute('aria-describedby', downloadPath.id);
  const downloadAsk = switchControl((on) => { if (cmd({ a: 'downloadask', on })) pending += 1; });
  row(downloadsCard, 'downloadAsk', downloadAsk.root);

  // Media
  const mediaCard = card('media');
  const mediaStatus = el('span', 'sp-media-status');
  const recheck = textButton('Check again', () => checkMedia(), { icon: 'reload' });
  const mediaHead = el('div', 'sp-media-head');
  mediaHead.append(mediaStatus, recheck);
  row(mediaCard, 'media', mediaHead);
  const mediaResults = el('div', 'sp-media');
  mediaCard.append(mediaResults);
  let mediaRun = 0;
  let mediaStarted = false;
  const paintMedia = (report: MediaReport | null): void => {
    mediaResults.replaceChildren();
    if (!report) return;
    const group = (heading: string, items: ReadonlyArray<readonly [string, string, string, string]>): HTMLElement => {
      const box = el('div', 'sp-media-group');
      const h = el('h3', 'sp-card-h');
      h.textContent = heading;
      const list = el('ul', 'sp-media-list');
      for (const [label, detail, value, tone] of items) {
        const item = el('li', 'sp-media-item');
        const text = el('span', 'sp-text');
        const name = el('span', 'sp-label');
        name.textContent = label;
        text.append(name);
        if (detail) { const d = el('span', 'sp-desc'); d.textContent = detail; text.append(d); }
        const chip = el('span', `sp-pill is-${tone}`);
        chip.textContent = value;
        item.append(text, chip);
        list.append(item);
      }
      box.append(h, list);
      return box;
    };
    const advice = guidance(report);
    const hints = el('ul', 'sp-hints');
    for (const line of advice.length > 0 ? advice : ['Nothing needs attention.']) { const li = el('li'); li.textContent = line; hints.append(li); }
    mediaResults.append(
      group('Video decoding', report.codecs.map((codec) => [codec.label, codecDetail(codec), verdictText(codec.verdict), codec.verdict] as const)),
      group('Protected streaming', report.drm.map((system) => [system.label, '', drmText(system.state), system.state] as const)),
      hints,
    );
  };
  const checkMedia = (): void => {
    mediaStarted = true;
    const run = ++mediaRun;
    recheck.disabled = true;
    mediaStatus.textContent = 'Checking…';
    void runMediaCheck(browserProbes()).then((report) => {
      if (run !== mediaRun) return;
      recheck.disabled = false;
      mediaStatus.textContent = 'Check complete';
      paintMedia(report);
    }, () => {
      if (run !== mediaRun) return;
      recheck.disabled = false;
      mediaStatus.textContent = 'The check could not run';
    });
  };
  mediaStatus.textContent = 'Runs when you open this section';
  // The check runs once, the first time the Media card comes into view.
  new IntersectionObserver((entries, observer) => {
    if (entries.some((entry) => entry.isIntersecting) && snapshot) {
      observer.disconnect();
      if (!mediaStarted) checkMedia();
    }
  }, { root: main }).observe(mediaCard);

  // Extensions: a block of its own, like the Privacy dashboard. Rust sends its `extensions` snapshots to
  // this page and takes its extension commands in the `set` family; search treats it as one row.
  const extensions = mountExtensionsSection({ send: cmd, toast: (text, ok) => showToast(text, ok) });
  sectionNodes.get('extensions')?.append(extensions.root);
  const extensionsSpec = ROWS.find((r) => r.id === 'extensions')!;
  rows.set('extensions', { spec: extensionsSpec, root: extensions.root, label: el('span'), desc: null, card: extensions.root, flag: null, target: null });

  // Updates
  const updateCard = card('updates');
  const autoUpdate = switchControl((on) => { if (cmd({ a: 'autoupdate', on })) pending += 1; });
  row(updateCard, 'autoUpdate', autoUpdate.root);
  const statusBox = el('div', 'sp-update');
  const statusIcon = el('span', 'sp-update-icon');
  const statusText = el('div', 'sp-text');
  const statusTitle = el('span', 'sp-label');
  const statusDetail = el('span', 'sp-desc');
  statusText.append(statusTitle, statusDetail);
  const updateAction = textButton('Check for updates', () => {
    const view = snapshot ? updateView(snapshot) : null;
    if (view?.action === 'restart') cmd({ a: 'restartupdate' });
    else if (view?.action === 'check' && cmd({ a: 'checkupdates' })) { updateAction.disabled = true; statusTitle.textContent = 'Checking for updates…'; }
  });
  statusBox.append(statusIcon, statusText, updateAction);
  const updateRow = row(updateCard, 'updateStatus');
  updateRow.root.replaceChildren(statusBox);
  updateRow.root.classList.add('is-status');

  // About
  const aboutCard = card('about');
  const version = el('span', 'sp-value');
  row(aboutCard, 'version', version);
  row(aboutCard, 'file', textButton('Open settings file', () => cmd({ a: 'openfile' }), { icon: 'external' }));
  const resetBox = el('div', 'sp-reset');
  let confirming = false;
  const reset = textButton('Reset settings', () => {
    if (!confirming) { confirming = true; paintReset(); cancel.focus(); return; }
    confirming = false;
    paintReset();
    if (cmd({ a: 'reset' })) pending += 1;
  }, { icon: 'reset' });
  const cancel = textButton('Cancel', () => { confirming = false; paintReset(); reset.focus(); });
  const paintReset = (): void => {
    reset.lastChild!.textContent = confirming ? 'Reset all settings' : 'Reset settings';
    reset.classList.toggle('is-danger', confirming);
    cancel.hidden = !confirming;
  };
  resetBox.append(cancel, reset);
  paintReset();
  row(aboutCard, 'reset', resetBox);
  const defaultBrowserCard = card('about');
  const defaultBrowserButton = textButton('Make Bowser the default browser', () => cmd({ a: 'setdefault' }));
  const defaultBrowserRow = row(defaultBrowserCard, 'defaultBrowser', defaultBrowserButton);
  defaultBrowserRow.root.classList.add('is-tight');
  const defaultBrowserStatus = defaultBrowserRow.desc!;

  // A group whose every row has no effect yet says so once, under its heading, not on each row.
  for (const group of groups) {
    const inGroup = [...rows.values()].filter((node) => node.card === group.card);
    if (inGroup.length < 2 || !inGroup.every((node) => node.spec.notYet) || !group.heading) continue;
    const note = el('p', 'sp-flag is-later sp-group-note');
    note.id = `${group.heading.id}-note`;
    const text = el('span');
    text.textContent = NOT_YET_GROUP_NOTE;
    note.append(icon('clock', { size: 12, fine: true }), text);
    group.heading.after(note);
    group.card.setAttribute('aria-describedby', note.id);
    for (const node of inGroup) {
      node.flag?.remove();
      if (node.desc) node.target?.setAttribute('aria-describedby', node.desc.id); else node.target?.removeAttribute('aria-describedby');
    }
  }

  // ---- applying a snapshot ----
  const apply = (s: SettingsSnapshot): void => {
    const settle = pending === 0;
    if (settle || !draft) draft = draftOf(s);
    const d = draft;
    layout.set(d.tabLayout);
    width.value = String(d.sidebarWidth);
    paintWidth();
    grouping.set(d.automaticTabGrouping);
    previewImages.set(d.tabPreviewImages);
    previewMemory.set(d.tabPreviewMemory);
    besideActive.set(d.newTabBesideActive);
    mru.set(d.ctrlTabMru);
    pinGroups.set(d.pinNewTabGroups);
    edgeDrop.set(d.splitEdgeDrop);
    vTabs.set(d.ctrlSVerticalTabs);
    theme.set(d.theme);
    accentInput.value = d.themeAccent !== '' ? d.themeAccent : '#000000';
    for (const [id, sw] of toolbarSwitches) sw.set(toolbarButtonsList(d.toolbarButtons).includes(id));
    showHome.set(d.showHomeButton);
    centred.set(d.centredAddressBar);
    minimal.set(d.minimalUrl);
    loadProgress.set(d.showLoadProgress);
    frameless.set(d.frameless);
    rounded.set(d.roundedContentFrame);
    bookmarksBarCtl.set(d.bookmarksBar);
    bookmarksGroups.set(d.bookmarksBarTabGroups);
    sidePanelCtl.set(d.sidePanelPosition);
    fontSizeCtl.set(d.fontSize);
    uiFont.value = d.uiFont;
    zoomInput.value = String(d.defaultZoom);
    zoomIndicator.set(d.showZoomIndicator);
    shiftRightClick.set(d.shiftRightClickContext);
    copyLink.set(d.ctrlShiftCCopyLink);
    blockYoutube.set(d.blockYoutubeVideoAds);
    defaultBrowserStatus.textContent = s.defaultBrowser === 'yes' ? 'Bowser is your default browser.'
      : s.defaultBrowser === 'no' ? 'Bowser is not your default browser.' : 'Bowser could not check whether it is the default browser.';
    blocker.set(d.blockerEnabled);
    paintSites();
    tracking.set(d.trackingPrevention);
    history.set(s.historyEnabled);
    history.disable(s.guest);
    historyRow.desc!.textContent = s.guest ? 'Guest windows never save history.' : ROWS.find((r) => r.id === 'history')!.description;
    efficiency.set(d.efficiencyMode);
    activeBadge.hidden = !s.efficiencyActive;
    sleep.set(d.sleepEnabled);
    paintSleepSelect(d.sleepAfterSecs);
    sleepSelect?.button.toggleAttribute('disabled', !d.sleepEnabled);
    engine.set(d.searchEngine ?? (s.searchFallback === 'custom' ? null : 'optimised'));
    customNote.hidden = !(d.searchEngine === null && s.searchFallback === 'custom');
    downloadPath.textContent = s.downloadDir !== '' ? s.downloadDir : 'Your Downloads folder';
    downloadAsk.set(s.downloadAsk);
    autoUpdate.set(s.autoUpdate);
    autoUpdate.disable(s.update === 'unavailable');
    const view = updateView(s);
    statusBox.dataset.tone = view.tone;
    statusIcon.replaceChildren(view.busy ? el('span', 'bw-spinner') : icon(view.tone === 'warn' ? 'warning' : view.action === 'restart' ? 'download' : 'check', { size: 20 }));
    statusTitle.textContent = view.title;
    statusDetail.textContent = view.detail;
    updateAction.hidden = view.action === null;
    updateAction.disabled = view.busy;
    updateAction.lastChild!.textContent = view.action === 'restart' ? 'Restart to update' : 'Check for updates';
    updateAction.classList.toggle('is-primary', view.action === 'restart');
    version.textContent = `Bowser ${s.version}`;
    avatar.textContent = s.avatar;
    profileRow.label.textContent = s.guest ? 'Guest' : s.profile === '' ? 'Default' : s.profile;
    profileRow.desc!.textContent = s.guest ? 'A guest window keeps nothing after it closes. Changes here last until then.' : ROWS.find((r) => r.id === 'profile')!.description;
    manage.disabled = false;
    banner.hidden = s.readable;
    banner.textContent = s.readable ? '' : 'settings.txt could not be read, so the defaults are shown and changes are not saved. Open the settings file to fix it.';
    for (const control of [
      layout, grouping, blocker, tracking, efficiency, sleep, engine,
      previewImages, previewMemory, besideActive, mru, pinGroups, edgeDrop, vTabs, theme, showHome, centred, minimal, loadProgress, frameless,
      rounded, bookmarksBarCtl, bookmarksGroups, sidePanelCtl, fontSizeCtl, zoomIndicator, shiftRightClick, copyLink, blockYoutube,
      ...toolbarSwitches.values(),
    ]) control.disable(!s.readable);
    downloadAsk.disable(!s.readable);
    downloadChange.disabled = !s.readable;
    width.disabled = !s.readable;
    allowInput.disabled = !s.readable;
    addButton.disabled = !s.readable;
    accentInput.disabled = !s.readable;
    accentReset.disabled = !s.readable;
    toolbarReset.disabled = !s.readable;
    uiFont.disabled = !s.readable;
    zoomInput.disabled = !s.readable;
    if (query !== '') filter();
  };

  // ---- navigation: scroll position, active section, indicator ----
  const paintNav = (): void => {
    let index = 0;
    SECTIONS.forEach((spec, i) => {
      const on = spec.key === current;
      const button = navButtons.get(spec.key);
      if (on) { button?.setAttribute('aria-current', 'true'); index = i; } else button?.removeAttribute('aria-current');
    });
    const button = navButtons.get(current);
    if (button) indicator.style.setProperty('--sp-ind-y', `${button.offsetTop}px`);
    indicator.style.setProperty('--sp-ind-h', `${button?.offsetHeight ?? 32}px`);
    indicator.dataset.index = String(index);
    // Narrow layout: keep the current chip in view.
    if (page.classList.contains('is-narrow')) button?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  };

  const scrollToSection = (key: SettingsSection | '', smooth: boolean): void => {
    const target = key === '' ? null : sectionNodes.get(key);
    const top = target ? target.offsetTop - 8 : 0;
    main.scrollTo({ top, behavior: smooth && !reduceMotion() ? 'smooth' : 'auto' });
  };

  /** Shows `key`; `tell` reports it to Bowser (the tab's address follows), `smooth` animates. */
  const go = (key: SettingsSection | '', smooth: boolean, tell: boolean): void => {
    if (query !== '') { input.value = ''; query = ''; filter(); }
    current = key === '' ? 'profile' : key;
    // The chosen section stays current even when the page cannot scroll it to the top (the last
    // ones), until the user scrolls by hand.
    pinned = true;
    paintNav();
    scrollToSection(key, smooth);
    if (tell) cmd({ a: 'section', text: key });
    if (key === 'media' && !mediaStarted) checkMedia();
    if (tell && key !== '') sectionNodes.get(key)?.querySelector<HTMLElement>('h2')?.focus({ preventScroll: true });
  };

  // The section whose heading last passed a line a third of the way down is the current one.
  let scrollFrame = 0;
  let pinned = false;
  for (const type of ['wheel', 'pointerdown', 'keydown', 'touchstart'] as const) main.addEventListener(type, () => { pinned = false; }, { passive: true });
  main.addEventListener('scroll', () => {
    if (scrollFrame || pinned) return;
    scrollFrame = requestAnimationFrame(() => {
      scrollFrame = 0;
      const line = main.scrollTop + main.clientHeight / 3;
      let next: SettingsSection = 'profile';
      for (const spec of SECTIONS) {
        const node = sectionNodes.get(spec.key);
        if (node && !node.hidden && node.offsetTop <= line) next = spec.key;
      }
      if (main.scrollTop + main.clientHeight >= main.scrollHeight - 2) {
        const visible = SECTIONS.filter((spec) => !sectionNodes.get(spec.key)?.hidden);
        next = visible[visible.length - 1]?.key ?? next;
      }
      if (next !== current) { current = next; paintNav(); }
      if (current === 'media' && !mediaStarted) checkMedia();
    });
  }, { passive: true });
  for (const node of sectionNodes.values()) node.querySelector<HTMLElement>('h2')!.tabIndex = -1;

  // ---- search ----
  const marked = (target: HTMLElement, text: string): void => {
    const ranges = query === '' ? [] : highlightRanges(text, query);
    if (ranges.length === 0) { if (target.textContent !== text || target.childElementCount > 0) target.textContent = text; return; }
    const parts: Node[] = [];
    let at = 0;
    for (const [start, end] of ranges) {
      if (start > at) parts.push(document.createTextNode(text.slice(at, start)));
      const m = el('mark', 'sp-mark-hit');
      m.textContent = text.slice(start, end);
      parts.push(m);
      at = end;
    }
    if (at < text.length) parts.push(document.createTextNode(text.slice(at)));
    target.replaceChildren(...parts);
  };
  function filter(): void {
    const hits = matchRows(query);
    page.classList.toggle('is-searching', hits !== null);
    for (const node of rows.values()) {
      const show = hits === null || hits.has(node.spec.id);
      node.root.hidden = !show;
      const labelText = node.label.textContent ?? '';
      marked(node.label, labelText);
      if (node.desc) marked(node.desc, node.desc.textContent ?? '');
    }
    for (const group of groups) group.root.hidden = hits !== null && !group.card.querySelector('.sp-row:not([hidden])');
    mediaResults.hidden = hits !== null && !hits.has('media');
    dashHost.hidden = hits !== null && !hits.has('dashboard');
    for (const spec of SECTIONS) {
      const section = sectionNodes.get(spec.key)!;
      const any = hits === null || [...rows.values()].some((node) => node.spec.section === spec.key && hits.has(node.spec.id));
      section.hidden = !any;
      navButtons.get(spec.key)?.classList.toggle('is-empty', !any);
    }
    const total = hits?.size ?? 0;
    empty.hidden = hits === null || total > 0;
    page.classList.toggle('is-nomatch', hits !== null && total === 0);
    emptyText.textContent = hits === null ? '' : resultCountText(0, query);
    count.textContent = hits === null ? '' : resultCountText(total, query);
    clear.hidden = query === '';
    main.scrollTop = 0;
    if (hits !== null) {
      const first = SECTIONS.find((spec) => !sectionNodes.get(spec.key)?.hidden);
      if (first && first.key !== current) { current = first.key; paintNav(); }
    }
  }
  input.addEventListener('input', () => { query = input.value; filter(); });
  input.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && input.value !== '') { event.preventDefault(); input.value = ''; query = ''; filter(); return; }
    if (event.key !== 'Enter') return;
    event.preventDefault();
    const first = [...rows.values()].find((node) => !node.root.hidden);
    if (!first) return;
    const target = first.root.querySelector<HTMLElement>('button:not([disabled]), input:not([disabled])');
    first.card.classList.remove('is-flash');
    void first.card.offsetWidth;
    first.card.classList.add('is-flash');
    first.root.scrollIntoView({ block: 'center', behavior: reduceMotion() ? 'auto' : 'smooth' });
    target?.focus({ preventScroll: true });
    cmd({ a: 'section', text: first.spec.section });
  });
  clear.addEventListener('click', () => { input.value = ''; query = ''; filter(); input.focus(); });
  // Ctrl+F and / jump to the search field, as on Chrome's settings page.
  document.addEventListener('keydown', (event) => {
    const typing = event.target instanceof HTMLInputElement;
    if ((event.key === 'f' && event.ctrlKey && !event.shiftKey && !event.altKey) || (event.key === '/' && !typing)) {
      event.preventDefault();
      input.focus();
      input.select();
    }
  });

  // ---- responsive: one column below 880px ----
  const fit = (): void => {
    const narrow = window.innerWidth < 880;
    if (page.classList.contains('is-narrow') !== narrow) page.classList.toggle('is-narrow', narrow);
    paintNav();
  };
  window.addEventListener('resize', fit);
  fit();

  // A fresh snapshot when the page comes back to the front (another window may have changed things).
  let lastRefresh = 0;
  window.addEventListener('focus', () => {
    const now = Date.now();
    if (snapshot && now - lastRefresh > 2000) { lastRefresh = now; cmd({ a: 'refresh' }); }
  });

  // ---- Bowser's messages ----
  let wanted: SettingsSection | '' | null = null;
  const receive = (message: Inbound): void => {
    if (message.t === 'state') { applyTheme(message.theme, message.solid, message.dark); return; }
    if (message.t === 'settingsgo') {
      // Bowser moved the tab (a typed address, a menu pick): jump, as a navigation would.
      if (!snapshot) { wanted = message.s; return; }
      go(message.s, false, false);
      return;
    }
    if (message.t === 'snap' && message.p === 'privacy') { receiveDashboard(message); return; }
    if (message.t === 'snap' && message.p === 'extensions') { extensions.receive(message); return; }
    if (message.t !== 'snap' || message.p !== 'settings') return;
    const parsed = parseSettingsSnapshot(message.d);
    if (!parsed) { banner.hidden = false; banner.textContent = 'Settings could not be loaded. The response was invalid.'; return; }
    if (parsed.ack !== 'none') {
      // Bowser runs queued commands together and answers once: this snapshot is the saved state.
      pending = 0;
      if (parsed.ack === 'saved') showToast('Saved', true);
      else showToast(parsed.error !== '' ? parsed.error : 'Settings could not be saved.', false);
    }
    snapshot = parsed;
    apply(parsed);
    if (!entered) {
      entered = true;
      page.classList.add('bw-enter', 'is-rise', 'is-entered');
      mark.classList.add('is-turn');
      requestAnimationFrame(paintNav);
      if (wanted !== null) { const target = wanted; wanted = null; go(target, false, false); }
    }
  };
  onInbound(receive);
  send({ v: 1, t: 'ready' });
  if (isSection(current)) paintNav();
}
