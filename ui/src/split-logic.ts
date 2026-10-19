// Pure split view logic: no DOM and no runtime imports, so test/split.mjs can load it directly.
// See docs/design/split-view.md.
import type { TabItem } from './protocol';
import type { PairMsg, PickClosed, PickTab, SplitAt, SplitEdge } from './split-protocol';

/** Flex weight of each half of the shown pair (the pair is the widest unit: 2.5 > 1.5). */
export const SHOWN_HALF_WEIGHT = 1.25;
/** Flex weight of each pane's tab of a shown layout of three or four panes (3 or 4 together). */
export const SHOWN_PANE_WEIGHT = 1;
/** Flex weight of each half of a parked pair (1.4 together, still narrower than the active tab's 1.5). */
export const PARKED_HALF_WEIGHT = 0.7;
/** A tab dragged this far (CSS px) below the strip counts as over the page. */
export const DRAG_OUT_PX = 12;

/** A tab's place in the joined capsule: the first (`l`), a middle one (`m`) or the last (`r`). */
export type PairRole = 'l' | 'm' | 'r';

/** The pair as the strip uses it, or null when there is none. */
export function livePair(msg: PairMsg | null): PairMsg | null {
  return msg !== null && msg.l !== 0 && msg.r !== 0 ? msg : null;
}

/** The layout's tabs in capsule order: a pair's two tabs, or every pane of a larger layout. */
export function layoutIds(pair: PairMsg | null): readonly number[] {
  if (pair === null) return [];
  return pair.panes !== undefined ? pair.panes.map((pane) => pane.id) : [pair.l, pair.r];
}

export function pairRole(id: number, pair: PairMsg | null): PairRole | null {
  const ids = layoutIds(pair);
  const at = ids.indexOf(id);
  if (at < 0) return null;
  return at === 0 ? 'l' : at === ids.length - 1 ? 'r' : 'm';
}

/**
 * The tab list as the strip shows it: every other tab of the layout directly after its first tab
 * (a pair: the right tab after the left), in the same group and pin section, so both renderers
 * draw one joined capsule. A tab that led its old group hands the group fields to that group's next
 * member. Unchanged without a live layout whose tabs are all in the list.
 */
export function pairOrder(items: readonly TabItem[], pair: PairMsg | null): readonly TabItem[] {
  const live = livePair(pair);
  if (live === null) return items;
  const ids = layoutIds(live);
  const members = ids.map((id) => items.find((item) => item.id === id));
  if (members.some((member) => member === undefined)) return items;
  const [first, ...others] = members as TabItem[];
  if (first === undefined) return items;
  let rest = items.slice();
  const joined: TabItem[] = [];
  for (const other of others) {
    rest = rest.filter((item) => item.id !== other.id);
    // Hand the group to the next member when this tab led it and leaves it.
    if (other.gl && other.g !== 0 && other.g !== first.g) {
      const heir = rest.findIndex((item) => item.g === other.g);
      if (heir >= 0) {
        const next = rest[heir] as TabItem;
        rest[heir] = {
          ...next,
          gl: true,
          ...(other.gname !== undefined ? { gname: other.gname } : {}),
          ...(other.gcol !== undefined ? { gcol: other.gcol } : {}),
          ...(other.gcollapsed !== undefined ? { gcollapsed: other.gcollapsed } : {}),
          ...(other.gsize !== undefined ? { gsize: Math.max(1, other.gsize - 1) } : {}),
        };
      }
    }
    joined.push({
      id: other.id,
      title: other.title,
      loading: other.loading,
      pinned: first.pinned,
      muted: other.muted,
      private: other.private,
      marker: other.marker,
      g: first.pinned ? 0 : first.g,
      gl: false,
      fav: other.fav,
    });
  }
  const at = rest.findIndex((item) => item.id === first.id);
  rest.splice(at + 1, 0, ...joined);
  return rest;
}

/**
 * Flex weight of tab `id`: halves of the shown pair 1.25 each, panes of a shown larger layout 1
 * each, of a parked layout 0.7 each; otherwise `fallback` (the strip's own active / inactive weight).
 */
export function pairWeight(id: number, pair: PairMsg | null, fallback: number): number {
  const live = livePair(pair);
  if (pairRole(id, live) === null) return fallback;
  if (live?.shown !== true) return PARKED_HALF_WEIGHT;
  return live.panes !== undefined ? SHOWN_PANE_WEIGHT : SHOWN_HALF_WEIGHT;
}

/** Whether the pair is stacked (top over bottom); a pair without `o` is side by side. */
export function pairStacked(pair: PairMsg | null): boolean {
  return pair?.o === 'v';
}

/** The page edge of a pair slot: `l`/`r` side by side, `t`/`b` stacked. */
export function roleEdge(role: 'l' | 'r', pair: PairMsg | null): SplitEdge {
  if (pairStacked(pair)) return role === 'l' ? 't' : 'b';
  return role;
}

const EDGE_WORD: Readonly<Record<SplitAt, string>> = {
  l: 'left',
  r: 'right',
  t: 'top',
  b: 'bottom',
  tl: 'top-left',
  tr: 'top-right',
  bl: 'bottom-left',
  br: 'bottom-right',
};

const capital = (word: string): string => `${word.charAt(0).toUpperCase()}${word.slice(1)}`;

