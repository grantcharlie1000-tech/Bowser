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
import { mountProtection } from './panels/protection'; // blocking, history and website appearance (thread T3)
import { mountExtensionsSection } from './extensions-section';
import { mountStoreSection } from './store-section';
import { mountCommandsSections } from './commands-section';
import { mountStartSection } from './settings-start'; // --- start page ---
import { mountThemeRows } from './settings-themes'; // --- theme options ---
import { themeAssignment, type ThemeOptionValue } from './theme-options-logic'; // --- theme options ---
import { mountAddressSections } from './address-section'; // --- address bar, search and bookmarks ---
import { WINDOW_APPEARANCE_KEYS, WINDOW_PANEL_KEYS, resetWindowGroup, stepValue, windowGroupIsDefault } from './window-logic'; // --- window and panels (thread T9) ---
// --- toolbars and feeds (thread T13) ---
import { mountToolbarEditor, type EditorCommand } from './toolbar-editor';
import { BAR_KEYS, itemsOf, itemsText, type BarId } from './bars-logic';
import { REFRESH_CHOICES, feedAssignment, refreshLabel, type RefreshChoice } from './feeds-settings-logic';
import { parseCommandsSnapshot } from './commands-logic';
// --- end toolbars and feeds ---
// --- layout (thread T6) --- the Layout block's presets, edges and values
import {
  BAR_POSITIONS, DEFAULT_LAYOUT_CONFIG, DEFAULT_UI_ZOOM, MAX_UI_ZOOM, MIN_UI_ZOOM, PRESETS, PRESET_HINTS, PRESET_LABELS, STATUS_BARS, STATUS_BAR_LABELS,
  TAB_BAR_EDGES, TAB_BAR_EDGE_LABELS, TOP_TAB_STYLES, UI_DENSITIES, UI_ZOOM_STEP, applyLayoutConfig, parseLayoutConfig, parseUiZoom, presetConfig,
  rememberOwnLayout, selectedTile, tabBarEdge, tabLayoutForEdge, uiZoomText, type BarPosition, type LayoutConfig, type Preset, type StatusBar,
  type TabBarEdge, type UiDensity,
} from './layout-logic';
// --- end layout ---
import { mountPageToolsSection } from './page-tools-section'; // --- page tools (T7) ---
import { browserProbes, codecDetail, drmText, guidance, runMediaCheck, verdictText, type MediaReport } from './panels/media-logic';
import { TAB_CARDS, TAB_CARD_TITLES, assignmentText, cardSpecs, clampNumber, numberLabel, textApplies, toggleApplies, type TabOptionSpec, type TabOptionValues } from './tab-options-logic';
import {
  BOOKMARKS_BAR_MODES, EFFICIENCY_MODES, FONT_SIZES, LAYOUT_HINTS, MAX_SIDEBAR, MAX_UI_FONT, MAX_ZOOM, MIN_SIDEBAR, MIN_ZOOM,
  NOT_YET_GROUP_NOTE, NOT_YET_NOTE, RESTART_NOTE, ROWS, SIDE_PANEL_POSITIONS, TAB_LAYOUT_LABELS, THEME_MODES,
  addSite, draftOf, durationLabel, highlightRanges, isSection, matchRows, parseSettingsSnapshot, resultCountText,
  saveText, sleepOptions, translateTargetOptions, updateView, type BookmarksBarMode, type Draft, type EfficiencyMode, type FontSize, type RowSpec,
  type SettingsSnapshot, type SidePanelPosition, type TabLayout, type ThemeMode, type TrackingPrevention,
  AREAS, LATER_LABEL, LATER_LEAD, LATER_TITLE, MORE_LABEL, PAGE_ROWS, areaOf, moreCountText, partTitle, rowTier, type AreaSpec,
  AUTO_HIDE_BARS, AUTO_HIDE_BAR_LABELS, autoHideBarsList, autoHideBarsText, type AutoHideBar, // --- auto-hide (T11) ---
  addRuleText, removeRuleText, ruleExists, ruleSite as ruleSiteOf, type WorkspaceRules, // --- tab stacks (T10) ---
} from './settings-page-logic';

/** --- settings slim --- the disclosures remember whether they are open (a per-viewer convenience only). */
const OPEN_KEY = 'bowser.settings.more';
function readOpen(): Set<string> {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(OPEN_KEY) ?? '[]');
    return new Set(Array.isArray(raw) ? raw.filter((v): v is string => typeof v === 'string') : []);
  } catch {
    return new Set();
  }
}
function writeOpen(open: ReadonlySet<string>): void {
  try { localStorage.setItem(OPEN_KEY, JSON.stringify([...open])); } catch { /* storage unavailable: the state lasts this visit */ }
}

const SVG_NS = 'http://www.w3.org/2000/svg';
/** How long a Tabs number field waits after its last change before it saves. */
const TAB_NUMBER_DEBOUNCE_MS = 400;
const NAV_ICONS: Readonly<Record<SettingsSection, IconName>> = {
  profile: 'person', appearance: 'sidebar', tabs: 'tab-switch', privacy: 'shield', performance: 'workspace-bolt',
  search: 'search', keyboard: 'command', commands: 'quick-commands', downloads: 'folder-download', media: 'volume', extensions: 'puzzle', updates: 'download', about: 'info',
  startpage: 'home', // --- start page ---
  address: 'address-bar', bookmarks: 'bookmark', // --- wave-1 integration ---
  pagetools: 'image', // --- page tools (T7) ---
  stacks: 'tab-group', // --- T10 --- (drawn in the Tabs and start page area, never in the nav on its own)
};
const reduceMotion = (): boolean => matchMedia('(prefers-reduced-motion: reduce)').matches;

function cmd(fields: PanelFields): boolean {
  return sendPanel('set', fields);
}

// ---- Small controls with setters (a snapshot updates them in place, so focus never jumps) ----

interface Control<T> { readonly root: HTMLElement; set(value: T): void; disable(off: boolean): void }

