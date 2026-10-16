// Command palette model: payload validation, filter prefixes, the calculator row, ranking and
// grouping. Pure (no DOM), tested in test/popup-pages.mjs and test/palette.mjs. Rust owns the item
// list and the actions behind it; this file only decides which rows show for a query. The scoring
// mirrors commands::palette in Rust (the native fallback), checked by test/fixtures/palette-cases.json.
import { resultText } from './calc';

/** What a row Rust sends can be. */
export type PaletteSection = 'tab' | 'closed' | 'workspace' | 'command' | 'action' | 'bookmark' | 'note' | 'history';
/** What the user can show or hide: the row types plus the calculator, whose row the page builds. */
export type ResultType = PaletteSection | 'calc';
/** `palette` is Quick Commands; `shortcuts` is the Ctrl+F1 cheat sheet (grouped, not pickable). */
export type PaletteMode = 'palette' | 'shortcuts';

export interface PaletteItem {
  readonly id: number;
  readonly s: ResultType;
  readonly label: string;
  readonly detail: string;
  readonly acc?: string;
  /** Cached favicon (PNG data: URI) for tab, closed, bookmark and history rows; absent when none is cached. */
  readonly fav?: string;
  /** The cheat sheet's group heading. */
  readonly g?: string;
  /** A bookmark row's nickname (T2's bookmark dialog). */
  readonly k?: string;
}

export interface PaletteMessage {
  readonly t: 'palette';
  readonly tok: number;
  readonly kind: string;
  readonly placeholder: string;
  readonly mode: PaletteMode;
  /** The result types to show, in the user's order. */
  readonly types: readonly ResultType[];
  /** The order is the default: results rank by match quality and the best match leads. */
  readonly ranked: boolean;
  /** Rows shown for a typed search. */
  readonly limit: number;
  /** Keep the last typed text when the palette opens again. */
  readonly keep: boolean;
  /** Typing a bookmark's nickname opens it at once (`palette_open_nickname`). */
  readonly autonick: boolean;
  readonly items: readonly PaletteItem[];
}

export interface PaletteGroup {
  readonly section: ResultType;
  readonly label: string;
  readonly items: readonly PaletteItem[];
}

/** The default order of the result types (src/commands/store.rs `ResultType::DEFAULT_ORDER`). */
export const DEFAULT_TYPES: readonly ResultType[] = ['tab', 'closed', 'workspace', 'command', 'action', 'bookmark', 'note', 'history', 'calc'];
export const SECTIONS: readonly PaletteSection[] = ['tab', 'closed', 'workspace', 'command', 'action', 'bookmark', 'note', 'history'];
export const SECTION_LABEL: Readonly<Record<ResultType, string>> = {
  tab: 'Open tabs',
  closed: 'Recently closed',
  workspace: 'Workspaces',
  command: 'Commands',
  action: 'Page actions',
  bookmark: 'Bookmarks',
  note: 'Notes',
  history: 'History',
  calc: 'Calculator',
};

/** Caps that match the Rust builder (command_palette.rs). */
export const MAX_PALETTE_ITEMS = 1024;
export const MAX_PALETTE_LABEL = 128;
export const MAX_PALETTE_DETAIL = 256;
export const MAX_PALETTE_ACCEL = 96;
export const MAX_PALETTE_GROUP = 64;
/** command_palette.rs HTML_MAX_NICK. */
export const MAX_PALETTE_NICK = 24;
export const MAX_QUERY = 256;
/** Rows shown for a typed query unless the setting says otherwise (src/commands/store.rs DEFAULT_RESULTS). */
export const MAX_RESULTS = 12;
export const MIN_LIMIT = 3;
export const MAX_LIMIT = 30;
/** Rows per type for an empty query (commands::palette::empty_limit). */
export const EMPTY_QUERY_LIMIT: Readonly<Record<ResultType, number>> = {
  tab: 5, closed: 3, workspace: 3, command: 8, action: 0, bookmark: 0, note: 0, history: 0, calc: 0,
};

const MAX_ID = 0xffffffff;
const ITEM_KEYS = ['id', 's', 'label', 'detail', 'acc', 'fav', 'g', 'k'];
const MESSAGE_KEYS = ['v', 't', 'tok', 'kind', 'placeholder', 'items', 'mode', 'types', 'ranked', 'limit', 'keep', 'autonick'];
/** Same gate as the other favicon fields (protocol.ts MAX_FAV_URI): a small PNG data: URI. */
const FAV_PREFIX = 'data:image/png;base64,';
const MAX_FAV = 24 * 1024;

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}
function isStr(v: unknown, max: number): v is string {
  return typeof v === 'string' && v.length <= max;
}
function isInt(v: unknown, min: number, max: number): v is number {
  return typeof v === 'number' && Number.isInteger(v) && v >= min && v <= max;
}

