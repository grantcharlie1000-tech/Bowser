// Pure tab-strip logic: no DOM and no runtime imports, so test/tabs-reconcile.mjs can load it directly.
import type { TabItem } from './protocol';

export const FAV_CACHE_MAX = 512;
export const DRAG_EDGE_PX = 24;

/** Share of the row an inactive tab gets; the active tab gets ACTIVE_WEIGHT of it (matches `.tab.active` in tabs.css). */
export const TAB_WEIGHT = 1;
export const ACTIVE_WEIGHT = 1.5;

/** Accessible name and tooltip of a private tab (the strip and the sidebar use the same words). */
export function privateTabName(title: string): string {
  return `${title} (Private)`;
}

/**
 * Flex-grow of a group island: the sum of its visible members' weights, so a group's tabs end up
 * exactly as wide as ungrouped ones (active 1.5x, the rest equal). 0 for a group with no visible tab.
 */
export function groupWeight(
  members: readonly Slot[],
  active: number,
  // --- split view --- a split pair's halves weigh differently (split-logic.ts pairWeight)
  weightOf: (id: number, fallback: number) => number = (_id, fallback) => fallback,
): number {
  let sum = 0;
  for (const s of members) {
    if (!s.hidden) sum += weightOf(s.item.id, s.item.id === active ? ACTIVE_WEIGHT : TAB_WEIGHT);
  }
  return sum;
}

export function hiddenForTab(grouped: boolean, collapsed: boolean, active: boolean): boolean {
  return grouped && collapsed && !active;
}

export function scrollDeltaForWheel(deltaX: number, deltaY: number, shiftKey: boolean): number {
  return shiftKey && deltaX === 0 ? deltaY : deltaX;
}

export function dragScrollDelta(x: number, left: number, right: number, edge = DRAG_EDGE_PX): number {
  if (x - left < edge) return -Math.max(1, edge - (x - left));
  if (right - x < edge) return Math.max(1, edge - (right - x));
  return 0;
}

export interface Slot {
  readonly item: TabItem;
  readonly hidden: boolean;
}
export interface TabEntry {
  readonly kind: 'tab';
  readonly slot: Slot;
}
export interface GroupEntry {
  readonly kind: 'group';
  readonly key: number;
  readonly leader: TabItem;
  readonly collapsed: boolean;
  readonly members: readonly Slot[];
  /** Number of members that are not hidden. */
  readonly visible: number;
}
export type Entry = TabEntry | GroupEntry;

/**
 * Turns the host's flat tab list into render entries: pinned tabs first (compact, never grouped),
 * then each group placed at the position of its first member. A collapsed group hides every member
 * except the active one.
 */
export function buildEntries(items: readonly TabItem[], active: number): Entry[] {
  const pinned: TabItem[] = [];
  const rest: TabItem[] = [];
  for (const item of items) {
    if (item.pinned) pinned.push(item);
    else rest.push(item);
  }
  const ordered = [...pinned, ...rest];

  const membersByKey = new Map<number, TabItem[]>();
  for (const item of ordered) {
    if (item.pinned || item.g === 0) continue;
    const list = membersByKey.get(item.g);
    if (list) list.push(item);
    else membersByKey.set(item.g, [item]);
  }

  const entries: Entry[] = [];
  const emitted = new Set<number>();
  for (const item of ordered) {
    if (item.pinned || item.g === 0) {
      entries.push({ kind: 'tab', slot: { item, hidden: false } });
      continue;
    }
    if (emitted.has(item.g)) continue;
    emitted.add(item.g);
    const list = membersByKey.get(item.g) ?? [item];
    const leader = list.find((m) => m.gl) ?? list[0] ?? item;
    const collapsed = leader.gcollapsed === true;
    const members = list.map((m): Slot => ({ item: m, hidden: hiddenForTab(true, collapsed, m.id === active) }));
    entries.push({
      kind: 'group',
      key: item.g,
      leader,
      collapsed,
      members,
      visible: members.filter((s) => !s.hidden).length,
    });
  }
  return entries;
}

/**
 * Destination index of the dragged tab. `centers` holds the centre x of every visible tab in strip
 * order (the dragged one at `from`); `x` is the dragged tab's current centre. The result is the
 * position it would take in the final order, so 0..centers.length-1.
 */
export function dropIndex(centers: readonly number[], from: number, x: number): number {
  let dest = 0;
  centers.forEach((c, i) => {
    if (i !== from && c < x) dest += 1;
  });
  return dest;
}

/** Slot index that tab `i` occupies once the dragged tab (from) sits at `dest`. */
export function shiftTarget(i: number, from: number, dest: number): number {
  if (i === from) return i;
  if (from < dest && i > from && i <= dest) return i - 1;
  if (from > dest && i >= dest && i < from) return i + 1;
  return i;
}

/** Horizontal translation (px) that moves tab `i` onto the centre of its target slot. */
export function slotShift(centers: readonly number[], i: number, from: number, dest: number): number {
  const here = centers[i];
  const there = centers[shiftTarget(i, from, dest)];
  if (here === undefined || there === undefined) return 0;
  return there - here;
}

/** Id of the tab to the right of the drop slot. `others` are the ids without the dragged tab; 0 when last. */
export function beforeId(others: readonly number[], dest: number): number {
  return others[dest] ?? 0;
}

/**
 * Stable string for the arrangement of the strip: visible tab ids in order plus group boundaries
 * and collapse state. Two renders with the same signature need no position animation.
 */
export function layoutSignature(entries: readonly Entry[]): string {
  const parts: string[] = [];
  for (const e of entries) {
    if (e.kind === 'tab') {
      parts.push(String(e.slot.item.id));
      continue;
    }
    parts.push(`g${e.key}${e.collapsed ? 'c' : 'o'}[${e.members.filter((s) => !s.hidden).map((s) => s.item.id).join(',')}]`);
  }
  return parts.join(' ');
}

/**
 * `before` id for moving the tab at `at` one slot left (-1) or right (+1) among `ids` (visible
 * order), or null when it is already at that end.
 */
export function stepMoveBefore(ids: readonly number[], at: number, dir: -1 | 1): number | null {
  const dest = at + dir;
  if (at < 0 || at >= ids.length || dest < 0 || dest >= ids.length) return null;
  const others = ids.filter((_, i) => i !== at);
  return beforeId(others, dest);
}

/** Stores a favicon and keeps the map bounded (oldest insertion dropped first). */
export function rememberFav(favs: Map<number, string>, id: number, uri: string, live: ReadonlySet<number>, max = FAV_CACHE_MAX): void {
  pruneFavs(favs, live);
  if (!live.has(id)) return;
  favs.delete(id);
  favs.set(id, uri);
  while (favs.size > max) {
    const oldest = favs.keys().next().value;
    if (oldest === undefined) break;
    favs.delete(oldest);
  }
}

/** Drops favicons whose tab no longer exists. */
export function pruneFavs(favs: Map<number, string>, live: ReadonlySet<number>): void {
  for (const id of [...favs.keys()]) {
    if (!live.has(id)) favs.delete(id);
  }
}
