// Command palette model: payload validation and fuzzy ranking. Pure (no DOM), tested in
// test/palette.mjs. Rust owns the item list and the actions behind it; this file only decides
// which rows show for a query. The scoring mirrors productivity::rank_items in Rust.

export type PaletteSection = 'tab' | 'command' | 'bookmark' | 'history';

export interface PaletteItem {
  readonly id: number;
  readonly s: PaletteSection;
  readonly label: string;
  readonly detail: string;
  readonly acc?: string;
}

export interface PaletteMessage {
  readonly t: 'palette';
  readonly tok: number;
  readonly kind: string;
  readonly placeholder: string;
  readonly items: readonly PaletteItem[];
}

export interface PaletteGroup {
  readonly section: PaletteSection;
  readonly label: string;
  readonly items: readonly PaletteItem[];
}

export const SECTIONS: readonly PaletteSection[] = ['tab', 'command', 'bookmark', 'history'];
export const SECTION_LABEL: Readonly<Record<PaletteSection, string>> = {
  tab: 'Open tabs',
  command: 'Commands',
  bookmark: 'Bookmarks',
  history: 'History',
};

/** Caps that match the Rust builder (command_palette.rs). */
export const MAX_PALETTE_ITEMS = 1024;
export const MAX_PALETTE_LABEL = 128;
export const MAX_PALETTE_DETAIL = 256;
export const MAX_PALETTE_ACCEL = 32;
export const MAX_QUERY = 256;
/** Rows shown for a typed query, and per section with an empty query. */
export const MAX_RESULTS = 12;
export const EMPTY_QUERY_LIMIT: Readonly<Record<PaletteSection, number>> = { tab: 5, command: 8, bookmark: 0, history: 0 };

const MAX_ID = 0xffffffff;
const ITEM_KEYS = ['id', 's', 'label', 'detail', 'acc'];

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
  if (!Object.keys(raw).every(key => ['v', 't', 'tok', 'kind', 'placeholder', 'items'].includes(key))) return false;
  if (!isInt(raw.tok, 0, MAX_ID) || !isStr(raw.kind, 32) || raw.kind === '' || !isStr(raw.placeholder, MAX_PALETTE_LABEL)) return false;
  if (!Array.isArray(raw.items) || raw.items.length > MAX_PALETTE_ITEMS) return false;
  const seen = new Set<number>();
  for (const item of raw.items as unknown[]) {
    if (!isRecord(item) || !Object.keys(item).every(key => ITEM_KEYS.includes(key))) return false;
    if (!isInt(item.id, 0, MAX_ID) || seen.has(item.id)) return false;
    seen.add(item.id);
    if (typeof item.s !== 'string' || !(SECTIONS as readonly string[]).includes(item.s)) return false;
    if (!isStr(item.label, MAX_PALETTE_LABEL) || !isStr(item.detail, MAX_PALETTE_DETAIL)) return false;
    if (item.acc !== undefined && !isStr(item.acc, MAX_PALETTE_ACCEL)) return false;
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

/**
 * The rows for `query`, grouped by section. Empty query: the first open tabs and commands. Typed
 * query: the best `MAX_RESULTS` matches (stable on ties by label), each section placed where its
 * best match ranks.
 */
export function filterPalette(items: readonly PaletteItem[], query: string): PaletteGroup[] {
  const needle = query.trim().toLocaleLowerCase().slice(0, MAX_QUERY);
  if (needle === '') {
    return SECTIONS.map(section => ({
      section,
      label: SECTION_LABEL[section],
      items: items.filter(item => item.s === section).slice(0, EMPTY_QUERY_LIMIT[section]),
    })).filter(group => group.items.length > 0);
  }
  const ranked = items
    .map((item, index) => ({ item, index, score: scoreItem(item, needle) }))
    .filter(entry => entry.score > 0)
    .sort((a, b) => b.score - a.score
      || a.item.label.toLocaleLowerCase().localeCompare(b.item.label.toLocaleLowerCase())
      || a.index - b.index)
    .slice(0, MAX_RESULTS);
  const groups: { section: PaletteSection; label: string; items: PaletteItem[] }[] = [];
  for (const { item } of ranked) {
    let group = groups.find(g => g.section === item.s);
    if (!group) {
      group = { section: item.s, label: SECTION_LABEL[item.s], items: [] };
      groups.push(group);
    }
    group.items.push(item);
  }
  return groups;
}

/** Rows in display order, for keyboard movement. */
export function flatten(groups: readonly PaletteGroup[]): PaletteItem[] {
  return groups.flatMap(group => group.items);
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