/** Strict check of a `palette` message: known keys only, capped strings, unique ids. */
export function validPalettePayload(raw: unknown): raw is PaletteMessage {
  if (!isRecord(raw) || raw.t !== 'palette' || raw.v !== 1) return false;
  if (!Object.keys(raw).every(key => MESSAGE_KEYS.includes(key))) return false;
  if (!isInt(raw.tok, 0, MAX_ID) || !isStr(raw.kind, 32) || raw.kind === '' || !isStr(raw.placeholder, MAX_PALETTE_LABEL)) return false;
  if (raw.mode !== 'palette' && raw.mode !== 'shortcuts') return false;
  if (typeof raw.ranked !== 'boolean' || typeof raw.keep !== 'boolean' || typeof raw.autonick !== 'boolean' || !isInt(raw.limit, 0, MAX_LIMIT)) return false;
  if (!Array.isArray(raw.types) || raw.types.length > DEFAULT_TYPES.length) return false;
  if (new Set(raw.types as unknown[]).size !== raw.types.length || !(raw.types as unknown[]).every(type => typeof type === 'string' && (DEFAULT_TYPES as readonly string[]).includes(type))) return false;
  if (!Array.isArray(raw.items) || raw.items.length > MAX_PALETTE_ITEMS) return false;
  const seen = new Set<number>();
  for (const item of raw.items as unknown[]) {
    if (!isRecord(item) || !Object.keys(item).every(key => ITEM_KEYS.includes(key))) return false;
    if (!isInt(item.id, 0, MAX_ID) || seen.has(item.id)) return false;
    seen.add(item.id);
    // The calculator row is the page's own: Rust never sends one.
    if (typeof item.s !== 'string' || !(SECTIONS as readonly string[]).includes(item.s)) return false;
    if (!isStr(item.label, MAX_PALETTE_LABEL) || !isStr(item.detail, MAX_PALETTE_DETAIL)) return false;
    if (item.acc !== undefined && !isStr(item.acc, MAX_PALETTE_ACCEL)) return false;
    if (item.g !== undefined && !isStr(item.g, MAX_PALETTE_GROUP)) return false;
    // Only a bookmark has a nickname, and an empty one is never sent.
    if (item.k !== undefined && !(item.s === 'bookmark' && isStr(item.k, MAX_PALETTE_NICK) && item.k.trim() !== '')) return false;
    if (item.fav !== undefined && !(isStr(item.fav, MAX_FAV) && item.fav.startsWith(FAV_PREFIX) && /^[A-Za-z0-9+/=]*$/.test(item.fav.slice(FAV_PREFIX.length)))) return false;
  }
  return true;
}

/** Span between the first and last matched character of `needle` in `hay`, in order; -1 if absent. */
export function subsequenceGap(needle: string, hay: string): number {
  let from = 0;
  let first = -1;
  let last = 0;
  for (const ch of needle) {
    const at = hay.indexOf(ch, from);
    if (at < 0) return -1;
    if (first < 0) first = at;
    last = at;
    from = at + ch.length;
  }
  return first < 0 ? -1 : last - first;
}

/**
 * Score of `item` for a lower-cased, trimmed `needle` (> 0 is a match). Exact label, label prefix,
 * word prefix, detail prefix, then ordered fuzzy matches in the label and the detail.
 */
export function scoreItem(item: PaletteItem, needle: string): number {
  if (needle === '') return 0;
  // A bookmark's nickname typed whole beats every other match (T2's nicknames).
  if (item.k !== undefined && item.k.toLocaleLowerCase() === needle) return 3500;
  const label = item.label.toLocaleLowerCase();
  const detail = item.detail.toLocaleLowerCase();
  if (label === needle) return 3000;
  if (label.startsWith(needle)) return 2000 - Math.min(label.length, 500);
  if (label.split(/\s+/).some(word => word.startsWith(needle))) return 1500 - Math.min(label.length, 500);
  if (detail.startsWith(needle)) return 1000 - Math.min(detail.length, 500);
  const inLabel = subsequenceGap(needle, label);
  if (inLabel >= 0) return 500 - Math.min(inLabel, 400);
  const inDetail = subsequenceGap(needle, detail);
  if (inDetail >= 0) return 250 - Math.min(inDetail, 200);
  return -1;
}

