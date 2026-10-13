// Typed bridge for the toolbar, tab strip and left rail pages. Mirrors docs/html-chrome-protocol.md (v1).
// Page-controlled text reaches the DOM only through textContent or attributes (see view.ts, tabs.ts, rail.ts).
import { MAX_RAIL_ITEMS, isRailIcon, isRailKey, type RailIconName } from './rail-logic';
import { validZoom } from './zoom-logic';

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
export type OpenTarget = 'menu' | 'site' | 'newtab' | 'split' | 'zoomreset';
export type CaptionAction = 'toggleMaximize' | 'systemMenu';
export type Security = 'secure' | 'notsecure' | 'certproblem' | 'internal' | 'unknown' | 'none';
export type TabAction = 'select' | 'close' | 'menu' | 'pin' | 'mute';
export type GroupAction = 'toggle' | 'menu';
export type RailAction = 'click' | 'menu';
export type PanelKind = 'panel' | 'hist' | 'bm' | 'cust' | 'notes' | 'set' | 'priv' | 'ren';
export type PanelToken = 'history' | 'bookmarks' | 'customize' | 'notes' | 'settings' | 'privacy' | 'rename';
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
  | { v: 1; t: 'side'; a: 'workspace' | 'newtab' | 'collapse' }
  // --- end vertical sidebar ---
  | { v: 1; t: 'rail'; a: RailAction; k: string }
  | { v: 1; t: 'railmove'; k: string; before: string }
  | ({ v: 1; t: PanelKind } & PanelFields)
  | { v: 1; t: 'size'; tok: number; w: number; h: number }
  | { v: 1; t: 'pick'; tok: number; id: number }
  | { v: 1; t: 'dismiss'; tok: number }
  // --- popup surfaces (lane PP) ---
  | { v: 1; t: 'act'; tok: number; id: number; n: number }
  // --- end popup surfaces ---
  // --- hover layer ---
  | { v: 1; t: 'tip'; text: string; r: string }
  | { v: 1; t: 'tipoff' }
  | { v: 1; t: 'hover'; k: HoverTargetKind; id: number; r: string }
  | { v: 1; t: 'hover'; k: 'none' }
  | ({ v: 1; t: 'hact'; tok: number } & HoverActFields)
  // --- end hover layer ---
  // --- omnibox suggest ---
  | { v: 1; t: 'suggestPick'; seq: number; i: number }
  | { v: 1; t: 'suggestPreview'; seq: number; i: number }
  | { v: 1; t: 'suggestEnd' };
  // --- end omnibox suggest ---

// --- hover layer ---
/** Longest tooltip text Rust accepts, in code points. */
export const MAX_TIP = 256;
/** Largest coordinate or size in a hover rectangle, in CSS pixels. */
export const MAX_HOVER_COORD = 16384;
/** Most member rows, swatches and markers Rust sends in one hover message. */
export const MAX_HOVER_ROWS = 12;
export const MAX_HOVER_SWATCHES = 32;
export const HOVER_MARKERS = 5;
/** Longest island name, in code points (`MAX_GROUP_NAME_CHARS`). */
export const MAX_ISLAND_NAME = 64;
const MAX_HOVER_HOST = 253;
const MAX_HOVER_ROW_INDEX = 63;
const MAX_HOVER_SWATCH_INDEX = 31;
export type HoverTargetKind = 'tab' | 'ball';
export type HoverActFields =
  | { a: 'marker'; n: number }
  | { a: 'row'; n: number }
  | { a: 'swatch'; n: number }
  | { a: 'name'; text: string }
  | { a: 'next' | 'close' | 'focus' | 'dismiss' };
