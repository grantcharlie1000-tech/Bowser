// Pure logic of the start page (docs/design/start-page.md): the snapshot contract (the page's strict
// parser for what Rust writes in chrome_host/start_page.rs), the settings object the settings page
// shares, and the small decisions the DOM code leans on (how many columns fit, when a title shows, where
// a dragged dial lands, what a widget background looks like). No DOM here; test/start-page.mjs covers it.

export const STARTUP_MODES = ['start_page', 'last_session', 'homepage', 'specific_pages'] as const;
export type StartupMode = typeof STARTUP_MODES[number];
export const GROUP_REOPEN = ['last', 'first'] as const;
export type GroupReopen = typeof GROUP_REOPEN[number];
export const TITLE_MODES = ['always', 'never', 'auto'] as const;
export type TitleMode = typeof TITLE_MODES[number];
export const WIDGET_HEIGHTS = ['regular', 'tall'] as const;
export type WidgetHeight = typeof WIDGET_HEIGHTS[number];
export const WIDGET_BGS = ['solid', 'translucent', 'transparent', 'custom'] as const;
export type WidgetBg = typeof WIDGET_BGS[number];
export const WIDGET_KINDS = ['topsites', 'bookmarks', 'notes', 'date', 'privacy', 'webpage'] as const;
export type WidgetKind = typeof WIDGET_KINDS[number];
export const BOOKMARK_VIEWS = ['list', 'tiles'] as const;
export type BookmarkView = typeof BOOKMARK_VIEWS[number];

export const MIN_COLUMNS = 1;
export const MAX_COLUMNS = 12;
export const MIN_THUMB_SIZE = 1;
export const MAX_THUMB_SIZE = 5;
export const MIN_WIDGET_COLUMNS = 1;
export const MAX_WIDGET_COLUMNS = 4;
export const MAX_STARTUP_PAGES = 8;
export const MAX_URL = 2048;
export const MAX_TITLE = 200;
export const MAX_NOTE = 2000;
const COLOR_RE = /^#[0-9a-f]{6}$/;

/** Limits shared with `src/chrome_host/start_page.rs`. */
const MAX_GROUPS = 24;
const MAX_DIALS = 120;
const MAX_WIDGETS = 12;
const MAX_ROWS = 12;
const MAX_FOLDERS = 200;
const MAX_FAVS = 80;
const MAX_URI = 40 * 1024;
const BASE64_RE = /^[A-Za-z0-9+/]+={0,2}$/;
const PNG_PREFIX = 'data:image/png;base64,';
const JPEG_PREFIX = 'data:image/jpeg;base64,';

/** The settings the settings page edits and the start page obeys (the `start` object of its snapshot). */
export interface StartSettings {
  groupReopen: GroupReopen;
  titles: TitleMode;
  addButton: boolean;
  columns: number;
  thumbSize: number;
  dragReorder: boolean;
  homepage: string;
  startup: StartupMode;
  startupPages: string[];
  widgets: boolean;
  widgetColumns: number;
  widgetHeight: WidgetHeight;
  widgetBg: WidgetBg;
  widgetColor: string;
  widgetDrag: boolean;
}

export const START_SETTINGS_KEYS = ['groupReopen', 'titles', 'addButton', 'columns', 'thumbSize', 'dragReorder', 'homepage', 'startup',
  'startupPages', 'widgets', 'widgetColumns', 'widgetHeight', 'widgetBg', 'widgetColor', 'widgetDrag'] as const;

type Rec = Record<string, unknown>;
const isRecord = (v: unknown): v is Rec => typeof v === 'object' && v !== null && !Array.isArray(v);
const isInt = (v: unknown, min: number, max: number): v is number => typeof v === 'number' && Number.isInteger(v) && v >= min && v <= max;
const isStr = (v: unknown, max: number): v is string => typeof v === 'string' && v.length <= max;
const isBool = (v: unknown): v is boolean => typeof v === 'boolean';
const oneOf = <T extends string>(v: unknown, list: readonly T[]): v is T => typeof v === 'string' && (list as readonly string[]).includes(v);
const exactKeys = (v: Rec, keys: readonly string[]): boolean => {
  const own = Object.keys(v);
  return own.length === keys.length && keys.every((k) => own.includes(k));
};

