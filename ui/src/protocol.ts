// Typed bridge for the toolbar, tab strip and left rail pages. Mirrors docs/html-chrome-protocol.md (v1).
// Page-controlled text reaches the DOM only through textContent or attributes (see view.ts, tabs.ts, rail.ts).
import { MAX_RAIL_ITEMS, isRailIcon, isRailKey, type RailIconName } from './rail-logic';

export const PROTOCOL_VERSION = 1;
export const MAX_TEXT = 2048;
const MAX_LABEL = 512;
/** Rail button label (also its tooltip and letter-tile source). */
const MAX_RAIL_LABEL = 64;
const MAX_COLOR = 64;
const MAX_WIRE = 16 * 1024;
const MAX_INBOUND = 256 * 1024;
const MAX_SEQ = 0x7fffffff;
export const MAX_ID = 0xffffffff;
export const MAX_TITLE = 512;
/** Most items in one `tabs` message; Rust sends at most this many. */
export const MAX_TABS = 512;
const FAV_PREFIX = 'data:image/png;base64,';
export const MAX_FAV_URI = 24 * 1024;
const BASE64_RE = /^[A-Za-z0-9+/]+={0,2}$/;

export type NavAction = 'back' | 'forward' | 'reload' | 'stop';
export type OpenTarget = 'menu' | 'site' | 'newtab' | 'split';
export type CaptionAction = 'toggleMaximize' | 'systemMenu';
export type Security = 'secure' | 'notsecure' | 'certproblem' | 'internal' | 'unknown' | 'none';
export type TabAction = 'select' | 'close' | 'menu' | 'pin' | 'mute';
export type GroupAction = 'toggle' | 'menu';
export type RailAction = 'click' | 'menu';
export type PanelKind = 'panel' | 'hist' | 'bm' | 'cust' | 'notes' | 'set' | 'priv';
export type PanelToken = 'history' | 'bookmarks' | 'customize' | 'notes' | 'settings' | 'privacy';
export interface PanelFields {
  a: string;
  id?: number;
  parent?: number;
  before?: number;
  n?: number;
  on?: boolean;
  text?: string;
  url?: string;
}

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
  | { v: 1; t: 'group'; a: GroupAction; key: number }
  // --- vertical sidebar (lane S) ---
  | { v: 1; t: 'side'; a: 'workspace' | 'newtab' }
  // --- end vertical sidebar ---
  | { v: 1; t: 'rail'; a: RailAction; k: string }
  | { v: 1; t: 'railmove'; k: string; before: string }
  | ({ v: 1; t: PanelKind } & PanelFields)
  | { v: 1; t: 'size'; tok: number; w: number; h: number }
  | { v: 1; t: 'pick'; tok: number; id: number }
  | { v: 1; t: 'dismiss'; tok: number };

