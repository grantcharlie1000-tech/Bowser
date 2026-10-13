// Split view wire contract (docs/html-chrome-protocol.md, "Split view"). Pure and self-contained so
// protocol.ts can import it without a cycle and test/split.mjs can load it directly.
//
// Rust -> JS: `pair` (tab strip and toolbar), `splitpick` / `splitdrop` / `splithide` (split view).
// JS -> Rust: `{v:1,t:"split",a,...}` with exactly the fields each action takes.

export type SplitSide = 'l' | 'r';

export interface PairMsg {
  readonly t: 'pair';
  /** Wire ids of the left and right tab; both 0 when the window has no pair. */
  readonly l: number;
  readonly r: number;
  readonly f: SplitSide;
  /** The pair is on screen (one of its tabs is the active tab). */
  readonly shown: boolean;
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
  readonly side: SplitSide;
  readonly tabs: readonly PickTab[];
  readonly closed: readonly PickClosed[];
}
export interface SplitDropMsg { readonly t: 'splitdrop'; readonly side: SplitSide; readonly title: string }
export interface SplitHideMsg { readonly t: 'splithide' }
export type SplitInbound = PairMsg | SplitPickMsg | SplitDropMsg | SplitHideMsg;

export type SplitCommand =
  | { v: 1; t: 'split'; a: 'pick'; id: number }
  | { v: 1; t: 'split'; a: 'reopen'; n: number }
  | { v: 1; t: 'split'; a: 'go'; text: string }
  | { v: 1; t: 'split'; a: 'close' }
  | { v: 1; t: 'split'; a: 'focus' }
  | { v: 1; t: 'split'; a: 'over'; id: number; x: number; y: number }
  | { v: 1; t: 'split'; a: 'drop'; id: number }
  | { v: 1; t: 'split'; a: 'end'; id: number };

/** Limits shared with `src/chrome_host/split.rs` and `protocol.rs`. */
export const MAX_PICK_TABS = 100;
export const MAX_PICK_CLOSED = 8;
export const MAX_SPLIT_COORD = 16384;
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
      if (!exactKeys(raw, ['v', 't', 'l', 'r', 'f', 'shown'])) return null;
      const { l, r, f, shown } = raw;
      if (!isInt(l, 0, MAX_ID) || !isInt(r, 0, MAX_ID) || !isSide(f) || !isBool(shown)) return null;
      // Either no pair (both 0, not shown) or two different tabs.
      if ((l === 0) !== (r === 0) || (l !== 0 && l === r) || (l === 0 && shown)) return null;
      return { t: 'pair', l, r, f, shown };
    }
    case 'splitpick': {
      if (!exactKeys(raw, ['v', 't', 'side', 'tabs', 'closed'])) return null;
      const { side, tabs, closed } = raw;
      if (!isSide(side) || !Array.isArray(tabs) || tabs.length > MAX_PICK_TABS || !Array.isArray(closed) || closed.length > MAX_PICK_CLOSED) return null;
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
      if (!isSide(side) || !isStr(title, MAX_TITLE)) return null;
      return { t: 'splitdrop', side, title };
    }
    case 'splithide':
      return exactKeys(raw, ['v', 't']) ? { t: 'splithide' } : null;
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
      return exactKeys(cmd, ['v', 't', 'a']);
    case 'over':
      return exactKeys(cmd, ['v', 't', 'a', 'id', 'x', 'y']) && isInt(cmd.id, 1, MAX_ID) && isInt(cmd.x, 0, MAX_SPLIT_COORD) && isInt(cmd.y, 0, MAX_SPLIT_COORD);
    default:
      return false;
  }
}