/** An http(s) address without user info or whitespace, as Rust's `web_address` accepts (before it normalises). */
export function isWebAddress(text: string): boolean {
  if (text === '' || text.length > MAX_URL || /[\s\u0000-\u001f\u007f]/.test(text)) return false;
  try {
    const url = new URL(text);
    return (url.protocol === 'http:' || url.protocol === 'https:') && url.hostname !== '' && url.username === '' && url.password === '';
  } catch {
    return false;
  }
}

/**
 * What was typed into an address field, as an http(s) address: `https://` is added to a bare host, and
 * anything with another scheme is refused. `null` when the result is not a web address.
 */
export function normaliseAddress(text: string): string | null {
  const t = text.trim();
  if (t === '') return null;
  const full = /^https?:\/\//i.test(t) ? t : t.includes('://') ? '' : `https://${t}`;
  return isWebAddress(full) ? full : null;
}

/** The `start` object of the settings snapshot, or `null` when anything is off. */
export function parseStartSettings(v: unknown): StartSettings | null {
  if (!isRecord(v) || !exactKeys(v, START_SETTINGS_KEYS)) return null;
  if (!oneOf(v.groupReopen, GROUP_REOPEN) || !oneOf(v.titles, TITLE_MODES) || !isBool(v.addButton) || !isBool(v.dragReorder)) return null;
  if (!isInt(v.columns, MIN_COLUMNS, MAX_COLUMNS) || !isInt(v.thumbSize, MIN_THUMB_SIZE, MAX_THUMB_SIZE)) return null;
  if (!isStr(v.homepage, MAX_URL) || (v.homepage !== '' && !isWebAddress(v.homepage)) || !oneOf(v.startup, STARTUP_MODES)) return null;
  if (!Array.isArray(v.startupPages) || v.startupPages.length > MAX_STARTUP_PAGES || !v.startupPages.every((p) => typeof p === 'string' && isWebAddress(p))) return null;
  if (!isBool(v.widgets) || !isInt(v.widgetColumns, MIN_WIDGET_COLUMNS, MAX_WIDGET_COLUMNS) || !oneOf(v.widgetHeight, WIDGET_HEIGHTS)) return null;
  if (!oneOf(v.widgetBg, WIDGET_BGS) || typeof v.widgetColor !== 'string' || !COLOR_RE.test(v.widgetColor) || !isBool(v.widgetDrag)) return null;
  return {
    groupReopen: v.groupReopen, titles: v.titles, addButton: v.addButton, columns: v.columns, thumbSize: v.thumbSize, dragReorder: v.dragReorder,
    homepage: v.homepage, startup: v.startup, startupPages: [...(v.startupPages as string[])], widgets: v.widgets, widgetColumns: v.widgetColumns,
    widgetHeight: v.widgetHeight, widgetBg: v.widgetBg, widgetColor: v.widgetColor, widgetDrag: v.widgetDrag,
  };
}

/** The `settings.txt` lines the start page settings save as (appended to the `save` text; every key once). */
export function startSaveLines(s: StartSettings): string[] | null {
  if (parseStartSettings({ ...s }) === null) return null;
  return [
    `start_group_reopen=${s.groupReopen}`,
    `start_titles=${s.titles}`,
    `start_add_button=${s.addButton}`,
    `start_columns=${s.columns}`,
    `start_thumb_size=${s.thumbSize}`,
    `start_drag_reorder=${s.dragReorder}`,
    `homepage=${s.homepage}`,
    `startup=${s.startup}`,
    `startup_pages=${s.startupPages.join(' ')}`,
    `start_widgets=${s.widgets}`,
    `start_widget_columns=${s.widgetColumns}`,
    `start_widget_height=${s.widgetHeight}`,
    `start_widget_bg=${s.widgetBg}`,
    `start_widget_color=${s.widgetColor}`,
    `start_widget_drag=${s.widgetDrag}`,
  ];
}

