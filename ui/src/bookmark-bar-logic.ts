// Pure rules of the bookmarks bar (?view=bar): the snapshot contract, what each item shows in each
// display mode, the commands it sends and how many items fit. No DOM here; test/address.mjs covers all
// of it. The wire is src/chrome_host/address_page.rs (`bookmarkbar`).
import type { BarDisplay } from './address-settings-logic';
import { BAR_DISPLAYS } from './address-settings-logic';
import type { PanelFields, SnapshotMsg } from './protocol';

export const MAX_BAR_ITEMS = 200;
export const MAX_BAR_GROUPS = 32;
const FAV_PREFIX = 'data:image/png;base64,';
const BASE64_RE = /^[A-Za-z0-9+/]+={0,2}$/;

export interface BarItem {
  readonly id: number;
  readonly kind: 'link' | 'folder';
  readonly title: string;
  readonly url: string;
  readonly fav: string;
}
export interface BarGroup {
  readonly key: string;
  readonly name: string;
  readonly pinned: boolean;
  readonly live: boolean;
  /** Palette index, or -1 for the theme accent. */
  readonly colour: number;
}
export interface BarSnapshot {
  readonly display: BarDisplay;
  readonly items: readonly BarItem[];
  readonly groups: readonly BarGroup[];
}

const noControls = (v: unknown, max: number): v is string => typeof v === 'string' && v.length <= max && !/[\u0000-\u001f\u007f]/.test(v);
const isFav = (v: unknown): v is string => typeof v === 'string' && (v === '' || (v.length <= 8 * 1024 + 64 && v.startsWith(FAV_PREFIX) && BASE64_RE.test(v.slice(FAV_PREFIX.length))));

/** Strict validation of a `bookmarkbar` snapshot; `null` for anything partial, extended or malformed. */
export function parseBarSnapshot(d: SnapshotMsg['d']): BarSnapshot | null {
  const keys = ['display', 'items', 'groups'];
  if (Object.keys(d).length !== keys.length || Object.keys(d).some((key) => !keys.includes(key))) return null;
  if (!(BAR_DISPLAYS as readonly unknown[]).includes(d.display) || !Array.isArray(d.items) || !Array.isArray(d.groups)) return null;
  if (d.items.length > MAX_BAR_ITEMS || d.groups.length > MAX_BAR_GROUPS) return null;
  const items: BarItem[] = [];
  const ids = new Set<number>();
  for (const raw of d.items) {
    if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return null;
    const { id, kind, title, url, fav } = raw as Record<string, unknown>;
    if (typeof id !== 'number' || !Number.isInteger(id) || id < 1 || ids.has(id) || (kind !== 'link' && kind !== 'folder')) return null;
    if (!noControls(title, 480) || !noControls(url, 2048) || !isFav(fav) || (kind === 'folder' && url !== '')) return null;
    ids.add(id);
    items.push({ id, kind, title, url, fav });
  }
  const groups: BarGroup[] = [];
  for (const raw of d.groups) {
    if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return null;
    const { key, name, pinned, live, colour } = raw as Record<string, unknown>;
    if (!noControls(key, 253) || key === '' || /[\s,/\\=#]/.test(key) || !noControls(name, 320) || typeof pinned !== 'boolean' || typeof live !== 'boolean') return null;
    if (typeof colour !== 'number' || !Number.isInteger(colour) || colour < -1 || colour > 255) return null;
    groups.push({ key, name, pinned, live, colour });
  }
  return { display: d.display as BarDisplay, items, groups };
}

/** What a bookmark or folder shows on the bar in `display`. */
export interface Face {
  readonly icon: 'site' | 'bookmark' | 'folder' | null;
  readonly text: boolean;
}

export function faceOf(item: Pick<BarItem, 'kind'>, display: BarDisplay): Face {
  if (item.kind === 'folder') return { icon: display === 'text' ? null : 'folder', text: display !== 'icon' };
  switch (display) {
    case 'text-icon': return { icon: 'site', text: true };
    case 'text': return { icon: null, text: true };
    case 'icon': return { icon: 'site', text: false };
    case 'bookmark-icon': return { icon: 'bookmark', text: false };
  }
}

// ---- commands ----

export function openFields(id: number, newTab: boolean): PanelFields { return { a: 'baropen', id, n: newTab ? 1 : 0 }; }
export function folderFields(id: number, rect: string): PanelFields { return { a: 'barfolder', id, text: rect }; }
export function contextFields(id: number): PanelFields { return { a: 'barctx', id }; }
export function groupFields(key: string): PanelFields { return { a: 'bargroup', text: key }; }
export function groupContextFields(key: string): PanelFields { return { a: 'bargctx', text: key }; }
export function overflowFields(first: number, rect: string): PanelFields { return { a: 'baroverflow', n: first, text: rect }; }

/**
 * How many of `widths` fit in `available` pixels with `gap` between them, leaving room for the
 * overflow button (`reserve`) when anything is left out.
 */
export function fitCount(widths: readonly number[], available: number, gap: number, reserve: number): number {
  const total = widths.reduce((sum, width, i) => sum + width + (i > 0 ? gap : 0), 0);
  if (total <= available) return widths.length;
  let used = 0;
  let count = 0;
  for (const width of widths) {
    const next = used + width + (count > 0 ? gap : 0);
    if (next + gap + reserve > available) break;
    used = next;
    count += 1;
  }
  return count;
}
