// Split view wire contract (docs/html-chrome-protocol.md, "Split view"). Pure and self-contained so
// protocol.ts can import it without a cycle and test/split.mjs can load it directly.
//
// Rust -> JS: `pair` (tab strip and toolbar), `splitpick` / `splitdrop` / `splithide` (split view),
// `privatepage` (the same host's private landing over a blank private tab, docs/design/private-tabs.md).
// JS -> Rust: `{v:1,t:"split",a,...}` with exactly the fields each action takes.

/** The pair's first (`l`) or second (`r`) slot: left and right side by side, top and bottom stacked. */
export type SplitSide = 'l' | 'r';
/** A page edge: the half of the page an empty side or a drop target covers. */
export type SplitEdge = 'l' | 'r' | 't' | 'b';
/** A page corner: the quarter of the page a pane of a larger layout, or a corner drop, covers. */
export type SplitCorner = 'tl' | 'tr' | 'bl' | 'br';
/** Where a pane sits: an edge for a whole half, a corner for a quarter. */
export type SplitAt = SplitEdge | SplitCorner;
/** `h` side by side (left | right), `v` stacked (top over bottom). */
export type SplitOrientation = 'h' | 'v';

/** One pane of a layout of three or four panes. */
export interface PaneSlot {
  readonly id: number;
  readonly at: SplitAt;
}

export interface PairMsg {
  readonly t: 'pair';
  /** Wire ids of the first and second tab (left/right, or top/bottom when `o` is `v`); both 0 when the window has no pair. */
  readonly l: number;
  readonly r: number;
  readonly f: SplitSide;
  /** The pair is on screen (one of its tabs is the active tab). */
  readonly shown: boolean;
  /** Optional only for callers that build a pair by hand; the wire always carries it (absent reads as `h`). */
  readonly o?: SplitOrientation;
  /**
   * Three or four panes only: every pane in tree order (`l` and `r` are then the halves' lead tabs).
   * A pair never carries it.
   */
  readonly panes?: readonly PaneSlot[];
  /** With `panes`: where the focused pane sits. */
  readonly fp?: SplitAt;
}
export interface PickTab {
  readonly id: number;
  readonly title: string;
  readonly host: string;
  /** `''` or a validated PNG data URI. */
  readonly fav: string;
  readonly private: boolean;
}
export interface PickClosed {
  readonly n: number;
  readonly title: string;
  readonly host: string;
}
export interface SplitPickMsg {
  readonly t: 'splitpick';
  readonly side: SplitAt;
  readonly tabs: readonly PickTab[];
  readonly closed: readonly PickClosed[];
}
export interface SplitDropMsg { readonly t: 'splitdrop'; readonly side: SplitAt; readonly title: string }
export interface SplitHideMsg { readonly t: 'splithide' }
/** The private landing over blank private tab `id` (wire id); `n` private tabs are open (>= 1). */
export interface PrivatePageMsg { readonly t: 'privatepage'; readonly id: number; readonly n: number }
export type SplitInbound = PairMsg | SplitPickMsg | SplitDropMsg | SplitHideMsg | PrivatePageMsg;

export type SplitCommand =
  | { v: 1; t: 'split'; a: 'pick'; id: number }
  | { v: 1; t: 'split'; a: 'reopen'; n: number }
  | { v: 1; t: 'split'; a: 'go'; text: string }
  | { v: 1; t: 'split'; a: 'close' }
  | { v: 1; t: 'split'; a: 'focus' }
  | { v: 1; t: 'split'; a: 'closeprivate' }
  | { v: 1; t: 'split'; a: 'over'; id: number; x: number; y: number }
  | { v: 1; t: 'split'; a: 'drop'; id: number }
  | { v: 1; t: 'split'; a: 'end'; id: number };