// ---- the page snapshot ----

export interface DisplaySettings {
  columns: number;
  thumbSize: number;
  titles: TitleMode;
  addButton: boolean;
  dragReorder: boolean;
  widgets: boolean;
  widgetColumns: number;
  widgetHeight: WidgetHeight;
  widgetBg: WidgetBg;
  widgetColor: string;
  widgetDrag: boolean;
}
export interface GroupTab { id: number; title: string; n: number }
export interface Crumb { id: number; title: string }
export interface Dial { id: number; title: string; kind: 'link' | 'folder'; url: string; host: string; n: number; thumb: boolean }
export interface FolderChoice { id: number; path: string }
export interface FolderRef { id: number; title: string }
export interface TopSite { host: string; title: string; url: string }
export interface BookmarkLink { id: number; title: string; url: string; host: string; thumb: boolean }
export type Widget =
  | { id: number; kind: 'topsites'; sites: TopSite[] }
  | { id: number; kind: 'bookmarks'; folder: number; view: BookmarkView; links: BookmarkLink[] }
  | { id: number; kind: 'notes'; text: string }
  | { id: number; kind: 'date' }
  | { id: number; kind: 'privacy'; today: number; total: number; topHost: string; topCount: number }
  | { id: number; kind: 'webpage'; url: string };
export interface StartSnapshot {
  guest: boolean;
  settings: DisplaySettings;
  groups: GroupTab[];
  group: number;
  folder: number;
  path: Crumb[];
  dials: Dial[];
  more: boolean;
  pickable: FolderChoice[];
  folders: FolderRef[];
  widgets: Widget[];
  favs: Record<string, string>;
  ack: 'none' | 'failed';
  error: string;
}
export type StartMessage = { kind: 'page'; snap: StartSnapshot } | { kind: 'thumbs'; thumbs: Record<string, string> };

const isPng = (v: unknown): v is string => isStr(v, MAX_URI) && v.startsWith(PNG_PREFIX) && BASE64_RE.test(v.slice(PNG_PREFIX.length));
const isJpeg = (v: unknown): v is string => isStr(v, 160 * 1024) && v.startsWith(JPEG_PREFIX) && BASE64_RE.test(v.slice(JPEG_PREFIX.length));

function list<T>(v: unknown, max: number, one: (item: unknown) => T | null): T[] | null {
  if (!Array.isArray(v) || v.length > max) return null;
  const out: T[] = [];
  for (const item of v) {
    const parsed = one(item);
    if (parsed === null) return null;
    out.push(parsed);
  }
  return out;
}

function parseDisplay(v: unknown): DisplaySettings | null {
  if (!isRecord(v) || !exactKeys(v, ['columns', 'thumbSize', 'titles', 'addButton', 'dragReorder', 'widgets', 'widgetColumns', 'widgetHeight', 'widgetBg', 'widgetColor', 'widgetDrag'])) return null;
  if (!isInt(v.columns, MIN_COLUMNS, MAX_COLUMNS) || !isInt(v.thumbSize, MIN_THUMB_SIZE, MAX_THUMB_SIZE) || !oneOf(v.titles, TITLE_MODES)) return null;
  if (!isBool(v.addButton) || !isBool(v.dragReorder) || !isBool(v.widgets) || !isInt(v.widgetColumns, MIN_WIDGET_COLUMNS, MAX_WIDGET_COLUMNS)) return null;
  if (!oneOf(v.widgetHeight, WIDGET_HEIGHTS) || !oneOf(v.widgetBg, WIDGET_BGS) || typeof v.widgetColor !== 'string' || !COLOR_RE.test(v.widgetColor) || !isBool(v.widgetDrag)) return null;
  return {
    columns: v.columns, thumbSize: v.thumbSize, titles: v.titles, addButton: v.addButton, dragReorder: v.dragReorder, widgets: v.widgets,
    widgetColumns: v.widgetColumns, widgetHeight: v.widgetHeight, widgetBg: v.widgetBg, widgetColor: v.widgetColor, widgetDrag: v.widgetDrag,
  };
}

