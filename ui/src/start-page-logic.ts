// Pure logic of the start page (docs/design/start-page.md): the snapshot contract (the page's strict
// parser for what Rust writes in chrome_host/start_page.rs), the settings object the settings page
// shares, and the small decisions the DOM code leans on (how many columns fit, when a title shows, where
// a dragged dial lands, what a widget background looks like). No DOM here; test/start-page.mjs covers it.

export const STARTUP_MODES = ['start_page', 'last_session', 'homepage', 'specific_pages'] as const;
export type StartupMode = typeof STARTUP_MODES[number];
/**
 * The choices the settings page offers. The saved tabs come back on every start, so each choice is what
 * opens in front of them; `start_page` (from before tabs always came back) is read as `last_session`
 * and never offered (src/start_page.rs `Startup`).
 */
export const STARTUP_CHOICES = ['last_session', 'homepage', 'specific_pages'] as const;
export type StartupChoice = typeof STARTUP_CHOICES[number];
/** The choice a saved mode is: the old `start_page` is `last_session`. */
export function startupChoice(mode: StartupMode): StartupChoice {
  return mode === 'start_page' ? 'last_session' : mode;
}
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
/** `start_wallpaper`: the window's glass, a picture the page draws, or the user's own picture. */
export const WALLPAPERS = ['none', 'aurora', 'dusk', 'ocean', 'forest', 'blossom', 'sand', 'graphite', 'midnight', 'custom'] as const;
export type Wallpaper = typeof WALLPAPERS[number];
export const MAX_WALL_DIM = 80;
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
/** `<size>-<modified>` of the picture file, or empty. */
const WALL_STAMP_RE = /^(?:[0-9]{1,12}-[0-9]{1,40})?$/;
const MAX_WALL_PART = 150 * 1024;
/** 1.5 MiB of JPEG is about 2 MiB of base64: 14 parts. */
const MAX_WALL_PARTS = 16;
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
  /** `start_dials`: the speed dials on a new tab (default off: the tab is blank). */
  dials: boolean;
  /** `start_search`: a search box on the page. */
  search: boolean;
  /** `start_clock`: the time and date above it. */
  clock: boolean;
  wallpaper: Wallpaper;
  /** `start_wallpaper_dim`: how much the picture is darkened (or, on a light picture, lightened), 0 to 80. */
  wallpaperDim: number;
}