/** Limits shared with `src/chrome_host/split.rs` and `protocol.rs`. */
export const MAX_PICK_TABS = 100;
export const MAX_PICK_CLOSED = 8;
export const MAX_SPLIT_COORD = 16384;
/** Most private tabs a `privatepage` may report (shared with `src/chrome_host/split.rs`). */
export const MAX_PRIVATE_TABS = 10000;
const MAX_ID = 0xffffffff;
const MAX_TITLE = 512;
const MAX_HOST = 253;
const MAX_TEXT = 2048;
const FAV_PREFIX = 'data:image/png;base64,';
const MAX_FAV_URI = 24 * 1024;
const BASE64_RE = /^[A-Za-z0-9+/]+={0,2}$/;

const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const isInt = (v: unknown, min: number, max: number): v is number => typeof v === 'number' && Number.isInteger(v) && v >= min && v <= max;
const isStr = (v: unknown, max: number): v is string => typeof v === 'string' && v.length <= max;
const isBool = (v: unknown): v is boolean => typeof v === 'boolean';
const isSide = (v: unknown): v is SplitSide => v === 'l' || v === 'r';
const isEdge = (v: unknown): v is SplitEdge => v === 'l' || v === 'r' || v === 't' || v === 'b';
const isOrientation = (v: unknown): v is SplitOrientation => v === 'h' || v === 'v';
const isAt = (v: unknown): v is SplitAt => isEdge(v) || v === 'tl' || v === 'tr' || v === 'bl' || v === 'br';
/** The page quarters (`tl`, `tr`, `bl`, `br` as bits 1, 2, 4, 8) a pane at `at` covers. */
const QUARTERS: Readonly<Record<SplitAt, number>> = { l: 1 | 4, r: 2 | 8, t: 1 | 2, b: 4 | 8, tl: 1, tr: 2, bl: 4, br: 8 };

/** A layout's panes: 3 or 4 distinct tabs whose places tile the page exactly once. */
function parsePanes(v: unknown): PaneSlot[] | null {
  if (!Array.isArray(v) || v.length < 3 || v.length > 4) return null;
  const out: PaneSlot[] = [];
  let covered = 0;
  for (const slot of v) {
    if (!isRecord(slot) || !exactKeys(slot, ['id', 'at'])) return null;
    const { id, at } = slot;
    if (!isInt(id, 1, MAX_ID) || !isAt(at) || out.some((p) => p.id === id) || (covered & QUARTERS[at]) !== 0) return null;
    covered |= QUARTERS[at];
    out.push({ id, at });
  }
  return covered === 15 ? out : null;
}
const exactKeys = (v: Record<string, unknown>, keys: readonly string[]): boolean => {
  const own = Object.keys(v);
  return own.length === keys.length && keys.every((k) => own.includes(k));
};
const isFav = (v: unknown): v is string =>
  v === '' || (isStr(v, MAX_FAV_URI) && v.startsWith(FAV_PREFIX) && BASE64_RE.test(v.slice(FAV_PREFIX.length)));

function parsePickTab(v: unknown, seen: Set<number>): PickTab | null {
  if (!isRecord(v) || !exactKeys(v, ['id', 'title', 'host', 'fav', 'private'])) return null;
  const { id, title, host, fav, private: priv } = v;
  if (!isInt(id, 1, MAX_ID) || seen.has(id) || !isStr(title, MAX_TITLE) || !isStr(host, MAX_HOST) || !isFav(fav) || !isBool(priv)) return null;
  seen.add(id);
  return { id, title, host, fav, private: priv };
}

function parsePickClosed(v: unknown, index: number): PickClosed | null {
  if (!isRecord(v) || !exactKeys(v, ['n', 'title', 'host'])) return null;
  const { n, title, host } = v;
  if (n !== index || !isStr(title, MAX_TITLE) || !isStr(host, MAX_HOST)) return null;
  return { n, title, host };
}