/** Filter prefixes and their short forms (`bookmark:` / `bkm:`, `tab:` / `tb:`, `command:` / `cmd:`, ...). */
const PREFIXES: Readonly<Record<string, readonly ResultType[]>> = {
  bookmark: ['bookmark'], bkm: ['bookmark'],
  note: ['note'],
  tab: ['tab', 'closed'], tb: ['tab', 'closed'],
  history: ['history'],
  command: ['command'], cmd: ['command'],
  action: ['action'],
  workspace: ['workspace'],
};

/** The types a leading `word:` names and the text after it; `kinds` is null for text with no known prefix. */
export function parsePrefix(query: string): { readonly kinds: readonly ResultType[] | null; readonly text: string } {
  const trimmed = query.trimStart();
  const colon = trimmed.indexOf(':');
  if (colon > 0) {
    const kinds = Object.hasOwn(PREFIXES, trimmed.slice(0, colon).toLowerCase()) ? PREFIXES[trimmed.slice(0, colon).toLowerCase()]! : null;
    if (kinds) return { kinds, text: trimmed.slice(colon + 1).trimStart() };
  }
  return { kinds: null, text: query };
}

export interface FilterOptions {
  /** The shown types in the user's order. */
  readonly types: readonly ResultType[];
  readonly ranked: boolean;
  readonly limit: number;
}
export const DEFAULT_FILTER: FilterOptions = { types: DEFAULT_TYPES, ranked: true, limit: MAX_RESULTS };

/** Id of the calculator row, which Rust never sends: picking it copies its label. */
export const CALC_ID = -1;

/**
 * The rows for `query`, grouped. Empty text: the first rows of each shown type (more with a prefix).
 * Typed text: the best `limit` matches, grouped by type (where the best match of each group ranks
 * while the order is the default, else in the user's order), with a calculator row first when the
 * text is a sum. A prefix limits the types; a hidden type never shows, prefix or not.
 */
export function filterPalette(items: readonly PaletteItem[], query: string, options: FilterOptions = DEFAULT_FILTER): PaletteGroup[] {
  const { kinds, text } = parsePrefix(query.slice(0, MAX_QUERY));
  const needle = text.trim().toLocaleLowerCase();
  const allowed = (kind: ResultType): boolean => options.types.includes(kind) && (kinds === null || kinds.includes(kind));
  if (needle === '') {
    return options.types
      .filter(allowed)
      .map(section => ({
        section,
        label: SECTION_LABEL[section],
        items: items.filter(item => item.s === section).slice(0, kinds ? options.limit : EMPTY_QUERY_LIMIT[section]),
      }))
      .filter(group => group.items.length > 0);
  }
  const ranked = items
    .map((item, index) => ({ item, index, score: allowed(item.s) ? scoreItem(item, needle) : -1 }))
    .filter(entry => entry.score > 0)
    .sort((a, b) => b.score - a.score
      || a.item.label.toLocaleLowerCase().localeCompare(b.item.label.toLocaleLowerCase())
      || a.index - b.index)
    .slice(0, options.limit);
  const order: ResultType[] = options.ranked
    ? ranked.map(entry => entry.item.s).filter((kind, at, all) => all.indexOf(kind) === at)
    : options.types.filter(kind => ranked.some(entry => entry.item.s === kind));
  const groups: PaletteGroup[] = [];
  const result = kinds === null && options.types.includes('calc') ? resultText(text) : null;
  if (result !== null) {
    groups.push({
      section: 'calc',
      label: SECTION_LABEL.calc,
      items: [{ id: CALC_ID, s: 'calc', label: result, detail: `${text.trim()} = ${result}. Press Enter to copy.` }],
    });
  }
  for (const section of order) {
    groups.push({ section, label: SECTION_LABEL[section], items: ranked.filter(entry => entry.item.s === section).map(entry => entry.item) });
  }
  return groups;
}

/**
 * The cheat sheet's rows for `query`, grouped by their `g` heading in the order Rust sent them. Every
 * word of the query must appear in a row's label, group, description or shortcuts.
 */
export function filterShortcuts(items: readonly PaletteItem[], query: string): PaletteGroup[] {
  const words = query.trim().toLocaleLowerCase().slice(0, MAX_QUERY).split(/\s+/).filter(Boolean);
  const groups: { section: ResultType; label: string; items: PaletteItem[] }[] = [];
  for (const item of items) {
    const hay = `${item.label} ${item.g ?? ''} ${item.detail} ${item.acc ?? ''}`.toLocaleLowerCase();
    if (!words.every(word => hay.includes(word))) continue;
    const label = item.g ?? SECTION_LABEL[item.s];
    let group = groups.find(g => g.label === label);
    if (!group) {
      group = { section: item.s, label, items: [] };
      groups.push(group);
    }
    group.items.push(item);
  }
  return groups;
}