export const START_SETTINGS_KEYS = ['groupReopen', 'titles', 'addButton', 'columns', 'thumbSize', 'dragReorder', 'homepage', 'startup',
  'startupPages', 'widgets', 'widgetColumns', 'widgetHeight', 'widgetBg', 'widgetColor', 'widgetDrag', 'dials', 'search', 'clock', 'wallpaper', 'wallpaperDim'] as const;

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
  if (!oneOf(v.widgetBg, WIDGET_BGS) || typeof v.widgetColor !== 'string' || !COLOR_RE.test(v.widgetColor) || !isBool(v.widgetDrag) || !isBool(v.dials)) return null;
  if (!isBool(v.search) || !isBool(v.clock) || !oneOf(v.wallpaper, WALLPAPERS) || !isInt(v.wallpaperDim, 0, MAX_WALL_DIM)) return null;
  return {
    groupReopen: v.groupReopen, titles: v.titles, addButton: v.addButton, columns: v.columns, thumbSize: v.thumbSize, dragReorder: v.dragReorder,
    homepage: v.homepage, startup: v.startup, startupPages: [...(v.startupPages as string[])], widgets: v.widgets, widgetColumns: v.widgetColumns,
    widgetHeight: v.widgetHeight, widgetBg: v.widgetBg, widgetColor: v.widgetColor, widgetDrag: v.widgetDrag, dials: v.dials,
    search: v.search, clock: v.clock, wallpaper: v.wallpaper, wallpaperDim: v.wallpaperDim,
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
    `start_dials=${s.dials}`,
    `start_search=${s.search}`,
    `start_clock=${s.clock}`,
    `start_wallpaper=${s.wallpaper}`,
    `start_wallpaper_dim=${s.wallpaperDim}`,
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
  /** Off (the default), the page shows no speed dials: a blank tab, or only the widgets. */
  dials: boolean;
  search: boolean;
  clock: boolean;
  wallpaper: Wallpaper;
  wallDim: number;
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
  /** The stamp of the user's background picture (its bytes follow in `wall` messages), or empty. */
  wall: string;
  ack: 'none' | 'failed';
  error: string;
}
export interface WallPart { stamp: string; part: number; parts: number; data: string }
export type StartMessage =
  | { kind: 'page'; snap: StartSnapshot }
  | { kind: 'thumbs'; thumbs: Record<string, string> }
  | ({ kind: 'wall' } & WallPart);

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
  if (!isRecord(v) || !exactKeys(v, ['columns', 'thumbSize', 'titles', 'addButton', 'dragReorder', 'widgets', 'widgetColumns', 'widgetHeight', 'widgetBg', 'widgetColor', 'widgetDrag', 'dials', 'search', 'clock', 'wallpaper', 'wallDim'])) return null;
  if (!isInt(v.columns, MIN_COLUMNS, MAX_COLUMNS) || !isInt(v.thumbSize, MIN_THUMB_SIZE, MAX_THUMB_SIZE) || !oneOf(v.titles, TITLE_MODES)) return null;
  if (!isBool(v.addButton) || !isBool(v.dragReorder) || !isBool(v.widgets) || !isInt(v.widgetColumns, MIN_WIDGET_COLUMNS, MAX_WIDGET_COLUMNS)) return null;
  if (!oneOf(v.widgetHeight, WIDGET_HEIGHTS) || !oneOf(v.widgetBg, WIDGET_BGS) || typeof v.widgetColor !== 'string' || !COLOR_RE.test(v.widgetColor) || !isBool(v.widgetDrag) || !isBool(v.dials)) return null;
  if (!isBool(v.search) || !isBool(v.clock) || !oneOf(v.wallpaper, WALLPAPERS) || !isInt(v.wallDim, 0, MAX_WALL_DIM)) return null;
  return {
    columns: v.columns, thumbSize: v.thumbSize, titles: v.titles, addButton: v.addButton, dragReorder: v.dragReorder, widgets: v.widgets,
    widgetColumns: v.widgetColumns, widgetHeight: v.widgetHeight, widgetBg: v.widgetBg, widgetColor: v.widgetColor, widgetDrag: v.widgetDrag, dials: v.dials,
    search: v.search, clock: v.clock, wallpaper: v.wallpaper, wallDim: v.wallDim,
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
  if (!exactKeys(d, ['kind', 'guest', 'settings', 'groups', 'group', 'folder', 'path', 'dials', 'more', 'pickable', 'folders', 'widgets', 'favs', 'wall', 'ack', 'error'])) return null;
  if (typeof d.wall !== 'string' || !WALL_STAMP_RE.test(d.wall)) return null;
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
    favs: { ...(d.favs as Record<string, string>) }, wall: d.wall as string, ack: d.ack, error: d.error,
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
  if (d.kind === 'wall') {
    if (!exactKeys(d, ['kind', 'stamp', 'part', 'parts', 'data']) || typeof d.stamp !== 'string' || d.stamp === '' || !WALL_STAMP_RE.test(d.stamp)) return null;
    if (!isInt(d.parts, 1, MAX_WALL_PARTS) || !isInt(d.part, 0, d.parts - 1) || !isStr(d.data, MAX_WALL_PART) || !/^[A-Za-z0-9+/]*={0,2}$/.test(d.data)) return null;
    return { kind: 'wall', stamp: d.stamp, part: d.part, parts: d.parts, data: d.data };
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
  topsites: 'Most visited',
  bookmarks: 'Bookmarks',
  notes: 'Note',
  date: 'Clock',
  privacy: 'Protection',
  webpage: 'Web page',
};

/** One line for each kind in the add menu. */
export const WIDGET_HINTS: Readonly<Record<WidgetKind, string>> = {
  topsites: 'The sites you open most',
  bookmarks: 'A bookmark folder',
  notes: 'A note that saves as you type',
  date: 'The time and the date',
  privacy: 'Trackers and ads blocked',
  webpage: 'Any page, live in a card',
};

/** The big clock: the time in the person's own 12 or 24 hour style, and the date as words. */
export function clockParts(now: Date, locale?: string): { time: string; date: string } {
  const time = new Intl.DateTimeFormat(locale, { hour: 'numeric', minute: '2-digit' }).format(now);
  const date = new Intl.DateTimeFormat(locale, { weekday: 'long', day: 'numeric', month: 'long' }).format(now);
  return { time, date };
}

// ---- backgrounds ----

export type WallTone = 'dark' | 'light';
export interface WallArt { label: string; tone: WallTone; art: string }
/**
 * The pictures the page draws itself: layered gradients, no files and no network. `tone` is what the
 * picture is mostly like, so the text over it can be the colour that reads.
 */
export const WALL_ART: Readonly<Record<Exclude<Wallpaper, 'none' | 'custom'>, WallArt>> = {
  aurora: { label: 'Aurora', tone: 'dark', art: 'radial-gradient(120% 90% at 15% 10%, #1fb6a6 0%, transparent 55%), radial-gradient(90% 80% at 85% 20%, #6d4bd8 0%, transparent 60%), radial-gradient(120% 100% at 60% 100%, #0c4f6e 0%, transparent 70%), linear-gradient(160deg, #0b1324 0%, #121a33 100%)' },
  dusk: { label: 'Dusk', tone: 'dark', art: 'radial-gradient(110% 80% at 80% 100%, #ff8a4c 0%, transparent 55%), radial-gradient(100% 90% at 10% 90%, #d1477a 0%, transparent 60%), radial-gradient(120% 100% at 50% 0%, #3b2a6e 0%, transparent 70%), linear-gradient(180deg, #1d1838 0%, #48244f 100%)' },
  ocean: { label: 'Ocean', tone: 'dark', art: 'radial-gradient(100% 80% at 20% 0%, #2f86d6 0%, transparent 60%), radial-gradient(90% 90% at 90% 80%, #0fb3c4 0%, transparent 55%), linear-gradient(170deg, #0a2a52 0%, #06172e 100%)' },
  forest: { label: 'Forest', tone: 'dark', art: 'radial-gradient(110% 90% at 80% 10%, #6fb36b 0%, transparent 55%), radial-gradient(100% 80% at 10% 100%, #1f6b55 0%, transparent 60%), linear-gradient(165deg, #12301f 0%, #0b1d16 100%)' },
  blossom: { label: 'Blossom', tone: 'light', art: 'radial-gradient(100% 80% at 10% 0%, #ffd6e7 0%, transparent 60%), radial-gradient(90% 90% at 100% 30%, #e3d4ff 0%, transparent 60%), radial-gradient(110% 90% at 40% 100%, #ffe7d1 0%, transparent 65%), linear-gradient(160deg, #fdf3f7 0%, #f3ecfb 100%)' },
  sand: { label: 'Sand', tone: 'light', art: 'radial-gradient(110% 90% at 90% 0%, #f6d7a8 0%, transparent 60%), radial-gradient(100% 80% at 0% 100%, #e9c9a6 0%, transparent 60%), linear-gradient(170deg, #fbf4ea 0%, #f2e6d6 100%)' },
  graphite: { label: 'Graphite', tone: 'dark', art: 'radial-gradient(100% 80% at 20% 0%, #4a5160 0%, transparent 60%), radial-gradient(90% 80% at 100% 100%, #2b3140 0%, transparent 60%), linear-gradient(170deg, #22262e 0%, #15181d 100%)' },
  midnight: { label: 'Midnight', tone: 'dark', art: 'radial-gradient(80% 60% at 70% 20%, #3a3f9e 0%, transparent 60%), radial-gradient(70% 70% at 10% 80%, #5a2a86 0%, transparent 60%), linear-gradient(180deg, #070a1a 0%, #0d1030 100%)' },
};

/** The ink and glass over a background of `tone` (set as CSS custom properties, like `textOn`). */
export function wallInk(tone: WallTone): Readonly<Record<string, string>> {
  return tone === 'dark'
    ? {
      '--sp-ink': '#ffffff', '--sp-ink-2': 'rgba(255,255,255,0.78)', '--sp-glass': 'rgba(18,20,26,0.38)', '--sp-glass-hover': 'rgba(255,255,255,0.16)',
      '--sp-glass-edge': 'rgba(255,255,255,0.14)', '--sp-field': 'rgba(255,255,255,0.94)', '--sp-field-ink': '#1b1f26', '--sp-field-ink-2': '#5b6270', '--sp-dim-colour': '#000000',
    }
    : {
      '--sp-ink': '#14181d', '--sp-ink-2': 'rgba(20,24,29,0.72)', '--sp-glass': 'rgba(255,255,255,0.52)', '--sp-glass-hover': 'rgba(255,255,255,0.8)',
      '--sp-glass-edge': 'rgba(20,24,29,0.1)', '--sp-field': 'rgba(255,255,255,0.96)', '--sp-field-ink': '#1b1f26', '--sp-field-ink-2': '#5b6270', '--sp-dim-colour': '#ffffff',
    };
}

/** The mean luminance (0 to 1) of RGBA pixels: a picture above 0.6 takes dark text. */
export function toneOf(rgba: ArrayLike<number>): WallTone {
  let sum = 0;
  let n = 0;
  for (let i = 0; i + 3 < rgba.length; i += 4) {
    sum += (0.2126 * rgba[i]! + 0.7152 * rgba[i + 1]! + 0.0722 * rgba[i + 2]!) / 255;
    n += 1;
  }
  return n > 0 && sum / n > 0.6 ? 'light' : 'dark';
}

/** The four dim steps the Customise panel offers, and the step a saved value is nearest to. */
export const DIM_STEPS: ReadonlyArray<readonly [number, string]> = [[0, 'Off'], [20, 'Low'], [40, 'Medium'], [60, 'High']];
export function dimStep(value: number): number {
  return DIM_STEPS.reduce((best, [step]) => (Math.abs(step - value) < Math.abs(best - value) ? step : best), DIM_STEPS[0]![0]);
}

/** Size names for the five tile widths. */
export const SIZE_LABELS: readonly string[] = ['XS', 'S', 'M', 'L', 'XL'];

/** What the sticky note's save button says once it has saved. */
export const NOTE_SAVED = 'Saved to Notes';
