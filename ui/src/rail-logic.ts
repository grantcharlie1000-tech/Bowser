// Pure left-rail logic: key grammar, icon names, section split, drop slot and favicon cache.
// No DOM and no runtime imports (the type import is erased), so test/rail.mjs can load it directly.
import type { RailItem } from './protocol';

/** Most items in one `rail` message. */
export const MAX_RAIL_ITEMS = 128;
/** Longest rail key (`ws:` plus 20 digits). */
export const MAX_RAIL_KEY = 24;
/** Most favicons kept at once; equal to the item cap, so a full rail always fits. */
export const RAIL_FAV_MAX = 128;

export const RAIL_KEY_RE = /^(home|customize|addws|addapp|bookmarks|history|menu|ws:[0-9]{1,20}|app:[0-9]{1,10})$/;

export const RAIL_ICON_NAMES = ['home', 'customize', 'plus', 'heart', 'clock', 'dots', 'apptile', 'letter'] as const;
export const RAIL_GLYPHS = [
  'briefcase',
  'home',
  'code',
  'book',
  'cart',
  'game',
  'music',
  'heart',
  'star',
  'bolt',
  'globe',
  'leaf',
] as const;
export type RailGlyph = (typeof RAIL_GLYPHS)[number];
export type RailIconName = (typeof RAIL_ICON_NAMES)[number] | `g:${RailGlyph}`;

/** The two add buttons are told apart by key (Rust sends `plus` for both): own glyph and own name. */
export const ADD_BUTTONS: Readonly<Record<string, { readonly icon: 'workspace-add' | 'app-tile'; readonly label: string }>> = {
  addws: { icon: 'workspace-add', label: 'New workspace' },
  addapp: { icon: 'app-tile', label: 'Add web app' },
};

/** Where a draggable moves with Alt+Up / Alt+Down: the key to drop before ('' for the end), or null at the edge. */
export function keyboardMoveBefore(sectionKeys: readonly string[], key: string, dir: -1 | 1): string | null {
  const at = sectionKeys.indexOf(key);
  if (at < 0) return null;
  if (dir < 0) return at === 0 ? null : (sectionKeys[at - 1] ?? null);
  if (at >= sectionKeys.length - 1) return null;
  return sectionKeys[at + 2] ?? '';
}

export type Section = 'top' | 'workspaces' | 'apps' | 'bottom';

export function isRailKey(v: string): boolean {
  return v.length <= MAX_RAIL_KEY && RAIL_KEY_RE.test(v);
}

export function isRailIcon(v: unknown): v is RailIconName {
  if (typeof v !== 'string') return false;
  if ((RAIL_ICON_NAMES as readonly string[]).includes(v)) return true;
  return v.startsWith('g:') && (RAIL_GLYPHS as readonly string[]).includes(v.slice(2));
}

/** The section a key belongs to, or null for a key outside the grammar. */
export function sectionOf(key: string): Section | null {
  if (key === 'home' || key === 'customize') return 'top';
  if (key === 'addws' || key.startsWith('ws:')) return 'workspaces';
  if (key === 'addapp' || key.startsWith('app:')) return 'apps';
  if (key === 'bookmarks' || key === 'history' || key === 'menu') return 'bottom';
  return null;
}

/** Only workspace and app buttons can be reordered, and only within their own section. */
export function isDraggable(key: string): boolean {
  return key.startsWith('ws:') || key.startsWith('app:');
}

/** Splits the flat list into sections, keeping the message order inside each one. */
export function splitSections(items: readonly RailItem[]): Record<Section, RailItem[]> {
  const out: Record<Section, RailItem[]> = { top: [], workspaces: [], apps: [], bottom: [] };
  for (const item of items) {
    const section = sectionOf(item.k);
    if (section !== null) out[section].push(item);
  }
  return out;
}

/**
 * Drop slot for a vertical drag. `centres` holds every draggable's centre in section order, `from`
 * is the dragged index and `y` the dragged item's centre now. Returns the index in the list of the
 * other items (0 to n-1) that the dragged item should land before.
 */
export function dropSlot(centres: readonly number[], from: number, y: number): number {
  let dest = 0;
  centres.forEach((c, j) => {
    if (j !== from && c < y) dest += 1;
  });
  return dest;
}

/** The key the dragged item is dropped before: the item at `dest` among the others, '' for the end. */
export function beforeKey(others: readonly string[], dest: number): string {
  return others[dest] ?? '';
}

/** The letter shown on a letter tile: the first code point of the label, upper-cased. */
export function initialOf(label: string): string {
  const first = Array.from(label.trim())[0];
  return first === undefined ? '?' : first.toUpperCase();
}

/** Stores a favicon by key, dropping the oldest entries beyond the bound. */
export function rememberRailFav(favs: Map<string, string>, key: string, uri: string, live: ReadonlySet<string>): void {
  pruneRailFavs(favs, live);
  if (!live.has(key)) return;
  favs.delete(key);
  favs.set(key, uri);
  while (favs.size > RAIL_FAV_MAX) {
    const oldest = favs.keys().next();
    if (oldest.done) break;
    favs.delete(oldest.value);
  }
}

/** Drops cached favicons for keys that are no longer in the rail. */
export function pruneRailFavs(favs: Map<string, string>, live: ReadonlySet<string>): void {
  for (const key of [...favs.keys()]) {
    if (!live.has(key)) favs.delete(key);
  }
}

// --- rail flyouts ---
/** The pointer must rest this long on another flyout button before the open flyout switches to it. */
export const FLYOUT_HOVER_MS = 120;

/** The flyout a rail button opens: History, Bookmarks, or Customize (also the rail's "more" button). */
export function flyoutOf(key: string): 'history' | 'bookmarks' | 'customize' | null {
  if (key === 'history' || key === 'bookmarks') return key;
  if (key === 'customize' || key === 'menu') return 'customize';
  return null;
}

/**
 * Whether resting on `key` may switch an open flyout. The rail cannot tell which flyout is open
 * (Bookmarks is also marked active when the page is bookmarked), so it reports every flyout button
 * and Rust acts only while a different flyout is open.
 */
export function hoverSwitches(key: string): boolean {
  return flyoutOf(key) !== null;
}
// --- end rail flyouts ---
