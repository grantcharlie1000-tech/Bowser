// --- toolbars and feeds (thread T13) ---
// Pure rules for the three bars' button lists (src/toolbar_prefs.rs is the authority; Rust checks every
// list again and refuses what does not parse): ids, zones, limits, and the edits the toolbar editor makes.
// No DOM here; test/bars.mjs covers all of it.

import { ICON_NAMES } from './icons.gen';

export type BarId = 'address' | 'tabs' | 'panel';
export const BAR_IDS: readonly BarId[] = ['address', 'tabs', 'panel'];
/** The settings.txt key of each bar's list. */
export const BAR_KEYS: Readonly<Record<BarId, string>> = { address: 'toolbar_buttons', tabs: 'toolbar_tabs', panel: 'toolbar_panel' };
export const BAR_TITLES: Readonly<Record<BarId, string>> = { address: 'Address bar', tabs: 'Tab bar', panel: 'Panel bar' };

export const ADDRESS_ITEMS = ['back', 'forward', 'reload', 'site-info', 'split', 'reader', 'bookmark', 'newtab', 'downloads', 'extensions'] as const;
export const DEFAULT_ADDRESS: readonly string[] = ['back', 'forward', 'reload', 'site-info', 'split', 'reader', 'extensions', 'downloads'];
export const TAB_ITEMS = ['closed-tabs'] as const;
export const PANEL_ITEMS = ['feeds'] as const;
export const FIXED_ITEMS: Readonly<Record<BarId, readonly string[]>> = { address: ADDRESS_ITEMS, tabs: TAB_ITEMS, panel: PANEL_ITEMS };
export const DEFAULTS: Readonly<Record<BarId, readonly string[]>> = { address: DEFAULT_ADDRESS, tabs: TAB_ITEMS, panel: [] };

/** Most items in a bar, most command buttons in a bar, and most buttons sharing the end of the address bar with the menu. */
export const MAX_ITEMS = 14;
export const MAX_COMMAND_ITEMS = 4;
export const MAX_RIGHT_STATIC = 5;
/** CSS px one always-there button at the end of the address bar takes (a 28 px button and a 4 px gap). */
export const SLOT_PX = 32;

export type Zone = 'left' | 'pill' | 'right';

const CMD_RE = /^cmd:[a-z0-9-]{1,32}$/;
const CHAIN_RE = /^chain:[1-9][0-9]{0,3}$/;

/** `cmd:<command id>` or `chain:<n>` (whether the command exists is Rust's to say). */
export function isCommandItem(id: string): boolean {
  return CMD_RE.test(id) || CHAIN_RE.test(id);
}

/** The ids of a comma list, `''` meaning none. */
export function itemsOf(text: string): string[] {
  return text === '' ? [] : text.split(',').filter((id) => id !== '');
}

export function zoneOf(id: string): Zone {
  if (id === 'back' || id === 'forward' || id === 'reload') return 'left';
  if (id === 'site-info' || id === 'split' || id === 'reader') return 'pill';
  return 'right';
}

/** Items at the end of the address bar that take room whether or not there is something to show. */
export function rightStatic(items: readonly string[]): number {
  return items.filter((id) => zoneOf(id) === 'right' && id !== 'downloads' && id !== 'extensions').length;
}

/** Whether `items` is a list Rust will accept for `bar` (same grammar and limits, command ids by shape only). */
export function validItems(bar: BarId, items: readonly string[]): boolean {
  if (items.length > MAX_ITEMS || new Set(items).size !== items.length) return false;
  if (!items.every((id) => FIXED_ITEMS[bar].includes(id) || isCommandItem(id))) return false;
  if (items.filter(isCommandItem).length > MAX_COMMAND_ITEMS) return false;
  return bar !== 'address' || rightStatic(items) <= MAX_RIGHT_STATIC;
}

/** The list as the comma text that is saved. */
export function itemsText(items: readonly string[]): string {
  return items.join(',');
}

export function isDefault(bar: BarId, items: readonly string[]): boolean {
  const defaults = DEFAULTS[bar];
  return items.length === defaults.length && items.every((id, at) => id === defaults[at]);
}

/** The fixed buttons of `bar` that are not in `items` yet, in the order the editor offers them. */
export function addable(bar: BarId, items: readonly string[]): string[] {
  return FIXED_ITEMS[bar].filter((id) => !items.includes(id));
}