function parseWidget(v: unknown): Widget | null {
  if (!isRecord(v) || !isInt(v.id, 1, 0xffffffff) || !oneOf(v.kind, WIDGET_KINDS)) return null;
  const id = v.id;
  switch (v.kind) {
    case 'topsites': {
      if (!exactKeys(v, ['id', 'kind', 'sites'])) return null;
      const sites = list(v.sites, MAX_ROWS, (s): TopSite | null => (isRecord(s) && exactKeys(s, ['host', 'title', 'url']) && isStr(s.host, 253) && isStr(s.title, MAX_TITLE) && isStr(s.url, 400) && isWebAddress(s.url) ? { host: s.host, title: s.title, url: s.url } : null));
      return sites === null ? null : { id, kind: 'topsites', sites };
    }
    case 'bookmarks': {
      if (!exactKeys(v, ['id', 'kind', 'folder', 'view', 'links']) || !isInt(v.folder, 0, Number.MAX_SAFE_INTEGER) || !oneOf(v.view, BOOKMARK_VIEWS)) return null;
      const links = list(v.links, MAX_ROWS, (s): BookmarkLink | null => (isRecord(s) && exactKeys(s, ['id', 'title', 'url', 'host', 'thumb']) && isInt(s.id, 1, Number.MAX_SAFE_INTEGER) && isStr(s.title, MAX_TITLE) && isStr(s.url, 400) && isWebAddress(s.url) && isStr(s.host, 253) && isBool(s.thumb) ? { id: s.id, title: s.title, url: s.url, host: s.host, thumb: s.thumb } : null));
      return links === null ? null : { id, kind: 'bookmarks', folder: v.folder, view: v.view, links };
    }
    case 'notes':
      return exactKeys(v, ['id', 'kind', 'text']) && isStr(v.text, MAX_NOTE) ? { id, kind: 'notes', text: v.text } : null;
    case 'date':
      return exactKeys(v, ['id', 'kind']) ? { id, kind: 'date' } : null;
    case 'privacy':
      return exactKeys(v, ['id', 'kind', 'today', 'total', 'topHost', 'topCount']) && isInt(v.today, 0, Number.MAX_SAFE_INTEGER) && isInt(v.total, 0, Number.MAX_SAFE_INTEGER) && isStr(v.topHost, 253) && isInt(v.topCount, 0, Number.MAX_SAFE_INTEGER)
        ? { id, kind: 'privacy', today: v.today, total: v.total, topHost: v.topHost, topCount: v.topCount }
        : null;
    case 'webpage':
      return exactKeys(v, ['id', 'kind', 'url']) && isStr(v.url, MAX_URL) && (v.url === '' || isWebAddress(v.url)) ? { id, kind: 'webpage', url: v.url } : null;
    default:
      return null;
  }
}

