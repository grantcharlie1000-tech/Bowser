// Typed bridge for the toolbar and tab strip pages. Mirrors docs/html-chrome-protocol.md (v1).
// Page-controlled text reaches the DOM only through textContent (see view.ts and tabs.ts).

export const PROTOCOL_VERSION = 1;
export const MAX_TEXT = 2048;
const MAX_LABEL = 512;
const MAX_COLOR = 64;
const MAX_WIRE = 16 * 1024;
const MAX_INBOUND = 256 * 1024;
const MAX_SEQ = 0x7fffffff;
export const MAX_ID = 0xffffffff;
export const MAX_TITLE = 512;
const MAX_TABS = 512;
const FAV_PREFIX = 'data:image/png;base64,';
export const MAX_FAV_URI = 24 * 1024;
const BASE64_RE = /^[A-Za-z0-9+/]+={0,2}$/;

export type NavAction = 'back' | 'forward' | 'reload' | 'stop';
export type OpenTarget = 'menu' | 'site' | 'newtab';
export type CaptionAction = 'toggleMaximize' | 'systemMenu';
export type Security = 'secure' | 'notsecure' | 'certproblem' | 'internal' | 'unknown' | 'none';
export type TabAction = 'select' | 'close' | 'menu' | 'pin' | 'mute';
export type GroupAction = 'toggle' | 'menu';

export const THEME_TOKENS = [
  'bg',
  'fill',
  'border',
  'focusBorder',
  'text',
  'placeholder',
  'completion',
  'selection',
  'hover',
  'progress',
] as const;
export type ThemeToken = (typeof THEME_TOKENS)[number];
export type Theme = Partial<Record<ThemeToken, string>>;

// JS -> Rust commands (sent as flat JSON text).
export type Command =
  | { v: 1; t: 'ready' }
  | { v: 1; t: 'nav'; a: NavAction }
  | { v: 1; t: 'edit'; seq: number; text: string }
  | { v: 1; t: 'submit'; text: string }
  | { v: 1; t: 'escape' }
  | { v: 1; t: 'focus'; on: boolean }
  | { v: 1; t: 'open'; a: OpenTarget }
  | { v: 1; t: 'drag' }
  | { v: 1; t: 'caption'; a: CaptionAction }
  | { v: 1; t: 'tab'; a: TabAction; id: number }
  | { v: 1; t: 'tabmove'; id: number; before: number }
  | { v: 1; t: 'group'; a: GroupAction; key: number };

// Rust -> JS messages (validated by parseInbound).
export interface StateMsg {
  readonly t: 'state';
  readonly scale: number;
  readonly solid: boolean;
  readonly dark: boolean;
  readonly back: boolean;
  readonly forward: boolean;
  readonly loading: boolean;
  readonly progress: number;
  readonly security: Security;
  readonly label: string;
  readonly theme: Theme;
}
export interface SetTextMsg {
  readonly t: 'setText';
  readonly text: string;
  readonly select: boolean;
}
export interface FocusOmniboxMsg {
  readonly t: 'focusOmnibox';
  readonly select: boolean;
}
export interface CompletionMsg {
  readonly t: 'completion';
  readonly seq: number;
  readonly suffix: string;
}
export interface TabItem {
  readonly id: number;
  readonly title: string;
  readonly loading: boolean;
  readonly pinned: boolean;
  readonly muted: boolean;
  /** Marker colour index 0-7, or -1 for none. */
  readonly marker: number;
  /** Group key, 0 when ungrouped. */
  readonly g: number;
  /** Group leader (the item that carries the group fields). */
  readonly gl: boolean;
  readonly gname?: string;
  /** Island colour index 0-7, or -1 for the theme accent. Leader only. */
  readonly gcol?: number;
  readonly gcollapsed?: boolean;
  readonly gsize?: number;
  /** A favicon is cached for this id (see the `fav` message). */
  readonly fav: boolean;
}
export interface TabsMsg {
  readonly t: 'tabs';
  /** Active tab id, 0 when none. */
  readonly active: number;
  readonly items: readonly TabItem[];
}
export interface FavMsg {
  readonly t: 'fav';
  readonly id: number;
  readonly uri: string;
}
export type Inbound = StateMsg | SetTextMsg | FocusOmniboxMsg | CompletionMsg | TabsMsg | FavMsg;

interface WebViewLike {
  postMessage(message: string): void;
  addEventListener(type: 'message', listener: (ev: { readonly data: unknown }) => void): void;
}

declare global {
  interface Window {
    chrome?: { webview?: WebViewLike };
  }
}