/** Why `id` cannot be added to `items` right now, or `null`. */
export function whyNotAdd(bar: BarId, items: readonly string[], id: string): string | null {
  if (items.includes(id)) return 'It is already on this bar.';
  if (items.length >= MAX_ITEMS) return `A bar holds at most ${MAX_ITEMS} buttons.`;
  if (isCommandItem(id) && items.filter(isCommandItem).length >= MAX_COMMAND_ITEMS) return `A bar holds at most ${MAX_COMMAND_ITEMS} command buttons.`;
  if (bar === 'address' && zoneOf(id) === 'right' && id !== 'downloads' && id !== 'extensions' && rightStatic(items) >= MAX_RIGHT_STATIC) {
    return `The end of the address bar holds at most ${MAX_RIGHT_STATIC} buttons.`;
  }
  return null;
}

/** `items` with `id` added at the end of its place (a new button joins the end of its zone), or `null` when not allowed. */
export function addItem(bar: BarId, items: readonly string[], id: string): string[] | null {
  if (whyNotAdd(bar, items, id) !== null) return null;
  if (bar !== 'address') return [...items, id];
  // The list is kept in zone order, so the editor's groups and the saved list read the same.
  const zone = zoneOf(id);
  const order: Zone[] = ['left', 'pill', 'right'];
  let at = items.length;
  for (let i = items.length - 1; i >= 0; i -= 1) {
    if (order.indexOf(zoneOf(items[i] ?? '')) <= order.indexOf(zone)) break;
    at = i;
  }
  return [...items.slice(0, at), id, ...items.slice(at)];
}

export function removeItem(items: readonly string[], id: string): string[] {
  return items.filter((other) => other !== id);
}

/**
 * `items` with `id` moved one place up or down among the buttons it can swap with: on the address bar the
 * buttons of its own zone, on the other bars all of them. `null` at the edge.
 */
export function moveItem(bar: BarId, items: readonly string[], id: string, dir: -1 | 1): string[] | null {
  const at = items.indexOf(id);
  if (at < 0) return null;
  const peers = (other: string): boolean => bar !== 'address' || zoneOf(other) === zoneOf(id);
  let to = at + dir;
  while (to >= 0 && to < items.length && !peers(items[to] ?? '')) to += dir;
  if (to < 0 || to >= items.length) return null;
  const next = [...items];
  next[at] = items[to] as string;
  next[to] = id;
  return next;
}

/** Whether `id` can move up or down (for the editor's arrows). */
export function canMove(bar: BarId, items: readonly string[], id: string, dir: -1 | 1): boolean {
  return moveItem(bar, items, id, dir) !== null;
}

/** The address bar's items by zone, each in list order. */
export function addressZones(items: readonly string[]): Record<Zone, string[]> {
  const zones: Record<Zone, string[]> = { left: [], pill: [], right: [] };
  for (const id of items) zones[zoneOf(id)].push(id);
  return zones;
}

/** What each fixed item is called, and the note under it in the editor. */
export const ITEM_LABELS: Readonly<Record<string, { readonly label: string; readonly hint: string }>> = {
  back: { label: 'Back', hint: '' },
  forward: { label: 'Forward', hint: '' },
  reload: { label: 'Reload and stop', hint: '' },
  'site-info': { label: 'Site information', hint: 'Inside the address field, on its left.' },
  split: { label: 'Split view', hint: 'Inside the address field, on its right.' },
  reader: { label: 'Reader view', hint: 'Shown on pages that can be read as an article.' },
  bookmark: { label: 'Bookmark this page', hint: '' },
  newtab: { label: 'New tab', hint: '' },
  downloads: { label: 'Downloads', hint: 'Shown once a download has started.' },
  extensions: { label: 'Extensions', hint: 'Shown when an installed extension has a popup.' },
  'closed-tabs': { label: 'Closed tabs', hint: 'Shown once a tab has been closed.' },
  feeds: { label: 'Feeds', hint: '' },
};

/** The bars the editor offers. The panel bar was the left rail's, and Bowser no longer has a rail, so its buttons could never show. */
export const EDITABLE_BARS: readonly BarId[] = ['address', 'tabs'];

/** The tooltip and name of a command button when Rust has not sent one (a command that no longer exists). */
export function itemLabel(id: string, names: ReadonlyMap<string, string>): string {
  return ITEM_LABELS[id]?.label ?? names.get(id) ?? (id.startsWith('chain:') ? `Chain ${id.slice(6)}` : id.replace(/^cmd:/, '').replace(/-/g, ' '));
}