function parsePage(d: Rec): StartSnapshot | null {
  if (!exactKeys(d, ['kind', 'guest', 'settings', 'groups', 'group', 'folder', 'path', 'dials', 'more', 'pickable', 'folders', 'widgets', 'favs', 'ack', 'error'])) return null;
  if (!isBool(d.guest) || !isBool(d.more) || !isInt(d.group, 0, Number.MAX_SAFE_INTEGER) || !isInt(d.folder, 0, Number.MAX_SAFE_INTEGER)) return null;
  if (!(d.ack === 'none' || d.ack === 'failed') || !isStr(d.error, 200) || (d.ack === 'none') !== (d.error === '')) return null;
  const settings = parseDisplay(d.settings);
  const groups = list(d.groups, MAX_GROUPS, (g): GroupTab | null => (isRecord(g) && exactKeys(g, ['id', 'title', 'n']) && isInt(g.id, 1, Number.MAX_SAFE_INTEGER) && isStr(g.title, 400) && isInt(g.n, 0, 100000) ? { id: g.id, title: g.title, n: g.n } : null));
  const path = list(d.path, 17, (c): Crumb | null => (isRecord(c) && exactKeys(c, ['id', 'title']) && isInt(c.id, 1, Number.MAX_SAFE_INTEGER) && isStr(c.title, 400) ? { id: c.id, title: c.title } : null));
  const dials = list(d.dials, MAX_DIALS, (x): Dial | null => {
    if (!isRecord(x) || !exactKeys(x, ['id', 'title', 'kind', 'url', 'host', 'n', 'thumb'])) return null;
    if (!isInt(x.id, 1, Number.MAX_SAFE_INTEGER) || !isStr(x.title, 400) || !(x.kind === 'link' || x.kind === 'folder') || !isStr(x.url, 400) || !isStr(x.host, 253) || !isInt(x.n, 0, 100000) || !isBool(x.thumb)) return null;
    if (x.kind === 'link' && !isWebAddress(x.url)) return null;
    if (x.kind === 'folder' && (x.url !== '' || x.host !== '')) return null;
    return { id: x.id, title: x.title, kind: x.kind, url: x.url, host: x.host, n: x.n, thumb: x.thumb };
  });
  const pickable = list(d.pickable, MAX_FOLDERS, (c): FolderChoice | null => (isRecord(c) && exactKeys(c, ['id', 'path']) && isInt(c.id, 1, Number.MAX_SAFE_INTEGER) && isStr(c.path, 400) ? { id: c.id, path: c.path } : null));
  const folders = list(d.folders, MAX_FOLDERS, (c): FolderRef | null => (isRecord(c) && exactKeys(c, ['id', 'title']) && isInt(c.id, 1, Number.MAX_SAFE_INTEGER) && isStr(c.title, 400) ? { id: c.id, title: c.title } : null));
  const widgets = list(d.widgets, MAX_WIDGETS, parseWidget);
  if (!settings || !groups || !path || !dials || !pickable || !folders || !widgets) return null;
  if (!isRecord(d.favs) || Object.keys(d.favs).length > MAX_FAVS || !Object.values(d.favs).every(isPng)) return null;
  if (new Set(dials.map((x) => x.id)).size !== dials.length || new Set(widgets.map((w) => w.id)).size !== widgets.length) return null;
  // No groups means no folder shown; otherwise the folder shown is the last step of the path.
  if (groups.length === 0 ? d.group !== 0 || d.folder !== 0 || path.length !== 0 || dials.length !== 0 : path.length === 0 || path[path.length - 1]!.id !== d.folder || path[0]!.id !== d.group) return null;
  return {
    guest: d.guest, settings, groups, group: d.group, folder: d.folder, path, dials, more: d.more, pickable, folders, widgets,
    favs: { ...(d.favs as Record<string, string>) }, ack: d.ack, error: d.error,
  };
}

/** Strict parser for the `d` of a `snap` with `p:"startpage"`; anything unexpected is `null`. */
export function parseStartMessage(d: unknown): StartMessage | null {
  if (!isRecord(d)) return null;
  if (d.kind === 'page') {
    const snap = parsePage(d);
    return snap ? { kind: 'page', snap } : null;
  }
  if (d.kind === 'thumbs') {
    if (!exactKeys(d, ['kind', 'thumbs']) || !isRecord(d.thumbs)) return null;
    const entries = Object.entries(d.thumbs);
    if (entries.length > 100 || !entries.every(([id, uri]) => /^[1-9][0-9]{0,15}$/.test(id) && isJpeg(uri))) return null;
    return { kind: 'thumbs', thumbs: Object.fromEntries(entries) as Record<string, string> };
  }
  return null;
}

