// Pure vertical-sidebar ordering and geometry, shared by the Tabs view renderer.
import type { TabItem } from './protocol';
import { buildEntries, groupColourIndex, type Entry } from './tabs-logic';

export type SidebarEntry =
  | { readonly kind: 'tab'; readonly item: TabItem }
  | {
    readonly kind: 'group';
    readonly key: number;
    readonly name: string;
    readonly colour: number;
    readonly collapsed: boolean;
    readonly count: number;
    readonly members: readonly TabItem[];
  };

/** Builds pinned-first rows and group headers from the existing flat tabs payload. */
export function buildSidebarEntries(items: readonly TabItem[], active: number): SidebarEntry[] {
  const entries: SidebarEntry[] = [];
  for (const entry of buildEntries(items, active)) {
    if (entry.kind === 'tab') {
      if (!entry.slot.hidden) entries.push({ kind: 'tab', item: entry.slot.item });
      continue;
    }
    const name = entry.leader.gname;
    entries.push({
      kind: 'group',
      key: entry.key,
      name: name !== undefined && name !== '' ? name : 'Tab group',
      colour: groupColourIndex(entry.leader.gcol, entry.key, name),
      collapsed: entry.collapsed,
      count: entry.leader.gsize ?? entry.members.length,
      members: entry.members.filter((member) => !member.hidden).map((member) => member.item),
    });
    for (const member of entry.members) {
      if (!member.hidden) entries.push({ kind: 'tab', item: member.item });
    }
  }
  return entries;
}

/** Vertical destination slot after excluding the dragged row. */
export function dropIndexVertical(centres: readonly number[], from: number, y: number): number {
  let destination = 0;
  centres.forEach((centre, index) => {
    if (index !== from && centre < y) destination += 1;
  });
  return destination;
}

/** Minimal scrollTop adjustment that fully reveals one row inside the viewport. */
export function scrollIntoViewDelta(scrollTop: number, viewportHeight: number, itemTop: number, itemHeight: number): number {
  if (itemTop < scrollTop) return itemTop - scrollTop;
  const itemBottom = itemTop + itemHeight;
  const viewportBottom = scrollTop + viewportHeight;
  return itemBottom > viewportBottom ? itemBottom - viewportBottom : 0;
}

/** Targets for moving sibling rows around a vertical drop slot. */
export function verticalShiftTarget(index: number, from: number, destination: number): number {
  if (index === from) return index;
  if (from < destination && index > from && index <= destination) return index - 1;
  if (from > destination && index >= destination && index < from) return index + 1;
  return index;
}

export function verticalSlotShift(centres: readonly number[], index: number, from: number, destination: number): number {
  return (centres[verticalShiftTarget(index, from, destination)] ?? centres[index] ?? 0) - (centres[index] ?? 0);
}

/** A pinned tile's centre in the 3-up grid (client px). */
export type TilePoint = readonly [x: number, y: number];

/** Pinned-grid drop slot: the tile slot whose centre is nearest to (x, y). Ties keep the lower slot. */
export function nearestTile(tiles: readonly TilePoint[], x: number, y: number): number {
  let best = 0;
  let bestDistance = Infinity;
  tiles.forEach(([tx, ty], index) => {
    const distance = (tx - x) ** 2 + (ty - y) ** 2;
    if (distance < bestDistance) {
      best = index;
      bestDistance = distance;
    }
  });
  return best;
}

/** Offset (x, y) that moves pinned tile `index` onto the slot it takes while `from` sits at `destination`. */
export function tileShift(tiles: readonly TilePoint[], index: number, from: number, destination: number): TilePoint {
  const here = tiles[index];
  const there = tiles[verticalShiftTarget(index, from, destination)];
  if (here === undefined || there === undefined) return [0, 0];
  return [there[0] - here[0], there[1] - here[1]];
}

/**
 * `before` id for a pinned tile dropped at `destination` among the other tiles: the tile after the
 * slot, or the first unpinned tab when it lands last (0 with no unpinned tab), so it stays pinned-first.
 */
export function pinnedBeforeId(others: readonly number[], destination: number, firstUnpinned: number): number {
  return others[destination] ?? firstUnpinned;
}

/** Workspace glyph id from Rust ("g:code") to an icons.ts name; unknown ids get the generic workspace stack. */
export function workspaceIconName(glyph: string): string {
  return /^g:(briefcase|home|code|book|cart|game|music|heart|star|bolt|globe|leaf)$/.test(glyph)
    ? `workspace-${glyph.slice(2)}`
    : 'workspace';
}

/** Next row index for a vertical roving-focus key (clamped, no wrap, like a native tree). */
export function rovingTarget(key: string, at: number, count: number): number {
  switch (key) {
    case 'ArrowUp': return Math.max(0, at - 1);
    case 'ArrowDown': return Math.min(count - 1, at + 1);
    case 'Home': return 0;
    case 'End': return count - 1;
    default: return at;
  }
}

/** How long the pointer must rest on the rail before it opens, and before it closes after leaving. */
export const RAIL_OPEN_MS = 80;
export const RAIL_CLOSE_MS = 140;

/**
 * Next hover command for the unpinned rail.
 * Pass `due: null` when `want` changes. `immediate` (keyboard focus) sends on this call.
 * `emit` is the boolean to send, or null when nothing should be sent yet.
 */
export function railHoverAdvance(
  sent: boolean,
  want: boolean,
  due: number | null,
  now: number,
  immediate: boolean,
): { sent: boolean; due: number | null; emit: boolean | null } {
  if (sent === want) return { sent, due: null, emit: null };
  if (immediate || (due !== null && now >= due)) return { sent: want, due: null, emit: want };
  if (due === null) return { sent, due: now + (want ? RAIL_OPEN_MS : RAIL_CLOSE_MS), emit: null };
  return { sent, due, emit: null };
}

export type { Entry };