/** The glyph each fixed item shows in the editor (icon names of the one family). */
export const ITEM_ICONS: Readonly<Record<string, string>> = {
  back: 'back', forward: 'forward', reload: 'reload', 'site-info': 'info', split: 'split', reader: 'reader', bookmark: 'bookmark',
  newtab: 'plus', downloads: 'download', extensions: 'puzzle', 'closed-tabs': 'tab-restore', feeds: 'feed',
};

/**
 * The glyph of every built-in command a bar can carry (`cmd:<id>`), by command id (src/commands/registry.rs),
 * so a command button looks like what it does on the toolbar, the tab strip, the sidebar, the rail and in the
 * editor. test/bars.mjs fails when a command offered as a button has no entry here. A capture's copy and note
 * variants share their capture's glyph: the same picture, sent somewhere else.
 */
export const COMMAND_ICONS: Readonly<Record<string, string>> = {
  'new-tab': 'new-tab', 'new-private-tab': 'private', 'close-tab': 'tab-close', 'close-private-tabs': 'glasses',
  settings: 'gear', 'toggle-notes': 'notes', profiles: 'person',
  'save-session': 'save', sessions: 'folder', 'save-all-session': 'archive', 'open-session': 'folder-open',
  'add-bookmark': 'bookmark', 'bookmarks-panel': 'heart',
  reload: 'reload', 'hard-reload': 'reload-hard', stop: 'close-others', back: 'back', forward: 'forward',
  extensions: 'puzzle', reader: 'reader', calm: 'droplet', translate: 'translate',
  'next-tab': 'tab-switch', 'prev-tab': 'swap', 'reopen-tab': 'tab-restore', 'new-window': 'window', 'copy-link': 'link',
  'toggle-sidebar': 'sidebar', 'toggle-fullscreen': 'caption-maximize', 'focus-address-bar': 'search', 'fill-password': 'password-fill',
  'privacy-dashboard': 'shield', 'toggle-blocker': 'block', 'cycle-blocking-level': 'customize', 'delete-browsing-data': 'trash',
  'undo-tab-organisation': 'reset', 'picture-in-picture': 'pip',
  'toggle-split': 'split', 'focus-split-left': 'split-left', 'focus-split-right': 'split-right', 'close-split-pane': 'unsplit',
  'focus-split-up': 'split-top', 'focus-split-down': 'split-bottom',
  'tile-vertical': 'split', 'tile-horizontal': 'split-stacked', 'tile-grid': 'grid', untile: 'caption-restore',
  'command-palette': 'quick-commands', history: 'clock', downloads: 'download',
  'zoom-in': 'zoom-in', 'zoom-out': 'zoom-out', 'zoom-reset': 'zoom-reset',
  devtools: 'workspace-code', 'shortcuts-cheat-sheet': 'file-text', 'toggle-site-shortcuts': 'globe', 'keyboard-settings': 'keyboard',
  'closed-tabs': 'tab-restore', 'periodic-reload': 'timer', 'stop-all-reloads': 'pause',
  'qr-code': 'qr-code', 'toggle-address-bar': 'address-bar', 'toggle-auto-hide': 'eye',
  'capture-visible': 'capture', 'capture-full': 'capture-full', 'capture-selection': 'capture-selection',
  'capture-visible-copy': 'capture', 'capture-full-copy': 'capture-full', 'capture-selection-copy': 'capture-selection',
  'capture-visible-note': 'capture', 'capture-full-note': 'capture-full', 'capture-selection-note': 'capture-selection',
  'note-from-selection': 'note-add', 'note-append-selection': 'edit',
  'stack-site-tabs': 'tab-group', 'stack-tabs-by-site': 'workspace', unstack: 'unstack',
  'next-workspace': 'chevron-right', 'previous-workspace': 'chevron-left',
  feeds: 'feed', 'subscribe-feed': 'bell',
};

/**
 * The glyph of any bar item: the one Rust named for it (`glyph`, when it is an icon of the family), else a fixed
 * button's own, a command's, `chain` for a command chain, `command` for a command Bowser no longer has.
 */
export function itemIcon(id: string, glyph?: string): string {
  if (glyph !== undefined && (ICON_NAMES as readonly string[]).includes(glyph)) return glyph;
  if (id.startsWith('chain:')) return 'chain';
  if (id.startsWith('cmd:')) return COMMAND_ICONS[id.slice(4)] ?? 'command';
  return ITEM_ICONS[id] ?? 'command';
}
// --- end toolbars and feeds ---