export interface HoverRowMsg {
  readonly title: string;
  readonly active: boolean;
  readonly linked: boolean;
  /** PNG data: URI, or '' for the placeholder dot. */
  readonly fav: string;
}
export interface HoverTooltipMsg { readonly t: 'hover'; readonly tok: number; readonly kind: 'tooltip'; readonly text: string }
export interface HoverCardMsg {
  readonly t: 'hover';
  readonly tok: number;
  readonly kind: 'card';
  readonly title: string;
  readonly host: string;
  /** Marker id 0-4, or -1 for none. */
  readonly marker: number;
  /** Marker swatch colours ('' = theme accent). */
  readonly markers: readonly string[];
  readonly sleeping: boolean;
  readonly fav: string;
  readonly rows: readonly HoverRowMsg[];
  readonly more: number;
}
export interface HoverIslandMsg {
  readonly t: 'hover';
  readonly tok: number;
  readonly kind: 'island';
  readonly name: string;
  readonly hint: string;
  readonly slot: number;
  readonly swatches: readonly string[];
  readonly rows: readonly HoverRowMsg[];
  readonly more: number;
  readonly close: string;
}
export type HoverMsg = HoverTooltipMsg | HoverCardMsg | HoverIslandMsg;
export interface HoverHideMsg { readonly t: 'hoverhide' }
// --- end hover layer ---

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
  /** The window shows a split view. Absent in older hosts. */
  readonly split?: boolean;
  /** The page the tab shows is bookmarked. Absent in older hosts. */
  readonly bookmarked?: boolean;
  readonly progress: number;
  readonly security: Security;
  readonly label: string;
  /** A permission request on the current page was denied (passive toolbar indicator). */
  readonly permblocked?: boolean;
  /** The page zoom of the active tab in percent (25 to 500). Absent in older hosts; 100 means no chip. */
  readonly zoom?: number;
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
// --- omnibox suggest ---
/** Most rows in one `suggest` message, the default row included (Rust MAX_SUGGEST_ITEMS). */
export const MAX_SUGGEST_ITEMS = 8;
const MAX_SUGGEST_TEXT = 256;
const MAX_SUGGEST_WIDTH = 4000;
export const SUGGEST_KINDS = ['search', 'url', 'hist', 'bm', 'tab'] as const;
export type SuggestKind = (typeof SUGGEST_KINDS)[number];
export interface SuggestItem {
  readonly k: SuggestKind;
  /** What the omnibox shows while the row is selected. */
  readonly text: string;
  readonly title: string;
  /** Display form of the address; empty for a search row. */
  readonly url: string;
  /** A PNG data: URI, or '' when none is known. */
  readonly fav: string;
}
/** The row list for edit `seq`. `tok` is 0 for the toolbar's copy; no items closes the list. */
export interface SuggestMsg {
  readonly t: 'suggest';
  readonly tok: number;
  readonly seq: number;
  /** Dropdown width in CSS pixels (0 in the toolbar's copy). */
  readonly w: number;
  readonly sel: number;
  /** The typed text the rows answer ('' in the toolbar's copy); matches in rows are emphasised. */
  readonly q: string;
  readonly items: readonly SuggestItem[];
}
/** The keyboard selection moved (hover layer only). */
export interface SuggestSelMsg {
  readonly t: 'suggestSel';
  readonly seq: number;
  readonly sel: number;
}
/**
 * Whether the dropdown is on screen under the pill (toolbar only), so the pill joins it as one
 * surface. Sent when the popup shows and once it is fully hidden; never per keystroke.
 */