export function switchControl(onChange: (on: boolean) => void): Control<boolean> {
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

export function segmentedControl<T extends string>(label: string, options: ReadonlyArray<readonly [T, string]>, onSelect: (value: T) => void): Control<T | null> {
  return radioGroup('bw-segmented sp-seg', label, options.map(([value, text]) => [value, (button: HTMLButtonElement) => { button.textContent = text; }] as const), onSelect);
}

/**
 * A schematic of a browser layout: window frame, toolbar, tab row or sidebar, page. Takes the tab
 * layout alone (the "style at the top" tiles) or a whole configuration (the preset tiles, thread
 * T6): tabs on any edge or hidden, the address bar on top or along the bottom or hidden, and the
 * title row that holds the window buttons when neither is at the top.
 */
function layoutArt(config: TabLayout | LayoutConfig): SVGSVGElement {
  const c: LayoutConfig = typeof config === 'string' ? { ...DEFAULT_LAYOUT_CONFIG, tabLayout: config } : config;
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
  const singleRow = (y: number): void => {
    rect('sp-art-dot', 5, y + 1.25, 3, 3, 1.5); rect('sp-art-pill', 10, y, 22, 5.5, 2.75); tabs(35, y + 0.5, 3, 9.3);
  };
  // The title row: the window buttons alone, at the right end.
  const titleRow = (y: number): void => {
    for (let i = 0; i < 3; i++) rect('sp-art-dot', 56 + i * 4, y + 1.25, 2.5, 2.5, 1.25);
  };
  const edge = c.tabBarVisible ? tabBarEdge(c.tabLayout) : null;
  const addressTop = c.addressBarVisible && c.addressBarPosition === 'top';
  const addressBottom = c.addressBarVisible && c.addressBarPosition === 'bottom';
  // The rows at the top, then the rows along the bottom, each 8.5 units of the 36 inside the frame.
  type Row = 'toolbar' | 'tabs' | 'single' | 'title';
  const top: Row[] = [];
  const bottom: Row[] = [];
  if (edge === 'top') {
    if (c.tabLayout === 'single-row' && addressTop) top.push('single');
    else if (c.tabLayout === 'horizontal' && addressTop) top.push('toolbar', 'tabs');
    else top.push('tabs', ...(addressTop ? ['toolbar' as Row] : []));
  } else if (addressTop) {
    top.push(...(c.showTitleBar && (edge === 'left' || edge === 'right') ? ['title' as Row] : []), 'toolbar');
  } else {
    top.push('title');
  }
  if (edge === 'bottom') bottom.push('tabs');
  if (addressBottom) bottom.push('toolbar');
  let y = 5;
  for (const row of top) {
    if (row === 'toolbar') toolbar(y); else if (row === 'tabs') tabs(5, y + 0.5, 3, 14); else if (row === 'single') singleRow(y); else titleRow(y);
    y += 8.5;
  }
  let floor = 41;
  for (const row of bottom.reverse()) {
    floor -= 8.5;
    if (row === 'toolbar') toolbar(floor + 3); else tabs(5, floor + 3.5, 3, 14);
  }
  const pageTop = y;
  const pageBottom = floor;
  let left = 5;
  let right = 67;
  if (edge === 'left' || edge === 'right') {
    const x = edge === 'left' ? 5 : 52;
    for (let i = 0; i < 4; i++) {
      const rowY = pageTop + i * 6.5;
      if (rowY + 4.5 <= pageBottom) rect(i === 0 ? 'sp-art-tab is-on' : 'sp-art-tab', x, rowY, 15, 4.5, 1.5);
    }
    if (edge === 'left') left = 23; else right = 49;
  }
  rect('sp-art-page', left, pageTop, Math.max(right - left, 1), Math.max(pageBottom - pageTop, 1), 2.5);
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
    else showToast('Settings could not be sent. Nothing was saved.', false);
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

  // --- settings slim --- one section per area (its heading, lead and nav item), holding one part per
  // Rust section; the rows are built into the parts, then `arrange` sorts them into place.
  const areaNodes = new Map<SettingsSection, HTMLElement>();
  for (const spec of AREAS) {
    const area = el('section', 'sp-sec');
    area.dataset.area = spec.key;
    const heading = el('h2', 'sp-h');
    heading.id = `sp-h-${spec.key}`;
    heading.textContent = spec.title;
    area.setAttribute('aria-labelledby', heading.id);
    const lead = el('p', 'sp-lead');
    lead.textContent = spec.lead;
    area.append(heading, lead);
    areaNodes.set(spec.key, area);
    column.append(area);
    for (const key of spec.parts) {
      const part = el('section', 'sp-part');
      part.dataset.section = key;
      if (key === spec.key) part.setAttribute('aria-labelledby', heading.id);
      else part.setAttribute('aria-label', partTitle(key));
      area.append(part);
      sectionNodes.set(key, part);
    }

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

  /** A titled group of rows inside a card: the shape the Privacy dashboard and the protection blocks share. */
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
  // Blocking level, history retention and website appearance (thread T3). Rust sends their snapshot
  // as `protection`; their controls sit in the rows below and their blocks follow the Privacy rows.
  // The level is a saved setting too, so the page's own draft is kept in step with it.
  const protection = mountProtection({ toast: showToast, section: dashSection, onLevel: (value) => { if (draft) draft.blockerEnabled = value !== 'none'; } });

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
  // --- layout (thread T6) --- the Layout block: the presets as thumbnails and the three edges in
  // view; the top style, sidebar width, the show/hide switches, the title row, the status bar, the
  // density and the UI zoom behind More options (rowTier). Each pick saves the whole draft; a
  // configuration that is no preset is remembered as Saved (rememberOwnLayout).
  const layoutCard = card('appearance', 'Layout');
  let savedTile: HTMLButtonElement | null = null;
  const presetTile = (key: Preset | 'saved') => (button: HTMLButtonElement): void => {
    button.className = 'sp-tile';
    button.dataset.preset = key;
    const name = el('span', 'sp-tile-name');
    name.textContent = PRESET_LABELS[key];
    const hint = el('span', 'sp-tile-hint');
    hint.textContent = PRESET_HINTS[key];
    button.append(layoutArt(key === 'saved' ? DEFAULT_LAYOUT_CONFIG : presetConfig(key)), name, hint);
    if (key === 'saved') savedTile = button;
  };
  const presets = radioGroup<Preset | 'saved'>('sp-tiles is-presets', 'Layout', [...PRESETS, 'saved' as const].map((key) => [key, presetTile(key)] as const), (value) => change((d) => {
    const config = value === 'saved' ? parseLayoutConfig(d.layout.saved) : presetConfig(value);
    if (config) applyLayoutConfig(d, config);
  }));
  row(layoutCard, 'layoutPreset', presets.root, { stack: true });
  /** The Saved tile shows the saved arrangement, or stays dimmed until there is one. */
  const paintSavedTile = (saved: string, readable: boolean): void => {
    if (!savedTile) return;
    const config = parseLayoutConfig(saved);
    savedTile.querySelector('svg')?.replaceWith(layoutArt(config ?? DEFAULT_LAYOUT_CONFIG));
    savedTile.disabled = config === null || !readable;
    savedTile.setAttribute('aria-disabled', String(savedTile.disabled));
  };
  const bars = el('div', 'sp-bars');
  const barGroup = (label: string, control: Control<unknown>): void => {
    const box = el('div', 'sp-bar');
    const text = el('span', 'sp-bar-label');
    text.textContent = label;
    box.append(text, control.root);
    bars.append(box);
  };
  const tabEdge = segmentedControl<TabBarEdge>('Tab bar', TAB_BAR_EDGES.map((key) => [key, TAB_BAR_EDGE_LABELS[key]] as const), (value) => change((d) => { d.tabLayout = tabLayoutForEdge(value, d.tabLayout); rememberOwnLayout(d); }));
  const addressEdge = segmentedControl<BarPosition>('Address bar', BAR_POSITIONS.map((key) => [key, key === 'top' ? 'Top' : 'Bottom'] as const), (value) => change((d) => { d.layout.addressBarPosition = value; rememberOwnLayout(d); }));
  const panelEdge = segmentedControl<SidePanelPosition>('Panels', SIDE_PANEL_POSITIONS.map((key) => [key, key === 'left' ? 'Left' : 'Right'] as const), (value) => change((d) => { d.sidePanelPosition = value; rememberOwnLayout(d); }));
  barGroup('Tab bar', tabEdge);
  barGroup('Address bar', addressEdge);
  barGroup('Panels', panelEdge);
  row(layoutCard, 'layoutBars', bars, { stack: true });
  const layout = radioGroup<TabLayout>('sp-tiles', 'Tab bar style at the top', TOP_TAB_STYLES.map((key) => [key, (button: HTMLButtonElement) => {
    button.className = 'sp-tile';
    const name = el('span', 'sp-tile-name');
    name.textContent = TAB_LAYOUT_LABELS[key];
    const hint = el('span', 'sp-tile-hint');
    hint.textContent = LAYOUT_HINTS[key];
    button.append(layoutArt(key), name, hint);
  }] as const), (value) => change((d) => { d.tabLayout = value; rememberOwnLayout(d); }));
  row(layoutCard, 'layout', layout.root, { stack: true });
  // --- end layout --- (the rest of the block follows the sidebar width row)
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
  // --- layout (thread T6) --- the rest of the Layout block
  const tabBarVisible = switchControl((on) => change((d) => { d.layout.tabBarVisible = on; rememberOwnLayout(d); }));
  row(layoutCard, 'tabBarVisible', tabBarVisible.root);
  const addressBarVisible = switchControl((on) => change((d) => { d.layout.addressBarVisible = on; rememberOwnLayout(d); }));
  row(layoutCard, 'addressBarVisible', addressBarVisible.root);
  const titleBar = switchControl((on) => change((d) => { d.layout.showTitleBar = on; rememberOwnLayout(d); }));
  row(layoutCard, 'showTitleBar', titleBar.root);
  const statusBarCtl = segmentedControl<StatusBar>('Status bar', STATUS_BARS.map((key) => [key, STATUS_BAR_LABELS[key]] as const), (value) => change((d) => { d.layout.statusBar = value; rememberOwnLayout(d); }));
  row(layoutCard, 'statusBar', statusBarCtl.root);
  const densityCtl = segmentedControl<UiDensity>('User interface density', UI_DENSITIES.map((key) => [key, key === 'regular' ? 'Regular' : 'Compact'] as const), (value) => change((d) => { d.layout.uiDensity = value; }));
  row(layoutCard, 'uiDensity', densityCtl.root);
  const compactMenus = switchControl((on) => change((d) => { d.layout.compactMenus = on; }));
  row(layoutCard, 'compactMenus', compactMenus.root);
  // UI zoom: a slider, the value as a field you can type into, and Reset.
  const uiZoomRange = el('input', 'sp-range');
  uiZoomRange.type = 'range';
  uiZoomRange.min = String(MIN_UI_ZOOM);
  uiZoomRange.max = String(MAX_UI_ZOOM);
  uiZoomRange.step = String(UI_ZOOM_STEP);
  uiZoomRange.setAttribute('aria-label', 'User interface zoom');
  const uiZoomValue = el('input', 'bw-input sp-zoom-input sp-ui-zoom-value');
  uiZoomValue.type = 'text';
  uiZoomValue.inputMode = 'numeric';
  uiZoomValue.maxLength = 4;
  uiZoomValue.autocomplete = 'off';
  uiZoomValue.setAttribute('aria-label', 'User interface zoom, percent');
  const uiZoomReset = textButton('Reset', () => change((d) => { d.layout.uiZoom = DEFAULT_UI_ZOOM; }));
  const uiZoomWrap = el('div', 'sp-range-wrap sp-ui-zoom');
  uiZoomWrap.append(uiZoomRange, uiZoomValue, uiZoomReset);
  let uiZoomShown = DEFAULT_UI_ZOOM;
  const paintUiZoom = (value: number): void => {
    uiZoomShown = value;
    uiZoomRange.value = String(value);
    uiZoomRange.style.setProperty('--sp-fill', `${((value - MIN_UI_ZOOM) / (MAX_UI_ZOOM - MIN_UI_ZOOM)) * 100}%`);
    uiZoomRange.setAttribute('aria-valuetext', `${value} percent`);
    uiZoomValue.value = uiZoomText(value);
    uiZoomReset.disabled = value === DEFAULT_UI_ZOOM || !(snapshot?.readable ?? false);
  };
  uiZoomRange.addEventListener('input', () => paintUiZoom(Number(uiZoomRange.value)));
  uiZoomRange.addEventListener('change', () => change((d) => { d.layout.uiZoom = Number(uiZoomRange.value); }));
  uiZoomValue.addEventListener('change', () => {
    const typed = parseUiZoom(uiZoomValue.value);
    if (typed === null) { showToast(`Enter a zoom from ${MIN_UI_ZOOM} to ${MAX_UI_ZOOM} percent.`, false); paintUiZoom(uiZoomShown); return; }
    paintUiZoom(typed);
    change((d) => { d.layout.uiZoom = typed; });
  });
  row(layoutCard, 'uiZoom', uiZoomWrap);
  // --- end layout ---

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
  // --- theme options --- the schedule (core), then the colours, the colouring mode, the page's accent,
  // saturation, contrast and the theme file (behind More options). Each is its own `themeopt` command.
  const sendThemeOption = (key: string, value: ThemeOptionValue): void => {
    const text = themeAssignment(key, value);
    if (text === null) { showToast('That value is not allowed.', false); return; }
    if (cmd({ a: 'themeopt', text })) pending += 1;
  };
  const themeRows = mountThemeRows({
    card: themeCard,
    row,
    switchControl,
    segmentedControl,
    send: sendThemeOption,
    // A cancelled file dialog answers with nothing, so the file commands are not counted as pending.
    action: (name) => { if (cmd({ a: name }) && name === 'themereset') pending += 1; },
  });
  // --- end theme options ---

  // Website appearance and the private tab theme (thread T3). --- settings slim --- they join the
  // Theme card, so the core view is one card of colour choices.
  row(themeCard, 'websiteAppearance', protection.websiteAppearance.root);
  row(themeCard, 'forceDark', protection.forceDark.root);
  row(themeCard, 'privateTheme', protection.privateTheme.root);

  // Toolbars (thread T13): the editor for the address bar, the tab bar and the panel bar, on a sub-page of its own.
  // The address bar's list is part of the page's Save (`toolbar_buttons`); the other two bars are each a `baropt` command.
  const toolbarCard = card('appearance', 'Toolbar');
  const sendBarOption = (text: string | null): void => {
    if (text === null) { showToast('That value is not allowed.', false); return; }
    if (cmd({ a: 'baropt', text })) pending += 1;
  };
  let editorCommands: readonly EditorCommand[] = [];
  const toolbarEditor = mountToolbarEditor({
    onChange: (bar: BarId, items: string[]) => {
      if (bar === 'address') change((d) => { d.toolbarButtons = itemsText(items); });
      else sendBarOption(`${BAR_KEYS[bar]}=${itemsText(items)}`);
    },
  });
  row(toolbarCard, 'toolbarButtons', toolbarEditor.root, { stack: true });
  const showHome = switchControl((on) => change((d) => { d.showHomeButton = on; }));
  row(toolbarCard, 'showHomeButton', showHome.root);

  // Address bar (--- settings slim --- in the Toolbar card it belongs to)
  const addressCard = toolbarCard;
  const centred = switchControl((on) => change((d) => { d.centredAddressBar = on; }));
  row(addressCard, 'centredAddressBar', centred.root);
  const minimal = switchControl((on) => change((d) => { d.minimalUrl = on; }));
  row(addressCard, 'minimalUrl', minimal.root);
  const loadProgress = switchControl((on) => change((d) => { d.showLoadProgress = on; }));
  row(addressCard, 'showLoadProgress', loadProgress.root);

  // Tab groups and split view. The new-tab position, Ctrl+Tab order and tab preview rows live in the
  // Tabs section (wave-1 integration), which also owns `new_tab_beside_active` through its position row.
  const tabsCard = card('appearance', 'Tab groups and split view');
  const grouping = switchControl((on) => change((d) => { d.automaticTabGrouping = on; }));
  row(tabsCard, 'grouping', grouping.root);
  const mru = switchControl((on) => change((d) => { d.ctrlTabMru = on; }));
  // --- T10 --- drawn in Tab stacks, tiling and workspaces, beside the other tiling options.
  const edgeDrop = switchControl((on) => change((d) => { d.splitEdgeDrop = on; }));
  const previewImages = switchControl((on) => change((d) => { d.tabPreviewImages = on; }));
  const previewMemory = switchControl((on) => change((d) => { d.tabPreviewMemory = on; }));
  const pinGroups = switchControl((on) => change((d) => { d.pinNewTabGroups = on; }));
  row(tabsCard, 'pinNewTabGroups', pinGroups.root);
  const vTabs = switchControl((on) => change((d) => { d.ctrlSVerticalTabs = on; }));

  // Window
  const windowCard = card('appearance', 'Window');
  const rounded = switchControl((on) => change((d) => { d.roundedContentFrame = on; }));
  row(windowCard, 'roundedContentFrame', rounded.root);
  // --- auto-hide (T11) --- the master switch, and the two rows behind "More options" (see ROWS).
  const frameless = switchControl((on) => change((d) => { d.frameless = on; }));
  row(windowCard, 'frameless', frameless.root);
  const autoHideBarSwitches = new Map<AutoHideBar, Control<boolean>>();
  const autoHideList = el('div', 'sp-toolbar-list');
  for (const bar of AUTO_HIDE_BARS) {
    const item = el('div', 'sp-toolbar-item');
    const label = el('span', 'sp-label');
    label.textContent = AUTO_HIDE_BAR_LABELS[bar];
    const sw = switchControl((on) => change((d) => {
      const bars = new Set(autoHideBarsList(d.autoHideBars));
      if (on) bars.add(bar); else bars.delete(bar);
      d.autoHideBars = autoHideBarsText(bars);
    }));
    sw.root.setAttribute('aria-label', AUTO_HIDE_BAR_LABELS[bar]);
    autoHideBarSwitches.set(bar, sw);
    item.append(label, sw.root);
    autoHideList.append(item);
  }
  row(windowCard, 'autoHideBars', autoHideList, { stack: true });
  const autoHideFullscreen = switchControl((on) => change((d) => { d.autoHideFullscreen = on; }));
  row(windowCard, 'autoHideFullscreen', autoHideFullscreen.root);
  // --- end auto-hide ---
  // --- window and panels (thread T9) --- Window appearance and Panels: each switch saves the draft and acts at once.
  const winAnimations = switchControl((on) => change((d) => { d.window.animations = on; }));
  row(windowCard, 'winAnimations', winAnimations.root);
  const winRangeButtons = switchControl((on) => change((d) => { d.window.rangeButtons = on; }));
  row(windowCard, 'winRangeButtons', winRangeButtons.root);
  const winFade = switchControl((on) => change((d) => { d.window.fadeBackground = on; }));
  row(windowCard, 'winFade', winFade.root);
  const winScrollbars = switchControl((on) => change((d) => { d.window.simpleScrollbars = on; }));
  row(windowCard, 'winScrollbars', winScrollbars.root);
  const winMonoIcons = switchControl((on) => change((d) => { d.window.monoIcons = on; }));
  row(windowCard, 'winMonoIcons', winMonoIcons.root);
  const winReset = textButton('Reset window options', () => change((d) => { d.window = resetWindowGroup(d.window, WINDOW_APPEARANCE_KEYS); }));
  row(windowCard, 'winReset', winReset);
  const panelsCard = card('appearance', 'Panels');
  const panelClose = switchControl((on) => change((d) => { d.window.panelCloseButton = on; }));
  row(panelsCard, 'panelCloseButton', panelClose.root);
  const panelCounts = switchControl((on) => change((d) => { d.window.panelFolderCounts = on; }));
  row(panelsCard, 'panelFolderCounts', panelCounts.root);
  const panelAutoClose = switchControl((on) => change((d) => { d.window.panelAutoClose = on; }));
  row(panelsCard, 'panelAutoClose', panelAutoClose.root);
  const panelReset = textButton('Reset panel options', () => change((d) => { d.window = resetWindowGroup(d.window, WINDOW_PANEL_KEYS); }));
  row(panelsCard, 'panelReset', panelReset);
  // The slider rows get minus and plus buttons; which of the two shows follows "Buttons instead of sliders".
  const addSteppers = (wrap: HTMLElement, range: HTMLInputElement, label: string, commit: (value: number) => void): void => {
    const group = el('div', 'sp-stepper');
    const step = (direction: -1 | 1): void => {
      const next = stepValue(Number(range.value), direction, Number(range.step), Number(range.min), Number(range.max));
      range.value = String(next);
      commit(next);
    };
    const less = textButton('−', () => step(-1));
    less.setAttribute('aria-label', `Decrease ${label}`);
    const more = textButton('+', () => step(1));
    more.setAttribute('aria-label', `Increase ${label}`);
    group.append(less, more);
    wrap.append(group);
  };
  addSteppers(widthWrap, width, 'sidebar width', (value) => { paintWidth(); change((d) => { d.sidebarWidth = value; }); });
  addSteppers(uiZoomWrap, uiZoomRange, 'interface zoom', (value) => { paintUiZoom(value); change((d) => { d.layout.uiZoom = value; }); });
  // --- end window and panels ---

  // Bookmarks bar
  const bookmarksCard = card('appearance', 'Bookmarks bar');
  const bookmarksBarCtl = segmentedControl<BookmarksBarMode>('Bookmarks bar', BOOKMARKS_BAR_MODES.map((key) => [key, key === 'always' ? 'Always show' : key === 'never' ? 'Always hide' : 'Only on the new tab'] as const), (value) => change((d) => { d.bookmarksBar = value; }));
  row(bookmarksCard, 'bookmarksBar', bookmarksBarCtl.root);
  const bookmarksGroups = switchControl((on) => change((d) => { d.bookmarksBarTabGroups = on; }));
  row(bookmarksCard, 'bookmarksBarTabGroups', bookmarksGroups.root);

  // --- layout (thread T6) --- the side panel's edge is the Panels choice of the Layout block above.

  // Fonts and zoom (--- settings slim --- one card)
  const fontsCard = card('appearance', 'Text and zoom');
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

  const zoomCard = fontsCard;
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

  // Translation: each control is its own command, applied at once and never part of `save`.
  const translateCard = card('appearance', 'Translation');
  const translateOffer = switchControl((on) => { if (cmd({ a: 'translateoffer', on })) pending += 1; });
  row(translateCard, 'translateOffer', translateOffer.root);
  const translateAuto = switchControl((on) => { if (cmd({ a: 'translateauto', on })) pending += 1; });
  row(translateCard, 'translateAuto', translateAuto.root);
  const translateHost = el('div', 'sp-select-host');
  let translateSelect: SelectField<string> | null = null;
  let translateChoices = '';
  const paintTranslateSelect = (target: string, codes: readonly string[]): void => {
    const key = codes.join(',');
    if (!translateSelect || key !== translateChoices) {
      translateChoices = key;
      translateSelect?.close();
      translateSelect = selectField<string>({
        options: translateTargetOptions(codes),
        value: target,
        label: 'Translate into',
        placeholder: 'Translate into',
        onChange: (next) => { if (cmd({ a: 'translatetarget', text: next })) pending += 1; },
      });
      const rowNode = rows.get('translateTarget');
      translateSelect.button.setAttribute('aria-labelledby', 'sp-l-translateTarget');
      if (rowNode?.desc) translateSelect.button.setAttribute('aria-describedby', rowNode.desc.id);
      translateHost.replaceChildren(translateSelect.root);
    } else {
      translateSelect.set(target);
    }
  };
  row(translateCard, 'translateTarget', translateHost);

  // The three shortcut switches are built into Shortcuts and commands, after the Keyboard rows.
  const shiftRightClick = switchControl((on) => change((d) => { d.shiftRightClickContext = on; }));
  const copyLink = switchControl((on) => change((d) => { d.ctrlShiftCCopyLink = on; }));

  // Tabs: every option is a `tabopt` command of its own (`key=value`), applied at once, never part of `save`.
  const tabControls = new Map<string, { set(values: TabOptionValues): void; disable(off: boolean): void }>();
  const sendTabOption = (key: string, value: boolean | number | string): void => {
    const text = assignmentText(key, value);
    if (text === null) { showToast('That value is not allowed.', false); return; }
    if (cmd({ a: 'tabopt', text })) pending += 1;
  };
  const buildTabControl = (spec: TabOptionSpec): HTMLElement => {
    switch (spec.kind) {
      case 'toggle': {
        const sw = switchControl((on) => sendTabOption(spec.key, on));
        // --- two-level stacks (T10b) --- a switch that only matters for one choice is off-limits otherwise.
        let unreadable = false;
        let applies = true;
        tabControls.set(spec.key, {
          set: (values) => { sw.set(values[spec.key] === true); applies = toggleApplies(spec, values); sw.disable(unreadable || !applies); },
          disable: (off) => { unreadable = off; sw.disable(unreadable || !applies); },
        });
        return sw.root;
      }
      case 'choice': {
        const seg = segmentedControl<string>(spec.label, spec.choices.map(([value, label]) => [value, label] as const), (value) => sendTabOption(spec.key, value));
        tabControls.set(spec.key, { set: (values) => seg.set(String(values[spec.key] ?? spec.default)), disable: (off) => seg.disable(off) });
        return seg.root;
      }
      case 'number': {
        const input = el('input', 'bw-input sp-zoom-input');
        input.type = 'number';
        input.min = String(spec.min);
        input.max = String(spec.max);
        input.step = '1';
        input.setAttribute('aria-label', spec.label);
        const suffix = el('span', 'sp-value');
        const wrap = el('div', 'sp-zoom-wrap');
        wrap.append(input, suffix);
        const paint = (n: number): void => { input.value = String(n); suffix.textContent = numberLabel(spec, n); };
        // Each save is a full read, merge and write in Rust, so a run of spinner steps sends only its
        // last value: 400 ms after the last change, or at once when the field loses focus.
        let timer = 0;
        let queued: number | null = null;
        const flush = (): void => {
          if (timer) { clearTimeout(timer); timer = 0; }
          if (queued === null) return;
          const n = queued;
          queued = null;
          sendTabOption(spec.key, n);
        };
        input.addEventListener('change', () => {
          const n = clampNumber(spec, Number(input.value));
          paint(n);
          queued = n;
          if (timer) clearTimeout(timer);
          timer = window.setTimeout(flush, TAB_NUMBER_DEBOUNCE_MS);
        });
        input.addEventListener('blur', flush);
        // Closing or hiding the page inside the wait still saves the value the field shows.
        window.addEventListener('pagehide', flush);
        document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'hidden') flush(); });
        // A host update while a value waits must not repaint the field under the user.
        tabControls.set(spec.key, { set: (values) => { if (queued === null) paint(Number(values[spec.key] ?? spec.default)); }, disable: (off) => { input.disabled = off; } });
        return wrap;
      }
      case 'text': {
        const input = el('input', 'bw-input sp-ui-font-input');
        input.type = 'text';
        input.placeholder = spec.placeholder;
        input.maxLength = spec.max;
        input.autocomplete = 'off';
        input.spellcheck = false;
        input.setAttribute('aria-label', spec.label);
        input.addEventListener('change', () => sendTabOption(spec.key, input.value.trim()));
        let unreadable = false;
        let latest: TabOptionValues = {};
        const refresh = (): void => { input.disabled = unreadable || !textApplies(spec, latest); };
        tabControls.set(spec.key, {
          set: (values) => { latest = values; input.value = String(values[spec.key] ?? ''); refresh(); },
          disable: (off) => { unreadable = off; refresh(); },
        });
        return input;
      }
    }
  };
  // --- tab stacks, tiling and workspaces (T10) --- the workspace rules editor, a sub-page of its own.
  let ruleWorkspaces: readonly string[] = [];
  let ruleList: WorkspaceRules['rules'] = [];
  let ruleWorkspace = '';
  const rulesList = el('ul', 'sp-sites');
  rulesList.setAttribute('aria-label', 'Workspace rules');
  const rulesForm = el('div', 'sp-allow-add');
  const ruleSite = el('input', 'bw-input sp-allow-input');
  ruleSite.type = 'text';
  ruleSite.placeholder = 'example.com';
  ruleSite.autocomplete = 'off';
  ruleSite.spellcheck = false;
  ruleSite.maxLength = 253;
  ruleSite.setAttribute('aria-label', 'Site');
  const ruleTargetHost = el('div', 'sp-select-host');
  let ruleSelect: SelectField<string> | null = null;
  let ruleChoices = '';
  const ruleError = el('p', 'sp-field-error');
  ruleError.setAttribute('role', 'alert');
  const addRule = (): void => {
    if (ruleSiteOf(ruleSite.value) === null) { ruleError.textContent = 'Enter a site such as example.com.'; return; }
    if (ruleExists(ruleList, ruleSite.value)) { ruleError.textContent = 'That site already has a rule.'; return; }
    const text = addRuleText(ruleSite.value, ruleWorkspace);
    if (text === null) { ruleError.textContent = 'Choose a workspace.'; return; }
    ruleError.textContent = '';
    ruleSite.value = '';
    if (cmd({ a: 'wsrule', text })) pending += 1;
  };
  const ruleAdd = textButton('Add', addRule, { icon: 'plus' });
  ruleSite.addEventListener('keydown', (event) => { if (event.key === 'Enter') { event.preventDefault(); addRule(); } });
  ruleSite.addEventListener('input', () => { ruleError.textContent = ''; });
  rulesForm.append(ruleSite, ruleTargetHost, ruleAdd);
  const paintRules = (value: WorkspaceRules): void => {
    ruleList = value.rules;
    ruleWorkspaces = value.workspaces;
    if (!ruleWorkspaces.includes(ruleWorkspace)) ruleWorkspace = ruleWorkspaces[0] ?? '';
    const key = ruleWorkspaces.join('\n');
    if (!ruleSelect || key !== ruleChoices) {
      ruleChoices = key;
      ruleSelect?.close();
      ruleSelect = selectField<string>({
        options: ruleWorkspaces.map((name) => ({ value: name, label: name })),
        value: ruleWorkspace,
        label: 'Workspace',
        placeholder: 'Workspace',
        onChange: (next) => { ruleWorkspace = next; },
      });
      ruleTargetHost.replaceChildren(ruleSelect.root);
    } else {
      ruleSelect.set(ruleWorkspace);
    }
    rulesList.replaceChildren(...value.rules.map((rule) => {
      const item = el('li', 'sp-site');
      const name = el('span', 'sp-site-name');
      name.textContent = `${rule.site} → ${rule.workspace}`;
      name.dir = 'auto';
      const missing = !ruleWorkspaces.includes(rule.workspace);
      if (missing) name.setAttribute('data-tip', 'No workspace has this name, so the rule rests until one does.');
      item.classList.toggle('is-muted', missing);
      const remove = el('button', 'bw-icon-btn sp-site-remove');
      remove.type = 'button';
      remove.setAttribute('aria-label', `Remove the rule for ${rule.site}`);
      remove.setAttribute('data-tip', 'Remove');
      remove.append(icon('close', { size: 12, fine: true }));
      remove.addEventListener('click', () => {
        if (cmd({ a: 'wsrule', text: removeRuleText(rule.site) })) pending += 1;
        ruleSite.focus();
      });
      item.append(name, remove);
      return item;
    }));
    rulesList.hidden = value.rules.length === 0;
  };
  // --- end tab stacks ---
  for (const cardKey of TAB_CARDS) {
    // --- T10 --- the stacks card is its own part of the Tabs and start page area.
    const tabCard = card(cardKey === 'stacks' ? 'stacks' : 'tabs', TAB_CARD_TITLES[cardKey]);
    for (const spec of cardSpecs(cardKey)) row(tabCard, spec.key, buildTabControl(spec));
    if (cardKey === 'stacks') {
      row(tabCard, 'splitEdgeDrop', edgeDrop.root);
      const rulesRow = row(tabCard, 'workspaceRules', rulesForm);
      rulesRow.root.classList.add('is-wrap', 'is-tight');
      rulesRow.root.append(rulesList, ruleError);
    }
    // --- wave-1 integration --- the older tab rows join the Tabs section's tab bar card.
    if (cardKey === 'bar') {
      row(tabCard, 'ctrlTabMru', mru.root);
      row(tabCard, 'tabPreviewImages', previewImages.root);
      row(tabCard, 'tabPreviewMemory', previewMemory.root);
    }
  }

  // Privacy and security
  const blockCard = card('privacy', 'Blocking');
  // The old on/off switch is the "None" choice of the blocking level (blocker_enabled=false).
  row(blockCard, 'blocker', protection.level.root);
  row(blockCard, 'cookieNotices', protection.cookieNotices.root);
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
  row(trackCard, 'historyRetention', protection.retention.root);

  // The blocks of the protection controls (thread T3): per-site choices, the rule lists and Delete
  // Browsing Data. Each is one block, so search treats it as one row.
  for (const [id, block] of [['siteRules', protection.sites], ['blockLists', protection.lists], ['deleteData', protection.deleteData]] as const) {
    const host = el('div', 'sp-dash');
    host.dataset.row = id;
    host.append(block);
    sectionNodes.get('privacy')?.append(host);
    rows.set(id, { spec: ROWS.find((r) => r.id === id)!, root: host, label: el('span'), desc: null, card: host, flag: null, target: null });
  }

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

  // --- start page --- the Start page section builds its own rows through this page's helpers
  const startSection = mountStartSection({
    card: (section, heading) => card(section, heading),
    row: (into, id, control, options) => row(into, id, control, options),
    change: (edit) => change((d) => edit(d.start)),
    switchControl,
    segmentedControl,
    toast: (text, ok) => showToast(text, ok),
  });
  // --- end start page ---

  // Address bar, search engines and bookmarks (src/address-section.ts): its own snapshot, its own
  // commands in the `set` family. The bookmarks bar's rows join the page's existing ones in Appearance.
  const addressSections = mountAddressSections({
    send: cmd,
    toast: (text, ok) => showToast(text, ok),
    card,
    row: (into, id, control, options) => row(into, id, control, options),
    barCard: bookmarksCard,
    switchControl,
    segmentedControl,
  });

  // Page tools (src/page-tools-section.ts): its own snapshot and commands in the `set` family. The few core
  // rows and the rest behind "More options" are decided by row id in settings-page-logic.ts.
  const pageToolsSection = mountPageToolsSection({
    send: cmd,
    toast: (text, ok) => showToast(text, ok),
    card,
    row: (into, id, control, options) => row(into, id, control, options),
    switchControl,
    segmentedControl,
  });

  // Feeds (thread T13, docs/feeds.md): open the panel, the address bar's feed button, and when Feeds fetches. Each is
  // its own command (`openfeeds`, `baropt`), applied at once; the rows join the page's search like any other.
  const feedsCard = card('address', 'Feeds');
  const feedsOpen = textButton('Open Feeds', () => cmd({ a: 'openfeeds' }), { icon: 'feed' });
  row(feedsCard, 'feedsOpen', feedsOpen);
  const feedsDetect = switchControl((on) => sendBarOption(feedAssignment('feeds_detect', on)));
  row(feedsCard, 'feedsDetect', feedsDetect.root);
  const feedsRefreshHost = el('div', 'sp-select-host');
  const feedsRefreshSelect = selectField<string>({
    options: REFRESH_CHOICES.map((minutes) => ({ value: String(minutes), label: refreshLabel(minutes) })),
    value: String(60),
    label: 'Fetch feeds again when Feeds opens',
    placeholder: 'Fetch feeds again when Feeds opens',
    onChange: (next) => sendBarOption(feedAssignment('feeds_refresh_minutes', Number(next) as RefreshChoice)),
  });
  feedsRefreshSelect.button.setAttribute('aria-labelledby', 'sp-l-feedsRefresh');
  feedsRefreshHost.append(feedsRefreshSelect.root);
  row(feedsCard, 'feedsRefresh', feedsRefreshHost);

  // Keyboard and Quick commands (docs/commands.md): each control is its own command, answered by the
  // `commands` snapshot, never part of `save`. The rows join the page's search like any other.
  const commandSections = mountCommandsSections({ send: cmd, toast: (text, ok) => showToast(text, ok) });
  sectionNodes.get('keyboard')?.append(...commandSections.keyboard);
  sectionNodes.get('commands')?.append(...commandSections.commands);
  for (const found of commandSections.rows) rows.set(found.spec.id, { ...found, flag: null, target: null });
  groups.push(...commandSections.groups);
  // --- settings slim --- the shortcut switches that lived in Appearance, beside the other keyboard options.
  const shortcutsCard = el('div', 'sp-card');
  {
    const group = el('div', 'sp-group');
    const heading = el('h3', 'sp-group-h');
    heading.id = `sp-card-${++cardSeq}`;
    heading.textContent = 'Shortcuts';
    shortcutsCard.setAttribute('role', 'group');
    shortcutsCard.setAttribute('aria-labelledby', heading.id);
    group.append(heading, shortcutsCard);
    // After the Keyboard options and websites, before the shortcut list.
    const kb = sectionNodes.get('keyboard')!;
    const editorBlock = kb.querySelector('.sp-kb');
    if (editorBlock) editorBlock.replaceWith(group); else kb.append(group);
    if (editorBlock) group.after(editorBlock);
    groups.push({ root: group, heading, card: shortcutsCard });
  }
  row(shortcutsCard, 'shiftRightClickContext', shiftRightClick.root);
  row(shortcutsCard, 'ctrlShiftCCopyLink', copyLink.root);
  row(shortcutsCard, 'ctrlSVerticalTabs', vTabs.root);

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
  const store = mountStoreSection({ send: cmd, toast: (text, ok) => showToast(text, ok) });
  root.append(store.root);
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

  // ---- --- settings slim --- arranging the rows: core, "More options", sub-pages, "Not active yet" ----

  interface Disclosure {
    readonly key: string; readonly root: HTMLElement; readonly button: HTMLButtonElement; readonly body: HTMLElement;
    readonly desc: HTMLElement; readonly groups: Map<string, HTMLElement>; count: number;
  }
  /** Where each row ended up: its area, the part it shows in (null inside a disclosure), its disclosure, or its sub-page. */
  interface Place { readonly area: string; readonly part: SettingsSection | null; readonly disclosure: Disclosure | null; readonly page: boolean }
  interface Opener { readonly id: string; readonly root: HTMLElement; readonly label: HTMLElement; readonly desc: HTMLElement | null; readonly button: HTMLButtonElement; readonly part: SettingsSection }
  interface SubPage { readonly id: string; readonly root: HTMLElement; readonly heading: HTMLElement; readonly area: AreaSpec }
  const places = new Map<string, Place>();
  const openers = new Map<string, Opener>();
  const subPages = new Map<string, SubPage>();
  const disclosures: Disclosure[] = [];
  const openState = readOpen();
  let openPage: SubPage | null = null;
  let searching = false;

  const paintDisclosure = (d: Disclosure, open: boolean): void => {
    d.button.setAttribute('aria-expanded', String(open));
    d.root.classList.toggle('is-open', open);
    d.body.hidden = !open;
  };
  /** One disclosure row: a full-width button in a card, its body (groups of rows) below it. */
  const disclosure = (key: string, label: string): Disclosure => {
    const root = el('div', 'sp-more');
    root.dataset.more = key;
    const button = el('button', 'sp-more-btn');
    button.type = 'button';
    button.id = `sp-more-b-${key}`;
    const text = el('span', 'sp-text');
    const name = el('span', 'sp-label');
    name.textContent = label;
    const desc = el('span', 'sp-desc');
    text.append(name, desc);
    button.append(text, icon('chevron-down', { size: 16, className: 'sp-more-chev' }));
    const head = el('div', 'sp-card sp-more-card');
    head.append(button);
    const body = el('div', 'sp-more-body');
    body.id = `sp-more-${key}`;
    body.setAttribute('role', 'region');
    body.setAttribute('aria-labelledby', button.id);
    button.setAttribute('aria-controls', body.id);
    root.append(head, body);
    const d: Disclosure = { key, root, button, body, desc, groups: new Map(), count: 0 };
    button.addEventListener('click', () => {
      const open = button.getAttribute('aria-expanded') !== 'true';
      if (open) openState.add(key); else openState.delete(key);
      writeOpen(openState);
      paintDisclosure(d, open);
    });
    // Using a row found by the search keeps its disclosure open after the search is cleared.
    body.addEventListener('focusin', () => {
      if (!searching || openState.has(key)) return;
      openState.add(key);
      writeOpen(openState);
    });
    paintDisclosure(d, openState.has(key));
    disclosures.push(d);
    return d;
  };
  /** The card for `heading` (a row's old group) inside a disclosure, made on first use. */
  const disclosureCard = (d: Disclosure, groupKey: string, heading: string): HTMLElement => {
    const found = d.groups.get(groupKey);
    if (found) return found;
    const group = el('div', 'sp-group');
    const node = el('div', 'sp-card');
    node.setAttribute('role', 'group');
    let h: HTMLElement | null = null;
    if (heading !== '') {
      h = el('h3', 'sp-group-h');
      h.id = `sp-card-${++cardSeq}`;
      h.textContent = heading;
      node.setAttribute('aria-labelledby', h.id);
      group.append(h);
    }
    group.append(node);
    d.body.append(group);
    groups.push({ root: group, heading: h, card: node });
    d.groups.set(groupKey, node);
    return node;
  };
  const groupOfCard = (cardNode: HTMLElement): Group | undefined => groups.find((g) => g.card === cardNode);

  // Sub-pages: the big editors, each on a page of its own with a way back.
  const closePage = (refocus: boolean): void => {
    const shown = openPage;
    if (!shown) return;
    openPage = null;
    page.classList.remove('is-sub');
    shown.root.hidden = true;
    for (const node of [...areaNodes.values(), laterNode]) node.hidden = false;
    const opener = openers.get(shown.id);
    if (opener && refocus) {
      opener.root.scrollIntoView({ block: 'center' });
      opener.button.focus({ preventScroll: true });
    }
  };
  const showPage = (id: string): void => {
    unpinSection();
    const sub = subPages.get(id);
    if (!sub) return;
    if (query !== '') { input.value = ''; query = ''; filter(); }
    if (openPage && openPage !== sub) openPage.root.hidden = true;
    openPage = sub;
    page.classList.add('is-sub');
    for (const node of [...areaNodes.values(), laterNode]) node.hidden = true;
    empty.hidden = true;
    sub.root.hidden = false;
    main.scrollTop = 0;
    if (current !== sub.area.key) { current = sub.area.key; paintNav(); }
    sub.heading.focus({ preventScroll: true });
  };
  const openerRow = (id: string, spec: RowSpec): Opener => {
    const node = el('div', 'sp-row');
    node.dataset.opens = id;
    const text = el('div', 'sp-text');
    const label = el('span', 'sp-label');
    label.id = `sp-l-open-${id}`;
    label.textContent = spec.label;
    text.append(label);
    let desc: HTMLElement | null = null;
    if (spec.description !== '') {
      desc = el('span', 'sp-desc');
      desc.id = `sp-d-open-${id}`;
      desc.textContent = spec.description;
      text.append(desc);
    }
    const button = textButton(PAGE_ROWS[id]!.button, () => showPage(id));
    button.setAttribute('aria-describedby', [label.id, desc?.id].filter(Boolean).join(' '));
    const wrap = el('div', 'sp-ctl');
    wrap.append(button);
    node.append(text, wrap);
    const made = { id, root: node, label, desc, button, part: spec.section };
    openers.set(id, made);
    return made;
  };
  const subPage = (node: RowNode, area: AreaSpec): SubPage => {
    const sub = el('section', 'sp-sec sp-page');
    sub.dataset.page = node.spec.id;
    sub.hidden = true;
    const back = textButton(area.title, () => closePage(true), { icon: 'chevron-left', subtle: true });
    back.classList.add('sp-back');
    back.setAttribute('aria-label', `Back to ${area.title}`);
    const heading = el('h2', 'sp-h');
    heading.id = `sp-h-page-${node.spec.id}`;
    heading.tabIndex = -1;
    heading.textContent = node.spec.label;
    sub.setAttribute('aria-labelledby', heading.id);
    const lead = el('p', 'sp-lead');
    lead.textContent = node.spec.description;
    sub.append(back, heading, lead);
    // The editor's own title and lead would repeat the page's: they stay for the search marks, unseen.
    if (node.root.classList.contains('sp-row')) {
      node.root.classList.add('is-page');
      const box = el('div', 'sp-card');
      box.append(node.root);
      sub.append(box);
    } else {
      const title = Array.from(node.root.querySelectorAll<HTMLElement>('.sp-group-h')).find((h) => h.textContent === node.spec.label);
      const words = Array.from(node.root.querySelectorAll<HTMLElement>('.sp-desc')).find((d) => d.textContent === node.spec.description);
      if (title) title.hidden = true;
      if (words) words.hidden = true;
      sub.append(node.root);
    }
    column.append(sub, empty);
    const made = { id: node.spec.id, root: sub, heading, area };
    subPages.set(node.spec.id, made);
    return made;
  };

  // "Not active yet": the rows Bowser saves but does not act on, at the very end, collapsed.
  const laterNode = el('section', 'sp-sec sp-later');
  laterNode.dataset.area = 'later';
  {
    const heading = el('h2', 'sp-h');
    heading.id = 'sp-h-later';
    heading.tabIndex = -1;
    heading.textContent = LATER_TITLE;
    laterNode.setAttribute('aria-labelledby', heading.id);
    const lead = el('p', 'sp-lead');
    lead.textContent = LATER_LEAD;
    laterNode.append(heading, lead);
  }
  const laterDisclosure = disclosure('later', LATER_LABEL);
  laterNode.append(laterDisclosure.root);

  /** True for a part whose rows are drawn as one card under the part's title (the merged areas' second and third parts too). */
  const mergedArea = (area: AreaSpec): boolean => area.parts.length > 1;
  const coreCards = new Map<SettingsSection, HTMLElement>();
  const coreGroups = new Map<SettingsSection, HTMLElement>();
  const coreCard = (part: SettingsSection): HTMLElement => {
    const found = coreCards.get(part);
    if (found) return found;
    const group = el('div', 'sp-group');
    const h = el('h3', 'sp-group-h');
    h.id = `sp-card-${++cardSeq}`;
    h.textContent = partTitle(part);
    const node = el('div', 'sp-card');
    node.setAttribute('role', 'group');
    node.setAttribute('aria-labelledby', h.id);
    group.append(h, node);
    sectionNodes.get(part)!.prepend(group);
    groups.push({ root: group, heading: h, card: node });
    coreCards.set(part, node);
    coreGroups.set(part, group);
    return node;
  };

  const arrange = (): void => {
    const moreOf = new Map<SettingsSection, Disclosure>();
    for (const area of AREAS) {
      const d = disclosure(area.key, MORE_LABEL);
      moreOf.set(area.key, d);
      areaNodes.get(area.key)!.append(d.root);
    }
    // Page order, so every list keeps the order the rows were built in.
    const ordered = Array.from(column.querySelectorAll<HTMLElement>('[data-row]')).map((node) => rows.get(node.dataset.row ?? '')).filter((node): node is RowNode => node !== undefined);
    for (const node of ordered) {
      const spec = node.spec;
      const area = areaOf(spec.section);
      const tier = rowTier(spec);
      const inCard = node.card !== node.root;
      const heading = inCard ? groupOfCard(node.card)?.heading?.textContent ?? '' : '';
      if (tier === 'core') {
        if (mergedArea(area)) coreCard(spec.section).append(node.root);
        places.set(spec.id, { area: area.key, part: spec.section, disclosure: null, page: false });
      } else if (tier === 'page') {
        const opener = openerRow(spec.id, spec);
        const after = PAGE_ROWS[spec.id]?.after;
        const anchor = after === undefined ? null : openers.get(after)?.root ?? rows.get(after)?.root ?? null;
        // The editor itself moves to its sub-page below.
        if (anchor) {
          anchor.after(opener.root);
        } else if (mergedArea(area)) {
          coreCard(spec.section).append(opener.root);
        } else if (inCard) {
          node.root.replaceWith(opener.root);
        } else {
          const group = el('div', 'sp-group');
          const box = el('div', 'sp-card');
          group.append(box);
          node.root.replaceWith(group);
          box.append(opener.root);
          groups.push({ root: group, heading: null, card: box });
        }
        subPage(node, area);
        places.set(spec.id, { area: area.key, part: spec.section, disclosure: null, page: true });
      } else {
        const d = tier === 'later' ? laterDisclosure : moreOf.get(area.key)!;
        if (inCard) disclosureCard(d, tier === 'later' ? area.key : `${spec.section}|${heading}`, tier === 'later' ? area.title : heading).append(node.root);
        else d.body.append(node.root);
        d.count += 1;
        places.set(spec.id, { area: tier === 'later' ? 'later' : area.key, part: null, disclosure: d, page: false });
      }
    }
    for (const d of disclosures) {
      d.desc.textContent = d === laterDisclosure
        ? ordered.filter((node) => places.get(node.spec.id)?.disclosure === d).map((node) => node.spec.label).join(', ')
        : moreCountText(d.count);
      if (d.count === 0) d.root.remove();
    }
    // A note that belongs to a row follows it: "shortcuts are off" sits under the Keyboard shortcuts switch.
    const offNote = sectionNodes.get('keyboard')?.querySelector('.sp-cmd-note');
    if (offNote) coreGroups.get('keyboard')?.append(offNote);
    if (laterDisclosure.count === 0) laterNode.hidden = true;
    column.append(laterNode, ...[...subPages.values()].map((sub) => sub.root), empty);
  };
  arrange();

  /** Rows Bowser does not act on yet keep their saved value but never look like a control that works. */
  const lockLater = (): void => {
    for (const [id, place] of places) {
      if (place.area !== 'later') continue;
      const node = rows.get(id);
      if (!node) continue;
      for (const control of Array.from(node.root.querySelectorAll<HTMLButtonElement | HTMLInputElement>('button, input'))) {
        control.disabled = true;
        control.setAttribute('aria-disabled', 'true');
      }
    }
  };
  lockLater();

  // ---- applying a snapshot ----
  const apply = (s: SettingsSnapshot): void => {
    const settle = pending === 0;
    if (settle || !draft) draft = draftOf(s);
    const d = draft;
    // --- layout (thread T6) ---
    presets.set(selectedTile(d));
    paintSavedTile(d.layout.saved, s.readable);
    tabEdge.set(tabBarEdge(d.tabLayout));
    addressEdge.set(d.layout.addressBarPosition);
    panelEdge.set(d.sidePanelPosition);
    layout.set((TOP_TAB_STYLES as readonly string[]).includes(d.tabLayout) ? d.tabLayout : null);
    tabBarVisible.set(d.layout.tabBarVisible);
    addressBarVisible.set(d.layout.addressBarVisible);
    titleBar.set(d.layout.showTitleBar);
    statusBarCtl.set(d.layout.statusBar);
    densityCtl.set(d.layout.uiDensity);
    compactMenus.set(d.layout.compactMenus);
    paintUiZoom(d.layout.uiZoom);
    uiZoomRange.disabled = !s.readable;
    uiZoomValue.disabled = !s.readable;
    // --- end layout ---
    // --- window and panels (thread T9) ---
    winAnimations.set(d.window.animations);
    winRangeButtons.set(d.window.rangeButtons);
    winFade.set(d.window.fadeBackground);
    winScrollbars.set(d.window.simpleScrollbars);
    winMonoIcons.set(d.window.monoIcons);
    panelClose.set(d.window.panelCloseButton);
    panelCounts.set(d.window.panelFolderCounts);
    panelAutoClose.set(d.window.panelAutoClose);
    for (const wrapNode of [widthWrap, uiZoomWrap]) wrapNode.classList.toggle('is-steps', d.window.rangeButtons);
    winReset.disabled = !s.readable || windowGroupIsDefault(d.window, WINDOW_APPEARANCE_KEYS);
    panelReset.disabled = !s.readable || windowGroupIsDefault(d.window, WINDOW_PANEL_KEYS);
    // --- end window and panels ---
    width.value = String(d.sidebarWidth);
    paintWidth();
    grouping.set(d.automaticTabGrouping);
    previewImages.set(d.tabPreviewImages);
    previewMemory.set(d.tabPreviewMemory);
    mru.set(d.ctrlTabMru);
    pinGroups.set(d.pinNewTabGroups);
    edgeDrop.set(d.splitEdgeDrop);
    paintRules(s.workspaceRules); // --- tab stacks (T10) ---
    vTabs.set(d.ctrlSVerticalTabs);
    theme.set(d.theme);
    accentInput.value = d.themeAccent !== '' ? d.themeAccent : '#000000';
    // --- toolbars and feeds (thread T13) ---
    toolbarEditor.set({ address: itemsOf(d.toolbarButtons), tabs: itemsOf(s.bars.tabs), panel: itemsOf(s.bars.panel), commands: editorCommands });
    toolbarEditor.disable(!s.readable);
    feedsDetect.set(s.bars.feedsDetect);
    feedsDetect.disable(!s.readable || s.guest);
    feedsRefreshSelect.set(String(s.bars.feedsRefresh));
    feedsRefreshSelect.button.toggleAttribute('disabled', !s.readable || s.guest);
    feedsOpen.disabled = !s.readable;
    // --- end toolbars and feeds ---
    showHome.set(d.showHomeButton);
    centred.set(d.centredAddressBar);
    minimal.set(d.minimalUrl);
    loadProgress.set(d.showLoadProgress);
    frameless.set(d.frameless);
    // --- auto-hide (T11) --- the bar list only matters while the master switch is on.
    for (const [bar, sw] of autoHideBarSwitches) { sw.set(autoHideBarsList(d.autoHideBars).includes(bar)); sw.disable(!s.readable || !d.frameless); }
    autoHideFullscreen.set(d.autoHideFullscreen);
    // --- end auto-hide ---
    rounded.set(d.roundedContentFrame);
    bookmarksBarCtl.set(d.bookmarksBar);
    bookmarksGroups.set(d.bookmarksBarTabGroups);
    fontSizeCtl.set(d.fontSize);
    uiFont.value = d.uiFont;
    zoomInput.value = String(d.defaultZoom);
    zoomIndicator.set(d.showZoomIndicator);
    shiftRightClick.set(d.shiftRightClickContext);
    copyLink.set(d.ctrlShiftCCopyLink);
    blockYoutube.set(d.blockYoutubeVideoAds);
    defaultBrowserStatus.textContent = s.defaultBrowser === 'yes' ? 'Bowser is your default browser.'
      : s.defaultBrowser === 'no' ? 'Bowser is not your default browser.' : 'Bowser could not check whether it is the default browser.';
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
    downloadPath.textContent = s.downloadDir !== '' ? s.downloadDir : 'Your Downloads folder';
    downloadAsk.set(s.downloadAsk);
    translateOffer.set(s.translateOffer);
    translateAuto.set(s.translateAuto);
    paintTranslateSelect(s.translateTarget, s.translateTargets);
    for (const control of tabControls.values()) { control.set(s.tabOptions); control.disable(!s.readable); }
    themeRows.apply(s.themeOptions, s.readable, d.window.rangeButtons); // --- theme options ---
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
      layout, grouping, tracking, efficiency, sleep,
      previewImages, previewMemory, mru, pinGroups, edgeDrop, vTabs, theme, showHome, centred, minimal, loadProgress, frameless, autoHideFullscreen,
      rounded, bookmarksBarCtl, bookmarksGroups, fontSizeCtl, zoomIndicator, shiftRightClick, copyLink, blockYoutube,
      presets, tabEdge, addressEdge, panelEdge, tabBarVisible, addressBarVisible, titleBar, statusBarCtl, densityCtl, compactMenus, // --- layout (thread T6) ---
      winAnimations, winRangeButtons, winFade, winScrollbars, winMonoIcons, panelClose, panelCounts, panelAutoClose, // --- window and panels (thread T9) ---
    ]) control.disable(!s.readable);
    paintSavedTile(d.layout.saved, s.readable); // --- layout (thread T6) --- after the group-wide disable
    downloadAsk.disable(!s.readable);
    translateOffer.disable(!s.readable);
    translateAuto.disable(!s.readable);
    translateSelect?.button.toggleAttribute('disabled', !s.readable);
    downloadChange.disabled = !s.readable;
    width.disabled = !s.readable;
    allowInput.disabled = !s.readable;
    addButton.disabled = !s.readable;
    accentInput.disabled = !s.readable;
    accentReset.disabled = !s.readable;
    uiFont.disabled = !s.readable;
    zoomInput.disabled = !s.readable;
    startSection.apply(d.start, s.readable); // --- start page ---
    lockLater(); // --- settings slim ---
    if (query !== '') filter();
  };

  // ---- navigation: scroll position, active area, indicator ----
  const paintNav = (): void => {
    let index = 0;
    AREAS.forEach((spec, i) => {
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

  /** The destination is relative to main, including the browser's end-of-page clamp. */
  const sectionTop = (key: SettingsSection | ''): number => {
    const area = key === '' ? null : areaOf(key);
    const target = key === '' || area === null ? null : key === area.key ? areaNodes.get(area.key) : sectionNodes.get(key);
    return Math.max(0, Math.min(target ? target.offsetTop - 8 : 0, Math.max(0, main.scrollHeight - main.clientHeight)));
  };
  // Keep the chosen section at its destination when late snapshots, media results or the host's
  // first visible size change the layout. Pinning is navigation state, not a permanent scroll lock.
  let pinned: { key: SettingsSection | ''; top: number } | null = null;
  let pinFrame = 0;
  const unpinSection = (): void => { pinned = null; };
  const alignPinnedSection = (): void => {
    if (pinFrame || !pinned) return;
    pinFrame = requestAnimationFrame(() => {
      pinFrame = 0;
      // Read the latest navigation here: a queued old layout must not undo a newer pick or input.
      if (!pinned || page.hidden || openPage || query !== '' || !store.root.hidden) return;
      const top = sectionTop(pinned.key);
      // Do not interrupt an unchanged smooth navigation, or loop at the bottom of the document.
      if (Math.abs(top - pinned.top) < 0.5) return;
      pinned.top = top;
      main.scrollTo({ top, behavior: 'auto' });
    });
  };
  if (typeof ResizeObserver !== 'undefined') {
    const layout = new ResizeObserver(alignPinnedSection);
    layout.observe(main);
    layout.observe(column);
    // Opposing size changes can leave a column or merged area unchanged but move its target.
    for (const node of new Set([...areaNodes.values(), ...sectionNodes.values()])) layout.observe(node);
  }

  /** Shows `key`; `tell` reports it to Bowser (the tab's address follows), `smooth` animates. */
  const go = (key: SettingsSection | '', smooth: boolean, tell: boolean): void => {
    closePage(false);
    if (query !== '') { input.value = ''; query = ''; filter(); }
    const area = key === '' ? AREAS[0]! : areaOf(key);
    current = area.key;
    // The chosen area stays current even when the page cannot scroll it to the top (the last
    // ones), until the user scrolls by hand.
    const top = sectionTop(key);
    pinned = { key, top };
    paintNav();
    main.scrollTo({ top, behavior: smooth && !reduceMotion() ? 'smooth' : 'auto' });
    alignPinnedSection();
    if (tell) cmd({ a: 'section', text: key });
    if (area.key === 'media' && !mediaStarted) checkMedia();
    if (tell && key !== '') areaNodes.get(area.key)?.querySelector<HTMLElement>('h2')?.focus({ preventScroll: true });
  };

  // The area whose heading last passed a line a third of the way down is the current one.
  let scrollFrame = 0;
  for (const type of ['wheel', 'pointerdown', 'keydown', 'touchstart'] as const) main.addEventListener(type, unpinSection, { passive: true });
  main.addEventListener('scroll', () => {
    if (scrollFrame || pinned || openPage) return;
    scrollFrame = requestAnimationFrame(() => {
      scrollFrame = 0;
      if (pinned || openPage || page.hidden) return;
      const line = main.scrollTop + main.clientHeight / 3;
      let next: SettingsSection = 'profile';
      for (const spec of AREAS) {
        const node = areaNodes.get(spec.key);
        if (node && !node.hidden && node.offsetTop <= line) next = spec.key;
      }
      if (main.scrollTop + main.clientHeight >= main.scrollHeight - 2) {
        const visible = AREAS.filter((spec) => !areaNodes.get(spec.key)?.hidden);
        next = visible[visible.length - 1]?.key ?? next;
      }
      if (next !== current) { current = next; paintNav(); }
      if (current === 'media' && !mediaStarted) checkMedia();
    });
  }, { passive: true });
  for (const node of areaNodes.values()) node.querySelector<HTMLElement>('h2')!.tabIndex = -1;

  // ---- search: every row is found, wherever it is drawn; a disclosure with a hit opens ----
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
  /** A card shows while any row in it shows (an emptied card, its rows moved elsewhere, never). */
  const paintGroups = (): void => {
    for (const group of groups) group.root.hidden = !group.card.querySelector('.sp-row:not([hidden])');
  };
  function filter(): void {
    unpinSection();
    if (query !== '') closePage(false);
    const hits = matchRows(query);
    searching = hits !== null;
    page.classList.toggle('is-searching', hits !== null);
    const shows = (id: string): boolean => hits === null || hits.has(id);
    for (const node of rows.values()) {
      node.root.hidden = !shows(node.spec.id);
      const labelText = node.label.textContent ?? '';
      marked(node.label, labelText);
      if (node.desc) marked(node.desc, node.desc.textContent ?? '');
    }
    // A row on a sub-page is found through the row that opens it.
    for (const opener of openers.values()) {
      opener.root.hidden = !shows(opener.id);
      marked(opener.label, opener.label.textContent ?? '');
      if (opener.desc) marked(opener.desc, opener.desc.textContent ?? '');
    }
    paintGroups();
    mediaResults.hidden = hits !== null && !hits.has('media');
    const seen = (place: Place | undefined, id: string): boolean => place !== undefined && shows(id);
    for (const d of disclosures) {
      const any = [...places].some(([id, place]) => place.disclosure === d && seen(place, id));
      d.root.hidden = hits !== null && !any;
      paintDisclosure(d, hits !== null ? any : openState.has(d.key));
    }
    const shownIn = (test: (place: Place) => boolean): boolean => [...places].some(([id, place]) => test(place) && seen(place, id));
    for (const spec of AREAS) {
      for (const key of spec.parts) sectionNodes.get(key)!.hidden = hits !== null && !shownIn((place) => place.part === key);
      const any = hits === null || shownIn((place) => place.area === spec.key);
      areaNodes.get(spec.key)!.hidden = !any;
      navButtons.get(spec.key)?.classList.toggle('is-empty', !any);
    }
    laterNode.hidden = laterDisclosure.count === 0 || (hits !== null && !shownIn((place) => place.area === 'later'));
    const total = hits?.size ?? 0;
    empty.hidden = hits === null || total > 0;
    page.classList.toggle('is-nomatch', hits !== null && total === 0);
    emptyText.textContent = hits === null ? '' : resultCountText(0, query);
    count.textContent = hits === null ? '' : resultCountText(total, query);
    clear.hidden = query === '';
    main.scrollTop = 0;
    if (hits !== null) {
      const first = AREAS.find((spec) => !areaNodes.get(spec.key)?.hidden);
      if (first && first.key !== current) { current = first.key; paintNav(); }
    }
  }
  paintGroups();
  input.addEventListener('input', () => { query = input.value; filter(); });
  input.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && input.value !== '') { event.preventDefault(); input.value = ''; query = ''; filter(); return; }
    if (event.key !== 'Enter') return;
    event.preventDefault();
    // The first hit in page order: a row, or the row that opens the sub-page a hit is on.
    const first = Array.from(column.querySelectorAll<HTMLElement>('[data-row], [data-opens]')).find((node) => {
      if (node.hidden) return false;
      const id = node.dataset.row;
      return id === undefined || !places.get(id)?.page;
    });
    if (!first) return;
    const id = first.dataset.row ?? first.dataset.opens ?? '';
    const found = rows.get(id);
    const box = found && first.dataset.row !== undefined ? found.card : groups.find((g) => g.card.contains(first))?.card ?? first;
    const target = first.querySelector<HTMLElement>('button:not([disabled]), input:not([disabled])');
    box.classList.remove('is-flash');
    void box.offsetWidth;
    box.classList.add('is-flash');
    first.scrollIntoView({ block: 'center', behavior: reduceMotion() ? 'auto' : 'smooth' });
    target?.focus({ preventScroll: true });
    const spec = ROWS.find((r) => r.id === id);
    if (spec) cmd({ a: 'section', text: spec.section });
  });
  clear.addEventListener('click', () => { input.value = ''; query = ''; filter(); input.focus(); });
  // Ctrl+F and / jump to the search field, as on Chrome's settings page.
  document.addEventListener('keydown', (event) => {
    if (!store.root.hidden) return;
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
    if (message.t === 'state') { applyTheme(message.theme, message.solid, message.dark, message.fontScale, message.uiFont, message.density, message.compactMenus, message.look); return; }
    if (message.t === 'settingsgo') {
      store.hide();
      page.hidden = false;
      // Bowser moved the tab (a typed address, a menu pick): jump, as a navigation would.
      if (!snapshot) { wanted = message.s; return; }
      go(message.s, false, false);
      return;
    }
    if (message.t === 'snap' && message.p === 'privacy') { receiveDashboard(message); return; }
    if (message.t === 'snap' && message.p === 'protection') { protection.receive(message); return; }
    if (message.t === 'snap' && message.p === 'store') {
      const shown = store.receive(message);
      if (shown !== null) { page.hidden = shown; if (shown) unpinSection(); }
      return;
    }
    if (message.t === 'snap' && message.p === 'extensions') { extensions.receive(message); store.receive(message); return; }
    if (message.t === 'snap' && message.p === 'pagetools') { pageToolsSection.receive(message); return; } // --- page tools (T7) ---
    if (message.t === 'snap' && message.p === 'commands') {
      commandSections.receive(message);
      // --- toolbars and feeds (thread T13) --- the editor offers every command and chain, and names the command buttons.
      const known = parseCommandsSnapshot(message.d);
      if (known && snapshot) {
        editorCommands = [...known.commands.map((c) => ({ id: c.id, label: c.label, chain: c.chain }))];
        toolbarEditor.set({ address: itemsOf(draft?.toolbarButtons ?? snapshot.toolbarButtons), tabs: itemsOf(snapshot.bars.tabs), panel: itemsOf(snapshot.bars.panel), commands: editorCommands });
        toolbarEditor.disable(!snapshot.readable);
      } else if (known) {
        editorCommands = [...known.commands.map((c) => ({ id: c.id, label: c.label, chain: c.chain }))];
      }
      return;
    }
    if (message.t === 'snap' && message.p === 'address') { addressSections.receive(message); return; } // --- address bar, search and bookmarks ---
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
