// Tab ghost wire format (docs/design/tab-drag.md, docs/html-chrome-protocol.md "Tab drag ghost").
// No runtime imports, so test/tab-drag.mjs and protocol.ts can both load it.
//
// While a tab is held, Rust shows a native layered "ghost" of it that follows the cursor in screen
// coordinates, because the strip's WebView2 cannot paint outside its own row. The page says when the
// drag starts, whether it is attached to the strip or floating, and how it ends; Rust answers when the
// ghost is up (`shown`), when it has landed (`land`, the page shows its own tab again) and when it was
// taken down without landing (`gone`).

/** What the ghost looks like: a strip tab, a sidebar row or a pinned sidebar tile. */
export type GhostKind = 'tab' | 'row' | 'tile';
/** How a drag ends: glide to (x, y), fade where it is (split drop), or vanish at once. */
export type GhostEnd = 'settle' | 'drop' | 'none';

/**
 * Messages stay within Rust's six-field cap, so `start` packs `r` = "w,h,gx,gy,c" (CSS px, c is 1
 * when the close glyph shows) and a settling `end` packs `p` = "x,y" (the slot's top-left).
 */
export type TabDragCommand =
  | { v: 1; t: 'tabdrag'; a: 'start'; id: number; k: GhostKind; r: string }
  | { v: 1; t: 'tabdrag'; a: 'move'; id: number; f: boolean }
  | { v: 1; t: 'tabdrag'; a: 'end'; id: number; m: 'settle' | 'drop'; p: string }
  | { v: 1; t: 'tabdrag'; a: 'end'; id: number; m: 'none' };

export interface TabGhostMsg {
  readonly t: 'tabghost';
  readonly a: 'shown' | 'land' | 'gone';
  readonly id: number;
}

/** Largest ghost side Rust accepts, in CSS pixels (`MAX_GHOST_SIDE`). */
export const MAX_GHOST_SIDE = 4096;
/** Largest settle coordinate Rust accepts, in CSS pixels of the tab view (`MAX_GHOST_COORD`). */
export const MAX_GHOST_COORD = 16384;
const MAX_ID = 2 ** 31 - 1;

const isInt = (v: unknown, lo: number, hi: number): v is number =>
  typeof v === 'number' && Number.isInteger(v) && v >= lo && v <= hi;

/** `count` comma-separated whole numbers in [0, max], or null. */
function unpack(raw: unknown, count: number, max: number): number[] | null {
  if (typeof raw !== 'string') return null;
  const parts = raw.split(',');
  if (parts.length !== count || !parts.every((part) => /^\d{1,5}$/.test(part))) return null;
  const values = parts.map(Number);
  return values.every((v) => v <= max) ? values : null;
}

/** The `start` command for a `w` x `h` CSS px tab grabbed at (gx, gy). */
export function startCommand(id: number, k: GhostKind, w: number, h: number, gx: number, gy: number, close: boolean): TabDragCommand {
  return { v: 1, t: 'tabdrag', a: 'start', id, k, r: `${w},${h},${gx},${gy},${close ? 1 : 0}` };
}

/** The settling `end` command: glide to (x, y), CSS px of this view. */
export function settleCommand(id: number, x: number, y: number): TabDragCommand {
  return { v: 1, t: 'tabdrag', a: 'end', id, m: 'settle', p: `${x},${y}` };
}

/**
 * Released over the page: Rust fades the ghost when a split drop zone is showing, and otherwise
 * glides it home to (x, y), the tab's slot (nothing opens there).
 */
export function dropCommand(id: number, x: number, y: number): TabDragCommand {
  return { v: 1, t: 'tabdrag', a: 'end', id, m: 'drop', p: `${x},${y}` };
}

/** Whether `cmd` is a well-formed tab drag command (what Rust's parser accepts). */
export function validTabDragCommand(cmd: TabDragCommand): boolean {
  if (cmd.v !== 1 || cmd.t !== 'tabdrag' || !isInt(cmd.id, 1, MAX_ID)) return false;
  // Exactly the fields each action takes, as Rust's parser requires.
  const keys = Object.keys(cmd).sort().join(',');
  const wanted = cmd.a === 'start' ? 'a,id,k,r,t,v'
    : cmd.a === 'move' ? 'a,f,id,t,v'
      : cmd.a === 'end' && (cmd.m === 'settle' || cmd.m === 'drop') ? 'a,id,m,p,t,v' : 'a,id,m,t,v';
  if (keys !== wanted) return false;
  switch (cmd.a) {
    case 'start': {
      const r = unpack(cmd.r, 5, MAX_GHOST_SIDE);
      if (r === null || !(cmd.k === 'tab' || cmd.k === 'row' || cmd.k === 'tile')) return false;
      const [w = 0, h = 0, gx = 0, gy = 0, c = 0] = r;
      return w >= 1 && h >= 1 && gx <= w && gy <= h && c <= 1;
    }
    case 'move':
      return typeof cmd.f === 'boolean';
    case 'end':
      if (cmd.m === 'settle' || cmd.m === 'drop') return unpack(cmd.p, 2, MAX_GHOST_COORD) !== null;
      return cmd.m === 'none';
    default:
      return false;
  }
}

/** Strict parse of a `tabghost` host message (already a record); null for anything unexpected. */
export function parseTabGhost(raw: Readonly<Record<string, unknown>>): TabGhostMsg | null {
  const keys = Object.keys(raw);
  if (keys.length !== 4 || !['v', 't', 'a', 'id'].every((k) => keys.includes(k))) return null;
  if (raw.v !== 1 || raw.t !== 'tabghost') return null;
  if (raw.a !== 'shown' && raw.a !== 'land' && raw.a !== 'gone') return null;
  if (!isInt(raw.id, 1, MAX_ID)) return null;
  return { t: 'tabghost', a: raw.a, id: raw.id };
}