// Rust -> JS messages (validated by parseInbound).
export interface StateMsg {
  readonly t: 'state';
  readonly scale: number;
  readonly pillx?: number;
  readonly pillw?: number;
  readonly menux?: number;
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
export interface RailItem {
  /** Rail key (see RAIL_KEY_RE in rail-logic.ts). */
  readonly k: string;
  readonly icon: RailIconName;
  readonly label: string;
  readonly active: boolean;
  readonly dot: boolean;
  /** Safe CSS colour, or '' for the theme accent. */
  readonly accent: string;
  /** A favicon is cached for this key (see the `railfav` message). */
  readonly fav: boolean;
}
export interface RailMsg {
  readonly t: 'rail';
  readonly items: readonly RailItem[];
}
export interface RailFavMsg {
  readonly t: 'railfav';
  readonly k: string;
  readonly uri: string;
}
export interface PanelShowMsg { readonly t: 'panelshow'; readonly p: PanelToken }
export interface LayoutMsg { readonly t: 'layout'; readonly mode: 'vertical' | 'horizontal' }
// --- vertical sidebar (lane S) ---
export interface WorkspaceMsg { readonly t: 'workspace'; readonly name: string; readonly icon: RailIconName }
// --- end vertical sidebar ---
export type SnapshotValue = string | number | boolean | null | readonly SnapshotValue[] | { readonly [key: string]: SnapshotValue };
export interface SnapshotMsg { readonly t: 'snap'; readonly p: PanelToken; readonly d: { readonly [key: string]: SnapshotValue } }
export type Inbound =
  | StateMsg
  | SetTextMsg
  | FocusOmniboxMsg
  | CompletionMsg
  | TabsMsg
  | FavMsg
  | RailMsg
  | RailFavMsg
  | PanelShowMsg
  | LayoutMsg
  // --- vertical sidebar (lane S) ---
  | WorkspaceMsg
  // --- end vertical sidebar ---
  | SnapshotMsg;

interface WebViewLike {
  postMessage(message: string): void;
  addEventListener(type: 'message', listener: (ev: { readonly data: unknown }) => void): void;
}

declare global {
  interface Window {
    chrome?: { webview?: WebViewLike };
  }
}

// Exactly the colours `is_safe_css_colour` accepts in Rust: `#` with 3, 4, 6 or 8 hex digits, or
// `rgb(`/`rgba(` over digits, commas, dots, spaces and percent signs with at least one digit.
const HEX_COLOR_RE = /^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/;
const RGB_COLOR_RE = /^rgba?\((?=[0-9,. %]*[0-9])[0-9,. %]*\)$/;

function isSafeColour(v: string): boolean {
  return HEX_COLOR_RE.test(v) || RGB_COLOR_RE.test(v);
}

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

function hasAllKeys(v: Record<string, unknown>, keys: readonly string[]): boolean {
  return keys.every((k) => k in v);
}

const SECURITY: readonly Security[] = ['secure', 'notsecure', 'certproblem', 'internal', 'unknown', 'none'];
const PANEL_TOKENS: readonly PanelToken[] = ['history', 'bookmarks', 'customize', 'notes', 'settings', 'privacy'];
const PANEL_ACTION_RE = /^[a-z]{1,24}$/;
const MAX_SNAPSHOT_DEPTH = 8;
const MAX_SNAPSHOT_KEYS = 4096;
const MAX_SNAPSHOT_OBJECT_KEYS = 128;
const MAX_SNAPSHOT_ARRAY_ITEMS = 4096;

/** Only validated, flat commands can cross to the Rust panel parser. */
export function sendPanel(t: PanelKind, fields: PanelFields): boolean {
  if (!PANEL_ACTION_RE.test(fields.a)) return false;
  const keys = Object.keys(fields);
  if (keys.length > 4 || keys.some(k => !['a', 'id', 'parent', 'before', 'n', 'on', 'text', 'url'].includes(k))) return false;
  for (const key of ['id', 'parent', 'before'] as const) {
    const value = fields[key];
    if (value !== undefined && !isInt(value, 0, Number.MAX_SAFE_INTEGER)) return false;
  }
  if (fields.n !== undefined && !isInt(fields.n, -0x80000000, 0x7fffffff)) return false;
  if (fields.on !== undefined && !isBool(fields.on)) return false;
  if (fields.text !== undefined && !isStr(fields.text, MAX_TEXT)) return false;
  if (fields.url !== undefined && !isStr(fields.url, MAX_TEXT)) return false;
  return send({ v: 1, t, ...fields });
}

// --- vertical sidebar (lane S) ---
export function sendSide(a: 'workspace' | 'newtab'): boolean {
  return (a === 'workspace' || a === 'newtab') && send({ v: 1, t: 'side', a });
}
// --- end vertical sidebar ---

export function sendPopupSize(tok: number, w: number, h: number): boolean {
  return isInt(tok, 0, MAX_ID) && isInt(w, 1, 4000) && isInt(h, 1, 4000) && send({ v: 1, t: 'size', tok, w, h });
}
export function sendPopupPick(tok: number, id: number): boolean {
  return isInt(tok, 0, MAX_ID) && isInt(id, 0, MAX_ID) && send({ v: 1, t: 'pick', tok, id });
}
export function sendPopupDismiss(tok: number): boolean {
  return isInt(tok, 0, MAX_ID) && send({ v: 1, t: 'dismiss', tok });
}

function validSnapshot(value: unknown, depth: number, budget: { keys: number }): value is SnapshotValue {
  if (depth > MAX_SNAPSHOT_DEPTH) return false;
  if (value === null || typeof value === 'string' || typeof value === 'boolean') return true;
  if (typeof value === 'number') return Number.isFinite(value);
  if (Array.isArray(value)) {
    if (value.length > MAX_SNAPSHOT_ARRAY_ITEMS) return false;
    return value.every(item => validSnapshot(item, depth + 1, budget));
  }
  if (!isRecord(value)) return false;
  const keys = Object.keys(value);
  budget.keys += keys.length;
  if (keys.length > MAX_SNAPSHOT_OBJECT_KEYS || budget.keys > MAX_SNAPSHOT_KEYS) return false;
  return keys.every(key => validSnapshot(value[key], depth + 1, budget));
}

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
    if (isStr(colour, MAX_COLOR) && isSafeColour(colour)) out[token] = colour;
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

const RAIL_ITEM_KEYS = ['k', 'icon', 'label', 'active', 'dot', 'accent', 'fav'];

/** A PNG data: URI with base64 payload, at most MAX_FAV_URI characters. */
function isFavUri(v: unknown): v is string {
  if (!isStr(v, MAX_FAV_URI) || !v.startsWith(FAV_PREFIX)) return false;
  return BASE64_RE.test(v.slice(FAV_PREFIX.length));
}

/** Strict validation of one `rail` item: every key present, none extra, key unique within the message. */
function parseRailItem(v: unknown, seen: ReadonlySet<string>): RailItem | null {
  if (!isRecord(v) || !hasOnlyKeys(v, RAIL_ITEM_KEYS) || !hasAllKeys(v, RAIL_ITEM_KEYS)) return null;
  const { k, icon, label, active, dot, accent, fav } = v;
  if (typeof k !== 'string' || !isRailKey(k) || seen.has(k)) return null;
  if (!isRailIcon(icon) || !isStr(label, MAX_RAIL_LABEL) || !isBool(active) || !isBool(dot) || !isBool(fav)) return null;
  if (!isStr(accent, MAX_COLOR) || (accent !== '' && !isSafeColour(accent))) return null;
  return { k, icon, label, active, dot, accent, fav };
}

/** Strictly validates a host message. Accepts an object or its JSON text; returns null for anything unexpected. */
export function parseInbound(data: unknown): Inbound | null {
  let raw: unknown = data;
  if (typeof raw === 'string') {
    if (new TextEncoder().encode(raw).length > MAX_INBOUND) return null;
    try {
      raw = JSON.parse(raw);
    } catch {
      return null;
    }
  }
  if (!isRecord(raw)) return null;
  if (typeof data !== 'string') {
    try { if (new TextEncoder().encode(JSON.stringify(raw)).length > MAX_INBOUND) return null; }
    catch { return null; }
  }

  switch (raw.t) {
    case 'panelshow': {
      if (!hasOnlyKeys(raw, ['v', 't', 'p']) || raw.v !== 1 || !isOneOf(raw.p, PANEL_TOKENS)) return null;
      return { t: 'panelshow', p: raw.p };
    }
    case 'layout': {
      if (!hasOnlyKeys(raw, ['v', 't', 'mode']) || raw.v !== 1 || !isOneOf(raw.mode, ['vertical', 'horizontal'] as const)) return null;
      return { t: 'layout', mode: raw.mode };
    }
    // --- vertical sidebar (lane S) ---
    case 'workspace': {
      if (!hasOnlyKeys(raw, ['v', 't', 'name', 'icon']) || raw.v !== 1 || !isStr(raw.name, MAX_RAIL_LABEL) || !isRailIcon(raw.icon) || !raw.icon.startsWith('g:')) return null;
      return { t: 'workspace', name: raw.name, icon: raw.icon };
    }
    // --- end vertical sidebar ---
    case 'snap': {
      if (!hasOnlyKeys(raw, ['v', 't', 'p', 'd']) || raw.v !== 1 || !isOneOf(raw.p, PANEL_TOKENS) || !isRecord(raw.d)) return null;
      const budget = { keys: 0 };
      if (!validSnapshot(raw.d, 1, budget)) return null;
      return { t: 'snap', p: raw.p, d: raw.d as SnapshotMsg['d'] };
    }
    case 'state': {
      if (!hasOnlyKeys(raw, ['t', 'scale', 'pillx', 'pillw', 'menux', 'solid', 'dark', 'back', 'forward', 'loading', 'progress', 'security', 'label', 'theme'])) return null;
      const { scale, solid, dark, back, forward, loading, progress, security, label } = raw;
      if (typeof scale !== 'number' || !Number.isFinite(scale)) return null;
      const placementKeys = ['pillx', 'pillw', 'menux'] as const;
      const placementCount = placementKeys.filter((key) => key in raw).length;
      if (placementCount !== 0 && placementCount !== placementKeys.length) return null;
      if (placementCount !== 0 && placementKeys.some((key) => !isInt(raw[key], 0, Number.MAX_SAFE_INTEGER))) return null;
      if (!isBool(solid) || !isBool(dark) || !isBool(back) || !isBool(forward) || !isBool(loading)) return null;
      if (!isInt(progress, 0, 100)) return null;
      if (!isOneOf(security, SECURITY)) return null;
      if (!isStr(label, MAX_LABEL)) return null;
      return {
        t: 'state',
        scale,
        ...(placementCount === 3 ? { pillx: raw.pillx as number, pillw: raw.pillw as number, menux: raw.menux as number } : {}),
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
      if (typeof text !== 'string' || !isBool(select)) return null;
      return { t: 'setText', text: truncateCodePoint(text, MAX_TEXT), select };
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
      if (!isInt(id, 0, MAX_ID) || !isFavUri(uri)) return null;
      return { t: 'fav', id, uri };
    }
    case 'rail': {
      if (!hasOnlyKeys(raw, ['v', 't', 'items']) || raw.v !== 1) return null;
      const { items } = raw;
      if (!Array.isArray(items) || items.length > MAX_RAIL_ITEMS) return null;
      const seen = new Set<string>();
      const out: RailItem[] = [];
      for (const v of items) {
        const item = parseRailItem(v, seen);
        if (!item) return null;
        seen.add(item.k);
        out.push(item);
      }
      return { t: 'rail', items: out };
    }
    case 'railfav': {
      if (!hasOnlyKeys(raw, ['v', 't', 'k', 'uri']) || raw.v !== 1) return null;
      const { k, uri } = raw;
      if (typeof k !== 'string' || !isRailKey(k) || !isFavUri(uri)) return null;
      return { t: 'railfav', k, uri };
    }
    default:
      return null;
  }
}

function truncateCodePoint(text: string, max: number): string {
  if (text.length <= max) return text;
  let end = max;
  const last = text.charCodeAt(end - 1);
  if (last >= 0xd800 && last <= 0xdbff) end -= 1;
  return text.slice(0, end);
}
