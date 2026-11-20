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

/**
 * What decides whether a render scrolls the active tab into view: its id and its place in the
 * order. A render that changes neither (a background tab's title, favicon, load progress or audio)
 * must leave the strip or sidebar where the user scrolled it; selecting or moving the active tab
 * reveals it again.
 */
export function revealKey(items: readonly Pick<TabItem, 'id'>[], active: number): string {
  return `${active}:${items.findIndex((item) => item.id === active)}`;
}

/** How far a wheel over the tab strip scrolls it sideways. The strip only scrolls one way, so an ordinary
 *  (vertical) mouse wheel scrolls it too, not just Shift+wheel or a sideways trackpad swipe; a sideways
 *  movement wins when there is one. */
export function scrollDeltaForWheel(deltaX: number, deltaY: number, _shiftKey: boolean): number {
  return deltaX !== 0 ? deltaX : deltaY;
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

/**
 * Left edge a dragged tab visually held when its host reorder lands: its slot's left edge once the
 * drag transform is cleared (`home`) plus the drag offset it carried (`prev`). Used instead of a
 * transformed rect so the 1.02 lift scale does not skew it.
 */
export function landingFirst(home: number, prev: number): number {
  return home + prev;
}

/** FLIP invert: the translation that puts a tab back at `first` while it sits at `last`. */
export function flipInvert(first: number, last: number): number {
  return first - last;
}

/**
 * What a render does while a dropped tab is landing: 'stale' (same layout as at the drop: content
 * only, no transform touched) or 'reorder' (the layout changed: one FLIP from where the tab was
 * released). 'none' when nothing is landing.
 */
export function landingPhase(landingSignature: string | null, nextSignature: string): 'none' | 'stale' | 'reorder' {
  if (landingSignature === null) return 'none';
  return landingSignature === nextSignature ? 'stale' : 'reorder';
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
 * order), or null when it is already at that end. A collapsed group is one slot, as for a drag:
 * `leaderOf` maps its visible tab to its first member (see `beforeSlotId`).
 */
export function stepMoveBefore(ids: readonly number[], at: number, dir: -1 | 1, leaderOf: ReadonlyMap<number, number> = new Map()): number | null {
  const dest = at + dir;
  if (at < 0 || at >= ids.length || dest < 0 || dest >= ids.length) return null;
  const others = ids.filter((_, i) => i !== at);
  return beforeSlotId(others, dest, leaderOf);
}

/**
 * The tab that takes the keyboard when the focused tab `focused` closes: the next one in `order` that
 * is still `live`, else the one before it; 0 when `focused` did not close (or nothing is left). Keeps
 * focus on a tab after Delete instead of dropping it to the page body.
 */
export function focusHeir(order: readonly number[], focused: number, live: ReadonlySet<number>): number {
  const at = order.indexOf(focused);
  if (at < 0 || live.has(focused)) return 0;
  const after = order.slice(at + 1).find((id) => live.has(id));
  if (after !== undefined) return after;
  return order.slice(0, at).reverse().find((id) => live.has(id)) ?? 0;
}

/**
 * Palette index (0-7) of a group. Rust sends the chosen colour id, or a negative id for an automatic
 * group (one folded per site); those get a stable palette colour from the group's name (or key) so
 * every group is told apart, instead of all sharing the theme accent.
 */
export function groupColourIndex(gcol: number | undefined, key: number, name: string | undefined): number {
  if (gcol !== undefined && gcol >= 0) return gcol & 7;
  const text = name !== undefined && name !== '' ? name : `#${key}`;
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return (h >>> 0) % 8;
}

/**
 * `before` id for a drop at slot `dest` among the visible `others`. A collapsed group is one slot: its
 * only visible tab stands for the whole group, so dropping before it means before the group's first
 * member (`leaderOf` maps a collapsed group's visible tab to that first member), never between its
 * hidden members.
 */
export function beforeSlotId(others: readonly number[], dest: number, leaderOf: ReadonlyMap<number, number>): number {
  const id = others[dest];
  if (id === undefined) return 0;
  return leaderOf.get(id) ?? id;
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

// ----- moving a whole group by its handle -----

/**
 * One top-level unit a group can be dragged among: an unpinned tab (`key` 0) or a whole group. `first`
 * is the tab a move in front of the unit names: the tab itself, or the group's first member (hidden
 * ones included, so a folded group is still one slot).
 */
export interface StripUnit {
  readonly key: number;
  readonly first: number;
}

/** The units after the pinned tabs, in strip order (pinned tabs never move with or past a group). */
export function groupUnits(entries: readonly Entry[]): StripUnit[] {
  const units: StripUnit[] = [];
  for (const e of entries) {
    if (e.kind === 'tab') {
      if (!e.slot.item.pinned) units.push({ key: 0, first: e.slot.item.id });
      continue;
    }
    const first = e.members[0]?.item.id;
    if (first !== undefined) units.push({ key: e.key, first });
  }
  return units;
}

/**
 * Slot the dragged block takes among units whose home centres are `centers` (the block's own at
 * `from`), from where the block is drawn now (`left`, `right`): a unit after it is passed once the
 * block's right edge crosses that unit's centre, a unit before it once the left edge does. Edges
 * rather than the pointer, so a wide group grabbed by its bar at the left end moves right as readily
 * as left.
 */
export function blockDropIndex(centers: readonly number[], from: number, left: number, right: number): number {
  let dest = 0;
  centers.forEach((c, i) => {
    if (i === from) return;
    if (i < from ? left > c : right > c) dest += 1;
  });
  return dest;
}

/**
 * Travel (px) that keeps a block at `from` within the row of units (`lefts`, `widths`): it never
 * slides over the pinned tabs or past the last unit.
 */
export function clampBlock(dx: number, lefts: readonly number[], widths: readonly number[], from: number): number {
  const n = lefts.length;
  const home = lefts[from];
  const width = widths[from];
  const start = lefts[0];
  const endLeft = lefts[n - 1];
  const endWidth = widths[n - 1];
  if (home === undefined || width === undefined || start === undefined || endLeft === undefined || endWidth === undefined) return 0;
  return Math.min(Math.max(dx, start - home), endLeft + endWidth - (home + width));
}

/**
 * `before` id for the group at `from` dropped at slot `dest` among units whose first tabs are
 * `firsts`: the first tab of the unit it lands in front of, 0 for the end; null when it stays.
 */
export function groupMoveBefore(firsts: readonly number[], from: number, dest: number): number | null {
  if (from < 0 || from >= firsts.length || dest === from || dest < 0 || dest >= firsts.length) return null;
  const others = firsts.filter((_, i) => i !== from);
  return others[dest] ?? 0;
}

/** `before` id for moving the group at `at` one unit left (-1) or right (+1) by the keyboard; null at that end. */
export function groupStepBefore(firsts: readonly number[], at: number, dir: -1 | 1): number | null {
  return groupMoveBefore(firsts, at, at + dir);
}

/** The strip's keyboard move chord (Ctrl+Shift+Left/Right or Page Up/Down): -1 or +1, 0 for any other key. */
export function moveChordDir(key: string, ctrl: boolean, shift: boolean, alt: boolean): -1 | 0 | 1 {
  if (!ctrl || !shift || alt) return 0;
  if (key === 'ArrowLeft' || key === 'PageUp') return -1;
  if (key === 'ArrowRight' || key === 'PageDown') return 1;
  return 0;
}

/** Screen-reader text for each moment of a group drag (`name` is the group's label). */
export function groupDragAnnouncement(kind: 'lift' | 'drop' | 'cancel', name: string, position = 0, count = 0): string {
  switch (kind) {
    case 'lift': return `Picked up group ${name}. Move to reorder, Escape to cancel.`;
    case 'drop': return count > 0 ? `Group ${name} moved to position ${position} of ${count}.` : `Group ${name} dropped.`;
    case 'cancel': return `Move cancelled. Group ${name} returned to its place.`;
  }
}