/** Where tab `id` sits: a pair half's edge, or a larger layout's pane place; null outside it. */
export function paneAt(id: number, pair: PairMsg | null): SplitAt | null {
  const live = livePair(pair);
  if (live === null) return null;
  if (live.panes !== undefined) return live.panes.find((pane) => pane.id === id)?.at ?? null;
  if (id === live.l) return roleEdge('l', live);
  if (id === live.r) return roleEdge('r', live);
  return null;
}

/** Whether tab `id` is the focused pane of the shown layout. */
export function paneFocused(id: number, pair: PairMsg | null): boolean {
  const live = livePair(pair);
  if (live === null || !live.shown) return false;
  if (live.panes !== undefined) return live.fp !== undefined && paneAt(id, live) === live.fp;
  return (live.f === 'l' ? live.l : live.r) === id;
}

/**
 * Accessible description of tab `id` in the layout: a pair's "Left side of split view" (as
 * [`pairDescription`]), or a larger layout's "Top-left pane of split view", plus ", focused".
 */
export function paneDescription(id: number, pair: PairMsg): string {
  const role = pairRole(id, pair);
  if (pair.panes === undefined && role !== null && role !== 'm') return pairDescription(role, pair);
  const at = paneAt(id, pair);
  if (at === null) return '';
  const text = `${capital(EDGE_WORD[at])} pane of split view`;
  return paneFocused(id, pair) ? `${text}, focused` : text;
}

/** A pane place's glyph: a half (`split-left` ...) or a quarter (`split-top-left` ...). */
export type PlaceGlyph =
  | 'split-left' | 'split-right' | 'split-top' | 'split-bottom'
  | 'split-top-left' | 'split-top-right' | 'split-bottom-left' | 'split-bottom-right';

const PLACE_GLYPH: Readonly<Record<SplitAt, PlaceGlyph>> = {
  l: 'split-left',
  r: 'split-right',
  t: 'split-top',
  b: 'split-bottom',
  tl: 'split-top-left',
  tr: 'split-top-right',
  bl: 'split-bottom-left',
  br: 'split-bottom-right',
};

/**
 * Accessible description of a half: "Left side of split view" ("Top side of split view" when
 * stacked), and whether it is the focused side.
 */
export function pairDescription(role: 'l' | 'r', pair: PairMsg): string {
  const word = EDGE_WORD[roleEdge(role, pair)];
  const side = `${capital(word)} side of split view`;
  return pair.shown && pair.f === role ? `${side}, focused` : side;
}

/**
 * The toolbar split button's glyph: the focused pane's place filled while a split is shown (a half,
 * or a quarter of a larger layout).
 */
export function splitGlyph(pair: PairMsg | null): 'split' | PlaceGlyph {
  const live = livePair(pair);
  if (live === null || !live.shown) return 'split';
  if (live.panes !== undefined && live.fp !== undefined) return PLACE_GLYPH[live.fp];
  return PLACE_GLYPH[roleEdge(live.f, live)];
}

/**
 * Whether the pointer (CSS px in the tab view) has left the strip onto the page: downwards, or
 * upwards when the strip runs along the bottom of the window (`atBottom`).
 */
export function overPage(clientY: number, strip: { readonly top: number; readonly bottom: number }, atBottom = false): boolean {
  return atBottom ? clientY < strip.top - DRAG_OUT_PX : clientY > strip.bottom + DRAG_OUT_PX;
}

/**
 * Whether the pointer (CSS px in the tab view) has left the vertical sidebar onto the page: to the
 * right, or to the left when the sidebar is docked at the right (`atRight`).
 */
export function overPageBeside(clientX: number, sidebar: { readonly left: number; readonly right: number }, atRight = false): boolean {
  return atRight ? clientX < sidebar.left - DRAG_OUT_PX : clientX > sidebar.right + DRAG_OUT_PX;
}

/**
 * The drop-zone card's wording and glyph for where a dragged tab would open: a half of the page
 * (`l` ... `b`) or a quarter (`tl` ... `br`).
 */
export function dropCard(side: SplitAt): { readonly label: string; readonly glyph: PlaceGlyph } {
  switch (side) {
    case 'l': return { label: 'Open on the left', glyph: 'split-left' };
    case 'r': return { label: 'Open on the right', glyph: 'split-right' };
    case 't': return { label: 'Open at the top', glyph: 'split-top' };
    case 'b': return { label: 'Open at the bottom', glyph: 'split-bottom' };
    case 'tl': return { label: 'Open at the top left', glyph: 'split-top-left' };
    case 'tr': return { label: 'Open at the top right', glyph: 'split-top-right' };
    case 'bl': return { label: 'Open at the bottom left', glyph: 'split-bottom-left' };
    case 'br': return { label: 'Open at the bottom right', glyph: 'split-bottom-right' };
  }
}

/** Clamps a drag point into the wire range (the view may report negative or fractional values). */
export function wirePoint(x: number, y: number): { x: number; y: number } {
  // Signed: with the sidebar at the right or the strip at the bottom, the page is left of or above the view.
  const clamp = (v: number): number => Math.min(16384, Math.max(-16384, Math.round(Number.isFinite(v) ? v : 0)));
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

/**
 * Heading of the empty side ("Choose a tab for the left side", "... for the top side", ...), or of
 * an empty quarter ("Choose a tab for the top-left pane").
 */
export function pickHeading(side: SplitAt): string {
  return side.length === 2 ? `Choose a tab for the ${EDGE_WORD[side]} pane` : `Choose a tab for the ${EDGE_WORD[side]} side`;
}