export interface SuggestOpenMsg {
  readonly t: 'suggestOpen';
  readonly on: boolean;
}
// --- end omnibox suggest ---
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
  // --- hover layer ---
  | HoverMsg
  | HoverHideMsg
  // --- end hover layer ---
  // --- omnibox suggest ---
  | SuggestMsg
  | SuggestSelMsg
  | SuggestOpenMsg
  // --- end omnibox suggest ---
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
const PANEL_TOKENS: readonly PanelToken[] = ['history', 'bookmarks', 'customize', 'notes', 'settings', 'privacy', 'rename'];
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
export function sendSide(a: 'workspace' | 'newtab' | 'collapse'): boolean {
  return (a === 'workspace' || a === 'newtab' || a === 'collapse') && send({ v: 1, t: 'side', a });
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
// --- popup surfaces (lane PP) ---
/** Largest `n` an `act` may carry (Rust: MAX_ACT_CHOICE). */
export const MAX_ACT_CHOICE = 7;
/** In-place action on an open popup page (site info control); the popup stays open. */
export function sendPopupAct(tok: number, id: number, n: number): boolean {
  return isInt(tok, 0, MAX_ID) && isInt(id, 0, MAX_ID) && isInt(n, 0, MAX_ACT_CHOICE) && send({ v: 1, t: 'act', tok, id, n });
}
// --- end popup surfaces ---

// --- hover layer ---
/** Code-point-safe cap, matching Rust's character count. */
function capCodePoints(text: string, max: number): string {
  const points = Array.from(text);
  return points.length <= max ? text : points.slice(0, max).join('');
}

/** `"x,y,w,h"` for a client rectangle: edges clamped to 0..MAX_HOVER_COORD, rounded outwards. */
export function rectWire(rect: { readonly left: number; readonly top: number; readonly right: number; readonly bottom: number }): string | null {
  const values = [rect.left, rect.top, rect.right, rect.bottom];
  if (!values.every(Number.isFinite)) return null;
  const clamp = (v: number): number => Math.min(MAX_HOVER_COORD, Math.max(0, v));
  const x = clamp(Math.floor(rect.left));
  const y = clamp(Math.floor(rect.top));
  const w = clamp(Math.ceil(rect.right) - x);
  const h = clamp(Math.ceil(rect.bottom) - y);
  return `${x},${y},${w},${h}`;
}

const RECT_RE = /^(0|[1-9][0-9]{0,4}),(0|[1-9][0-9]{0,4}),(0|[1-9][0-9]{0,4}),(0|[1-9][0-9]{0,4})$/;
/** The grammar Rust's `HoverRect::parse` accepts. */
export function isRectWire(value: string): boolean {
  const match = RECT_RE.exec(value);
  return match !== null && match.slice(1).every(part => Number(part) <= MAX_HOVER_COORD);
}

export function sendTip(text: string, rect: Parameters<typeof rectWire>[0]): boolean {
  const capped = capCodePoints(text, MAX_TIP);
  const r = rectWire(rect);
  return capped.trim() !== '' && r !== null && send({ v: 1, t: 'tip', text: capped, r });
}
export function sendTipOff(): boolean {
  return send({ v: 1, t: 'tipoff' });
}
export function sendHover(k: HoverTargetKind | 'none', id = 0, rect?: Parameters<typeof rectWire>[0]): boolean {
  if (k === 'none') return send({ v: 1, t: 'hover', k });
  const r = rect ? rectWire(rect) : null;
  return (k === 'tab' || k === 'ball') && isInt(id, 0, MAX_ID) && r !== null && send({ v: 1, t: 'hover', k, id, r });
}
export function sendHoverAct(tok: number, fields: HoverActFields): boolean {
  if (!isInt(tok, 0, MAX_ID)) return false;
  switch (fields.a) {
    case 'marker': if (!isInt(fields.n, 0, HOVER_MARKERS - 1)) return false; break;
    case 'row': if (!isInt(fields.n, 0, MAX_HOVER_ROW_INDEX)) return false; break;
    case 'swatch': if (!isInt(fields.n, 0, MAX_HOVER_SWATCH_INDEX)) return false; break;
    case 'name': if (typeof fields.text !== 'string' || Array.from(fields.text).length > MAX_ISLAND_NAME) return false; break;
    case 'next': case 'close': case 'focus': case 'dismiss': break;
    default: return false;
  }
  return send({ v: 1, t: 'hact', tok, ...fields });
}

// --- omnibox suggest ---
const SUGGEST_ITEM_KEYS = ['k', 'text', 'title', 'url', 'fav'];
function parseSuggestItem(v: unknown): SuggestItem | null {
  if (!isRecord(v) || !hasOnlyKeys(v, SUGGEST_ITEM_KEYS) || !['k', 'text', 'title', 'url'].every(key => key in v)) return null;
  const { k, text, title, url } = v;
  if (!isOneOf(k, SUGGEST_KINDS)) return null;
  if (!isStr(text, MAX_SUGGEST_TEXT) || !isStr(title, MAX_TITLE) || !isStr(url, MAX_SUGGEST_TEXT)) return null;
  const fav = 'fav' in v ? v.fav : '';
  if (fav !== '' && !isFavUri(fav)) return null;
  return { k, text, title, url, fav: fav as string };
}

/** Strict validation of a `suggest` message. */
function parseSuggestMsg(raw: Record<string, unknown>): SuggestMsg | null {
  if (!hasOnlyKeys(raw, ['v', 't', 'tok', 'seq', 'w', 'sel', 'q', 'items']) || raw.v !== 1) return null;
  const { tok, seq, w, sel, q, items } = raw;
  if (!isInt(tok, 0, MAX_ID) || !isInt(seq, 0, MAX_SEQ) || !isInt(w, 0, MAX_SUGGEST_WIDTH)) return null;
  // Rust caps the query in UTF-16 units (twice the row text limit).
  if (!isStr(q, MAX_SUGGEST_TEXT * 2)) return null;
  if (!Array.isArray(items) || items.length > MAX_SUGGEST_ITEMS || !isInt(sel, 0, MAX_SUGGEST_ITEMS - 1)) return null;
  if (items.length > 0 && sel >= items.length) return null;
  // One bad row (say an over-long one) must not take the whole list down with it: keep the valid
  // rows, and move the selection with them (to the first row if its own was dropped). A list whose
  // rows are all invalid is rejected.
  const out: SuggestItem[] = [];
  let keptSel = items.length === 0 ? sel : 0;
  items.forEach((v, index) => {
    const item = parseSuggestItem(v);
    if (!item) return;
    if (index === sel) keptSel = out.length;
    out.push(item);
  });
  if (items.length > 0 && out.length === 0) return null;
  return { t: 'suggest', tok, seq, w, sel: keptSel, q, items: out };
}

export function sendSuggestPick(seq: number, i: number): boolean {
  return isInt(seq, 0, MAX_SEQ) && isInt(i, 0, MAX_SUGGEST_ITEMS - 1) && send({ v: 1, t: 'suggestPick', seq, i });
}
export function sendSuggestPreview(seq: number, i: number): boolean {
  return isInt(seq, 0, MAX_SEQ) && isInt(i, 0, MAX_SUGGEST_ITEMS - 1) && send({ v: 1, t: 'suggestPreview', seq, i });
}
export function sendSuggestEnd(): boolean {
  return send({ v: 1, t: 'suggestEnd' });
}
// --- end omnibox suggest ---

const HOVER_ROW_KEYS = ['title', 'active', 'linked', 'fav'];
function parseHoverRows(v: unknown): HoverRowMsg[] | null {
  if (!Array.isArray(v) || v.length > MAX_HOVER_ROWS) return null;
  const rows: HoverRowMsg[] = [];
  for (const row of v) {
    if (!isRecord(row) || !hasOnlyKeys(row, HOVER_ROW_KEYS) || !hasAllKeys(row, HOVER_ROW_KEYS)) return null;
    const { title, active, linked, fav } = row;
    if (!isStr(title, MAX_TITLE) || !isBool(active) || !isBool(linked) || (fav !== '' && !isFavUri(fav))) return null;
    rows.push({ title, active, linked, fav: fav as string });
  }
  return rows;
}
function parseColours(v: unknown, max: number): string[] | null {
  if (!Array.isArray(v) || v.length > max) return null;
  const out: string[] = [];
  for (const colour of v) {
    if (!isStr(colour, MAX_COLOR) || (colour !== '' && !isSafeColour(colour))) return null;
    out.push(colour);
  }
  return out;
}
const TOOLTIP_KEYS = ['v', 't', 'tok', 'kind', 'text'];
const CARD_KEYS = ['v', 't', 'tok', 'kind', 'title', 'host', 'marker', 'markers', 'sleeping', 'fav', 'rows', 'more'];
const ISLAND_KEYS = ['v', 't', 'tok', 'kind', 'name', 'hint', 'slot', 'swatches', 'rows', 'more', 'close'];

/** Strict validation of a Rust `hover` message (tooltip, card or island content). */
export function parseHoverMsg(raw: Record<string, unknown>): HoverMsg | null {
  if (raw.v !== 1 || !isInt(raw.tok, 1, MAX_ID)) return null;
  const tok = raw.tok;
  switch (raw.kind) {
    case 'tooltip': {
      if (!hasOnlyKeys(raw, TOOLTIP_KEYS) || !hasAllKeys(raw, TOOLTIP_KEYS) || !isStr(raw.text, MAX_TIP * 2) || Array.from(raw.text).length > MAX_TIP) return null;
      return { t: 'hover', tok, kind: 'tooltip', text: raw.text };
    }
    case 'card': {
      if (!hasOnlyKeys(raw, CARD_KEYS) || !hasAllKeys(raw, CARD_KEYS)) return null;
      const { title, host, marker, sleeping, fav, more } = raw;
      const markers = parseColours(raw.markers, HOVER_MARKERS);
      const rows = parseHoverRows(raw.rows);
      if (!isStr(title, MAX_TITLE) || !isStr(host, MAX_HOVER_HOST) || !isInt(marker, -1, HOVER_MARKERS - 1)) return null;
      if (!isBool(sleeping) || (fav !== '' && !isFavUri(fav)) || !isInt(more, 0, MAX_ID) || !markers || !rows) return null;
      return { t: 'hover', tok, kind: 'card', title, host, marker, markers, sleeping, fav: fav as string, rows, more };
    }
    case 'island': {
      if (!hasOnlyKeys(raw, ISLAND_KEYS) || !hasAllKeys(raw, ISLAND_KEYS)) return null;
      const { name, hint, slot, more, close } = raw;
      const swatches = parseColours(raw.swatches, MAX_HOVER_SWATCHES);
      const rows = parseHoverRows(raw.rows);
      if (!isStr(name, MAX_ISLAND_NAME * 2) || Array.from(name).length > MAX_ISLAND_NAME) return null;
      if (!isStr(hint, MAX_ISLAND_NAME * 2) || Array.from(hint).length > MAX_ISLAND_NAME) return null;
      if (!isStr(close, MAX_ISLAND_NAME * 2) || !isInt(more, 0, MAX_ID) || !swatches || !rows) return null;
      if (!isInt(slot, 0, Math.max(0, swatches.length - 1))) return null;
      return { t: 'hover', tok, kind: 'island', name, hint, slot, swatches, rows, more, close };
    }
    default:
      return null;
  }
}
// --- end hover layer ---

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
    // --- hover layer ---
    case 'hover':
      return parseHoverMsg(raw);
    case 'hoverhide':
      return hasOnlyKeys(raw, ['v', 't']) && raw.v === 1 ? { t: 'hoverhide' } : null;
    // --- end hover layer ---
    // --- omnibox suggest ---
    case 'suggest':
      return parseSuggestMsg(raw);
    case 'suggestSel': {
      if (!hasOnlyKeys(raw, ['v', 't', 'seq', 'sel']) || raw.v !== 1) return null;
      const { seq, sel } = raw;
      if (!isInt(seq, 0, MAX_SEQ) || !isInt(sel, 0, MAX_SUGGEST_ITEMS - 1)) return null;
      return { t: 'suggestSel', seq, sel };
    }
    case 'suggestOpen':
      if (!hasOnlyKeys(raw, ['v', 't', 'on']) || raw.v !== 1 || !isBool(raw.on)) return null;
      return { t: 'suggestOpen', on: raw.on };
    // --- end omnibox suggest ---
    case 'snap': {
      if (!hasOnlyKeys(raw, ['v', 't', 'p', 'd']) || raw.v !== 1 || !isOneOf(raw.p, PANEL_TOKENS) || !isRecord(raw.d)) return null;
      const budget = { keys: 0 };
      if (!validSnapshot(raw.d, 1, budget)) return null;
      return { t: 'snap', p: raw.p, d: raw.d as SnapshotMsg['d'] };
    }
    case 'state': {
      if (!hasOnlyKeys(raw, ['t', 'scale', 'pillx', 'pillw', 'menux', 'solid', 'dark', 'back', 'forward', 'loading', 'split', 'bookmarked', 'progress', 'security', 'label', 'permblocked', 'zoom', 'theme'])) return null;
      if ('permblocked' in raw && !isBool(raw.permblocked)) return null;
      if ('zoom' in raw && !validZoom(raw.zoom)) return null;
      const { scale, solid, dark, back, forward, loading, progress, security, label } = raw;
      if (typeof scale !== 'number' || !Number.isFinite(scale)) return null;
      const placementKeys = ['pillx', 'pillw', 'menux'] as const;
      const placementCount = placementKeys.filter((key) => key in raw).length;
      if (placementCount !== 0 && placementCount !== placementKeys.length) return null;
      if (placementCount !== 0 && placementKeys.some((key) => !isInt(raw[key], 0, Number.MAX_SAFE_INTEGER))) return null;
      if (!isBool(solid) || !isBool(dark) || !isBool(back) || !isBool(forward) || !isBool(loading)) return null;
      if (!isInt(progress, 0, 100)) return null;
      if (('split' in raw && !isBool(raw.split)) || ('bookmarked' in raw && !isBool(raw.bookmarked))) return null;
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
        ...(typeof raw.split === 'boolean' ? { split: raw.split } : {}),
        ...(typeof raw.bookmarked === 'boolean' ? { bookmarked: raw.bookmarked } : {}),
        progress,
        security,
        label,
        ...(isBool(raw.permblocked) ? { permblocked: raw.permblocked } : {}),
        ...(typeof raw.zoom === 'number' ? { zoom: raw.zoom } : {}),
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
