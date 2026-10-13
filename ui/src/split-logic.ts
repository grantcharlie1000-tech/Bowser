// Pure split view logic: no DOM and no runtime imports, so test/split.mjs can load it directly.
// See docs/design/split-view.md.
import type { TabItem } from './protocol';
import type { PairMsg, PickClosed, PickTab } from './split-protocol';

/** Flex weight of each half of the shown pair (the pair is the widest unit: 2.5 > 1.5). */
export const SHOWN_HALF_WEIGHT = 1.25;
/** Flex weight of each half of a parked pair (1.4 together, still narrower than the active tab's 1.5). */
export const PARKED_HALF_WEIGHT = 0.7;
/** A tab dragged this far (CSS px) below the strip counts as over the page. */
export const DRAG_OUT_PX = 12;

export type PairRole = 'l' | 'r';

/** The pair as the strip uses it, or null when there is none. */
export function livePair(msg: PairMsg | null): PairMsg | null {
  return msg !== null && msg.l !== 0 && msg.r !== 0 ? msg : null;
}

export function pairRole(id: number, pair: PairMsg | null): PairRole | null {
  if (pair === null) return null;
  if (id === pair.l) return 'l';
  if (id === pair.r) return 'r';
  return null;
}

/**
 * The tab list as the strip shows it: the pair's right tab directly after its left tab, in the same
 * group and pin section, so both renderers draw one joined capsule. A right tab that led its old
 * group hands the group fields to that group's next member. Unchanged without a live pair whose
 * tabs are both in the list.
 */
export function pairOrder(items: readonly TabItem[], pair: PairMsg | null): readonly TabItem[] {
  const live = livePair(pair);
  if (live === null) return items;
  const left = items.find((item) => item.id === live.l);
  const right = items.find((item) => item.id === live.r);
  if (left === undefined || right === undefined) return items;
  const rest = items.filter((item) => item.id !== right.id);
  // Hand the group to the next member when the right tab led it and leaves it.
  if (right.gl && right.g !== 0 && right.g !== left.g) {
    const heir = rest.findIndex((item) => item.g === right.g);
    if (heir >= 0) {
      const next = rest[heir] as TabItem;
      rest[heir] = {
        ...next,
        gl: true,
        ...(right.gname !== undefined ? { gname: right.gname } : {}),
        ...(right.gcol !== undefined ? { gcol: right.gcol } : {}),
        ...(right.gcollapsed !== undefined ? { gcollapsed: right.gcollapsed } : {}),
        ...(right.gsize !== undefined ? { gsize: Math.max(1, right.gsize - 1) } : {}),
      };
    }
  }
  const joined: TabItem = {
    id: right.id,
    title: right.title,
    loading: right.loading,
    pinned: left.pinned,
    muted: right.muted,
    private: right.private,
    marker: right.marker,
    g: left.pinned ? 0 : left.g,
    gl: false,
    fav: right.fav,
  };
  const at = rest.findIndex((item) => item.id === left.id);
  rest.splice(at + 1, 0, joined);
  return rest;
}

/**
 * Flex weight of tab `id`: halves of the shown pair 1.25 each, of a parked pair 0.7 each; otherwise
 * `fallback` (the strip's own active / inactive weight).
 */
export function pairWeight(id: number, pair: PairMsg | null, fallback: number): number {
  if (pairRole(id, livePair(pair)) === null) return fallback;
  return pair?.shown === true ? SHOWN_HALF_WEIGHT : PARKED_HALF_WEIGHT;
}

/** Accessible description of a half: "Left side of split view" (and whether it is the focused side). */
export function pairDescription(role: PairRole, pair: PairMsg): string {
  const side = role === 'l' ? 'Left side of split view' : 'Right side of split view';
  return pair.shown && pair.f === role ? `${side}, focused` : side;
}

/** Whether the pointer (CSS px in the tab view) has left the strip downwards, onto the page. */
export function overPage(clientY: number, stripBottom: number): boolean {
  return clientY > stripBottom + DRAG_OUT_PX;
}

/** Clamps a drag point into the wire range (the view may report negative or fractional values). */
export function wirePoint(x: number, y: number): { x: number; y: number } {
  const clamp = (v: number): number => Math.min(16384, Math.max(0, Math.round(Number.isFinite(v) ? v : 0)));
  return { x: clamp(x), y: clamp(y) };
}

// ----- the empty side ("choose a tab") -----

export interface PickRows {
  readonly tabs: readonly PickTab[];
  readonly closed: readonly PickClosed[];
}

/** Rows whose title or host contains every word of `query` (case-insensitive). */
export function filterPick(tabs: readonly PickTab[], closed: readonly PickClosed[], query: string): PickRows {
  const words = query.toLowerCase().split(/\s+/).filter((word) => word !== '');
  if (words.length === 0) return { tabs, closed };
  const hit = (title: string, host: string): boolean => {
    const hay = `${title} ${host}`.toLowerCase();
    return words.every((word) => hay.includes(word));
  };
  return {
    tabs: tabs.filter((tab) => hit(tab.title, tab.host)),
    closed: closed.filter((page) => hit(page.title, page.host)),
  };
}

/** Next highlighted row for Up/Down/Home/End over `count` rows, or -1 when there are none. */
export function moveSelection(count: number, index: number, key: 'ArrowUp' | 'ArrowDown' | 'Home' | 'End'): number {
  if (count <= 0) return -1;
  switch (key) {
    case 'Home':
      return 0;
    case 'End':
      return count - 1;
    case 'ArrowUp':
      return index <= 0 ? count - 1 : index - 1;
    case 'ArrowDown':
      return index < 0 || index >= count - 1 ? 0 : index + 1;
  }
}

/** Heading of the empty side. */
export function pickHeading(side: 'l' | 'r'): string {
  return side === 'l' ? 'Choose a tab for the left side' : 'Choose a tab for the right side';
}
