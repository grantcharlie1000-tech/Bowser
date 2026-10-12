// Pure vertical-sidebar ordering and geometry, shared by the Tabs view renderer.
import type { TabItem } from './protocol';
import { buildEntries, type Entry } from './tabs-logic';

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
      colour: entry.leader.gcol ?? -1,
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

export type { Entry };