/** Strict parser for the split messages; anything unexpected is `null`. */
export function parseSplitInbound(raw: Record<string, unknown>): SplitInbound | null {
  if (raw.v !== 1) return null;
  switch (raw.t) {
    case 'pair': {
      const layout = exactKeys(raw, ['v', 't', 'l', 'r', 'f', 'shown', 'o', 'panes', 'fp']);
      if (!layout && !exactKeys(raw, ['v', 't', 'l', 'r', 'f', 'shown', 'o'])) return null;
      const { l, r, f, shown, o } = raw;
      if (!isInt(l, 0, MAX_ID) || !isInt(r, 0, MAX_ID) || !isSide(f) || !isBool(shown) || !isOrientation(o)) return null;
      // Either no pair (both 0, not shown) or two different tabs.
      if ((l === 0) !== (r === 0) || (l !== 0 && l === r) || (l === 0 && shown)) return null;
      if (!layout) return { t: 'pair', l, r, f, shown, o };
      // Three or four panes: the halves' lead tabs are among them, and the focused place is one of theirs.
      const panes = parsePanes(raw.panes);
      const fp = raw.fp;
      if (l === 0 || panes === null || !isAt(fp) || !panes.some((p) => p.at === fp) || !panes.some((p) => p.id === l) || !panes.some((p) => p.id === r)) return null;
      return { t: 'pair', l, r, f, shown, o, panes, fp };
    }
    case 'splitpick': {
      if (!exactKeys(raw, ['v', 't', 'side', 'tabs', 'closed'])) return null;
      const { side, tabs, closed } = raw;
      if (!isAt(side) || !Array.isArray(tabs) || tabs.length > MAX_PICK_TABS || !Array.isArray(closed) || closed.length > MAX_PICK_CLOSED) return null;
      const seen = new Set<number>();
      const outTabs: PickTab[] = [];
      for (const v of tabs) {
        const tab = parsePickTab(v, seen);
        if (!tab) return null;
        outTabs.push(tab);
      }
      const outClosed: PickClosed[] = [];
      for (const [i, v] of closed.entries()) {
        const page = parsePickClosed(v, i);
        if (!page) return null;
        outClosed.push(page);
      }
      return { t: 'splitpick', side, tabs: outTabs, closed: outClosed };
    }
    case 'splitdrop': {
      if (!exactKeys(raw, ['v', 't', 'side', 'title'])) return null;
      const { side, title } = raw;
      if (!isAt(side) || !isStr(title, MAX_TITLE)) return null;
      return { t: 'splitdrop', side, title };
    }
    case 'splithide':
      return exactKeys(raw, ['v', 't']) ? { t: 'splithide' } : null;
    case 'privatepage': {
      if (!exactKeys(raw, ['v', 't', 'id', 'n'])) return null;
      const { id, n } = raw;
      if (!isInt(id, 1, MAX_ID) || !isInt(n, 1, MAX_PRIVATE_TABS)) return null;
      return { t: 'privatepage', id, n };
    }
    default:
      return null;
  }
}

/** Whether `cmd` is a well-formed split command (Rust re-validates every field and id). */
export function validSplitCommand(cmd: SplitCommand): boolean {
  const keys = Object.keys(cmd);
  if (cmd.v !== 1 || cmd.t !== 'split' || keys.length > 6) return false;
  switch (cmd.a) {
    case 'pick':
    case 'drop':
    case 'end':
      return exactKeys(cmd, ['v', 't', 'a', 'id']) && isInt(cmd.id, 1, MAX_ID);
    case 'reopen':
      return exactKeys(cmd, ['v', 't', 'a', 'n']) && isInt(cmd.n, 0, MAX_PICK_CLOSED - 1);
    case 'go':
      return exactKeys(cmd, ['v', 't', 'a', 'text']) && isStr(cmd.text, MAX_TEXT) && cmd.text.trim() !== '';
    case 'close':
    case 'focus':
    case 'closeprivate':
      return exactKeys(cmd, ['v', 't', 'a']);
    case 'over':
      return exactKeys(cmd, ['v', 't', 'a', 'id', 'x', 'y']) && isInt(cmd.id, 1, MAX_ID) && isInt(cmd.x, 0, MAX_SPLIT_COORD) && isInt(cmd.y, 0, MAX_SPLIT_COORD);
    default:
      return false;
  }
}