// ---- layout and display decisions ----

/** Tile widths in CSS px for the five thumbnail sizes. */
export const TILE_WIDTHS: readonly number[] = [96, 120, 148, 184, 232];
export const TILE_GAP = 16;
/** The widest the dial area grows regardless of `columns`: nothing here wraps text or leaves a gap. */
export function tileWidth(size: number): number {
  return TILE_WIDTHS[Math.min(Math.max(size, MIN_THUMB_SIZE), MAX_THUMB_SIZE) - 1]!;
}

/** Columns that fit `width` px at `size`, never more than the `columns` setting, at least one. */
export function columnsFor(width: number, columns: number, size: number): number {
  const tile = tileWidth(size);
  const fit = Math.floor((width + TILE_GAP) / (tile + TILE_GAP));
  return Math.max(1, Math.min(columns, fit));
}

/** `show`: the title is under the tile; `hover`: it appears on hover and focus; `hide`: it never does. */
export function titleState(mode: TitleMode, hasThumb: boolean): 'show' | 'hover' | 'hide' {
  if (mode === 'never') return 'hide';
  if (mode === 'always') return 'show';
  return hasThumb ? 'hover' : 'show';
}

/** `#rrggbb` and the text colour that reads on it (WCAG relative luminance, white or near-black). */
export function textOn(color: string): '#ffffff' | '#14181d' {
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/.exec(color);
  if (!m) return '#ffffff';
  const lin = (hex: string): number => {
    const c = parseInt(hex, 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  const lum = 0.2126 * lin(m[1]!) + 0.7152 * lin(m[2]!) + 0.0722 * lin(m[3]!);
  // White on the colour if its contrast is at least that of near-black on it.
  return (1.05) / (lum + 0.05) >= (lum + 0.05) / 0.0589 ? '#ffffff' : '#14181d';
}

/** The classes the page root carries for the widget background style and height. */
export function widgetClasses(bg: WidgetBg, height: WidgetHeight): string[] {
  return [`is-bg-${bg}`, `is-${height}`];
}

/** Where a dragged dial lands: the id of the dial it goes before (`null`: the end), given the order, the dragged id and the pointer. */
export function dropBefore(ids: readonly number[], dragged: number, over: number, after: boolean): number | null | undefined {
  if (!ids.includes(dragged) || !ids.includes(over)) return undefined;
  const rest = ids.filter((id) => id !== dragged);
  const at = rest.indexOf(over);
  if (over === dragged) return undefined;
  const index = after ? at + 1 : at;
  const target = index >= rest.length ? null : rest[index]!;
  // Dropping where it already is changes nothing.
  const current = ids.indexOf(dragged);
  const currentBefore = current + 1 >= ids.length ? null : ids[current + 1]!;
  return target === currentBefore ? undefined : target;
}

export function blockedText(n: number): string {
  return n === 1 ? '1 request blocked' : `${n.toLocaleString('en-GB')} requests blocked`;
}

export function dialCountText(n: number): string {
  return n === 1 ? '1 site' : `${n} sites`;
}

export function dateParts(now: Date, locale?: string): { weekday: string; day: string; month: string; year: string } {
  const f = (opts: Intl.DateTimeFormatOptions): string => new Intl.DateTimeFormat(locale, opts).format(now);
  return { weekday: f({ weekday: 'long' }), day: f({ day: 'numeric' }), month: f({ month: 'long' }), year: f({ year: 'numeric' }) };
}

/** The label of a widget kind in the add menu and a card's heading. */
export const WIDGET_LABELS: Readonly<Record<WidgetKind, string>> = {
  topsites: 'Top sites',
  bookmarks: 'Bookmarks',
  notes: 'Sticky note',
  date: 'Date',
  privacy: 'Privacy statistics',
  webpage: 'Webpage',
};

/** What the sticky note's save button says once it has saved. */
export const NOTE_SAVED = 'Saved to Notes';