/**
 * The bookmark whose nickname is the whole typed text (ignoring case, and after a `bookmark:` /
 * `bkm:` prefix), when "open bookmark automatically on nickname match" is on and bookmarks are a
 * shown type. Any other prefix, or a nickname two bookmarks share, opens nothing.
 */
export function nicknameMatch(items: readonly PaletteItem[], query: string, autonick: boolean, types: readonly ResultType[]): PaletteItem | null {
  if (!autonick || !types.includes('bookmark')) return null;
  const { kinds, text } = parsePrefix(query.slice(0, MAX_QUERY));
  if (kinds !== null && !kinds.includes('bookmark')) return null;
  const needle = text.trim().toLocaleLowerCase();
  if (needle === '') return null;
  const hits = items.filter(item => item.s === 'bookmark' && item.k !== undefined && item.k.toLocaleLowerCase() === needle);
  return hits.length === 1 ? hits[0]! : null;
}

/** Rows in display order, for keyboard movement. */
export function flatten(groups: readonly PaletteGroup[]): PaletteItem[] {
  return groups.flatMap(group => group.items);
}

/**
 * `typed` completed with the rest of `label` when `label` starts with the text after any filter prefix
 * (ignoring case) and is longer; the typed prefix and spacing stay. `null` when there is nothing to add.
 */
export function completion(typed: string, label: string): string | null {
  const { kinds, text } = parsePrefix(typed);
  const start = kinds ? typed.length - text.length : 0;
  const tail = typed.slice(start);
  if (tail === '' || label.length <= tail.length) return null;
  if (label.slice(0, tail.length).toLocaleLowerCase() !== tail.toLocaleLowerCase()) return null;
  return typed.slice(0, start) + label;
}

/** Shown for an untitled tab. */
export const UNTITLED_TAB = 'New tab';

/** The row's label: an untitled tab reads "New tab", never a blank row. */
export function rowLabel(item: PaletteItem): string {
  return item.label.trim() === '' ? (item.s === 'tab' ? UNTITLED_TAB : item.detail) : item.label;
}

/** Glyph (an icons.ts name) for each built-in command, by its label; any other command gets the command glyph. */
const COMMAND_GLYPH: Readonly<Record<string, string>> = {
  'new tab': 'new-tab',
  'close tab': 'close',
  'settings': 'gear',
  'toggle notes and tasks': 'notes',
  'profiles': 'person',
  'save session': 'folder',
  'saved sessions': 'tab-switch',
  'add bookmark': 'bookmark',
  'reload': 'reload',
  'back': 'back',
  'forward': 'forward',
  'extensions': 'puzzle',
  'history': 'clock',
  'downloads': 'download',
  'zoom in': 'zoom-in',
  'zoom out': 'zoom-out',
  'reader view': 'reader',
  'translate page': 'translate',
  'keyboard shortcuts': 'command',
  // Wave-1 commands (T1 tabs, T2 address bar, T3 privacy), all from the existing icon set.
  'reopen closed tab': 'reset',
  'closed tabs': 'clock',
  'save all tabs as session': 'save',
  'open saved session': 'folder-open',
  'reload this tab every…': 'reload',
  'stop reloading all tabs': 'reload',
  'delete browsing data': 'trash',
  'next blocking level for this site': 'shield',
  'qr code for this page': 'qr-code',
  'show or hide the address bar': 'search',
};

export function commandGlyph(label: string): string {
  const key = label.trim().toLocaleLowerCase();
  // The secrets command is matched by prefix: chrome sources never spell out the store's name (guard.rs).
  if (key.startsWith('pass')) return 'key';
  return COMMAND_GLYPH[key] ?? 'command';
}

/** Next selection after a key; `count` rows, wrapping for single steps, clamping for pages. */
export function moveSelection(selected: number, count: number, key: string): number {
  if (count === 0) return -1;
  switch (key) {
    case 'ArrowDown': return (selected + 1 + count) % count;
    case 'ArrowUp': return selected <= 0 ? count - 1 : selected - 1;
    case 'PageDown': return Math.min(count - 1, Math.max(selected, 0) + 5);
    case 'PageUp': return Math.max(0, selected - 5);
    default: return selected;
  }
}