const COLOR_RE = /^[#(),.%+\-\w\s]+$/;

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

function isBool(v: unknown): v is boolean {
  return typeof v === 'boolean';
}

function isStr(v: unknown, max: number): v is string {
  return typeof v === 'string' && v.length <= max;
}

function isInt(v: unknown, min: number, max: number): v is number {
  return typeof v === 'number' && Number.isInteger(v) && v >= min && v <= max;
}

function isOneOf<T extends string>(v: unknown, list: readonly T[]): v is T {
  return typeof v === 'string' && (list as readonly string[]).includes(v);
}

function hasOnlyKeys(v: Record<string, unknown>, keys: readonly string[]): boolean {
  return Object.keys(v).every((k) => keys.includes(k));
}

const SECURITY: readonly Security[] = ['secure', 'notsecure', 'certproblem', 'internal', 'unknown', 'none'];

/** Sends one command. Returns false when the host bridge is missing or the message is too large. */
export function send(cmd: Command): boolean {
  const wv = window.chrome?.webview;
  if (!wv) return false;
  const wire = JSON.stringify(cmd);
  if (wire.length > MAX_WIRE) return false;
  wv.postMessage(wire);
  return true;
}

/** Registers the listener for host messages; invalid or unknown messages are dropped. */
export function onInbound(listener: (msg: Inbound) => void): void {
  const wv = window.chrome?.webview;
  if (!wv) return;
  wv.addEventListener('message', (ev) => {
    const msg = parseInbound(ev.data);
    if (msg) listener(msg);
  });
}

function parseTheme(v: unknown): Theme {
  const out: Theme = {};
  if (!isRecord(v)) return out;
  for (const token of THEME_TOKENS) {
    const colour = v[token];
    if (isStr(colour, MAX_COLOR) && COLOR_RE.test(colour)) out[token] = colour;
  }
  return out;
}

const TAB_KEYS = ['id', 'title', 'loading', 'pinned', 'muted', 'marker', 'g', 'gl', 'fav', 'gname', 'gcol', 'gcollapsed', 'gsize'];
const LEADER_KEYS = ['gname', 'gcol', 'gcollapsed', 'gsize'];

/** Strict validation of one `tabs` item. Ids must be new within the message (`seen`). */
function parseTabItem(v: unknown, seen: ReadonlySet<number>): TabItem | null {
  if (!isRecord(v) || !hasOnlyKeys(v, TAB_KEYS)) return null;
  const { id, title, loading, pinned, muted, marker, g, gl, fav } = v;
  if (!isInt(id, 0, MAX_ID) || seen.has(id)) return null;
  if (!isStr(title, MAX_TITLE) || !isBool(loading) || !isBool(pinned) || !isBool(muted) || !isBool(fav)) return null;
  if (!isInt(marker, -1, 7) || !isInt(g, 0, MAX_ID) || !isBool(gl)) return null;
  const base = { id, title, loading, pinned, muted, marker, g, gl, fav };
  if (!gl) {
    // Group fields are leader-only: a member must not carry them.
    if (LEADER_KEYS.some((k) => k in v)) return null;
    return base;
  }
  const { gname, gcol, gcollapsed, gsize } = v;
  if (g === 0) return null;
  if (!isStr(gname, MAX_TITLE) || !isInt(gcol, -1, 7) || !isBool(gcollapsed) || !isInt(gsize, 1, MAX_ID)) return null;
  return { ...base, gname, gcol, gcollapsed, gsize };
}

/** Strictly validates a host message. Accepts an object or its JSON text; returns null for anything unexpected. */
export function parseInbound(data: unknown): Inbound | null {
  let raw: unknown = data;
  if (typeof raw === 'string') {
    if (raw.length > MAX_INBOUND) return null;
    try {
      raw = JSON.parse(raw);
    } catch {
      return null;
    }
  }
  if (!isRecord(raw)) return null;

  switch (raw.t) {
    case 'state': {
      const { scale, solid, dark, back, forward, loading, progress, security, label } = raw;
      if (typeof scale !== 'number' || !Number.isFinite(scale)) return null;
      if (!isBool(solid) || !isBool(dark) || !isBool(back) || !isBool(forward) || !isBool(loading)) return null;
      if (!isInt(progress, 0, 100)) return null;
      if (!isOneOf(security, SECURITY)) return null;
      if (!isStr(label, MAX_LABEL)) return null;
      return {
        t: 'state',
        scale,
        solid,
        dark,
        back,
        forward,
        loading,
        progress,
        security,
        label,
        theme: parseTheme(raw.theme),
      };
    }
    case 'setText': {
      const { text, select } = raw;
      if (!isStr(text, MAX_TEXT) || !isBool(select)) return null;
      return { t: 'setText', text, select };
    }
    case 'focusOmnibox': {
      const { select } = raw;
      if (!isBool(select)) return null;
      return { t: 'focusOmnibox', select };
    }
    case 'completion': {
      const { seq, suffix } = raw;
      if (!isInt(seq, 0, MAX_SEQ) || !isStr(suffix, MAX_TEXT)) return null;
      return { t: 'completion', seq, suffix };
    }
    case 'tabs': {
      if (!hasOnlyKeys(raw, ['v', 't', 'active', 'items']) || raw.v !== 1) return null;
      const { active, items } = raw;
      if (!isInt(active, 0, MAX_ID) || !Array.isArray(items) || items.length > MAX_TABS) return null;
      const seen = new Set<number>();
      const out: TabItem[] = [];
      for (const v of items) {
        const item = parseTabItem(v, seen);
        if (!item) return null;
        seen.add(item.id);
        out.push(item);
      }
      return { t: 'tabs', active, items: out };
    }
    case 'fav': {
      if (!hasOnlyKeys(raw, ['v', 't', 'id', 'uri']) || raw.v !== 1) return null;
      const { id, uri } = raw;
      if (!isInt(id, 0, MAX_ID) || !isStr(uri, MAX_FAV_URI) || !uri.startsWith(FAV_PREFIX)) return null;
      if (!BASE64_RE.test(uri.slice(FAV_PREFIX.length))) return null;
      return { t: 'fav', id, uri };
    }
    default:
      return null;
  }
}
