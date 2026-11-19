// Pure rules of the Bookmarks panel: the snapshot contract, the commands, the tree it draws and the
// address fix-up. No DOM here; test/panel-bookmarks.mjs covers all of it. The wire is
// src/chrome_host/panels/bookmarks.rs.
const MAX_ID = 0xffff_ffff;
/** "Other bookmarks" before its folder exists (Rust's `OTHER_WIRE`); picking it makes the folder. */
export const OTHER_WIRE = MAX_ID;
/** The bookmarks bar's own root: the top level of the tree (wire parent 0). Never a real id. */
export const BAR_ROOT = 0;
const FAV_PNG = /^data:image\/png;base64,[A-Za-z0-9+/=]{1,32768}$/;

export type BookmarkItem = {
  id: number;
  parent: number | null;
  title: string;
  kind: 'folder' | 'link';
  url?: string;
  /** The link's host (its favicon key). */
  host?: string;
  /** A folder's item count (its links may not all be in the snapshot). */
  n?: number;
  folderPath?: string[];
};
export type BookmarkSnapshot = {
  items: BookmarkItem[];
  query: string;
  currentFolder: number | null;
  /** Items open with one click (else two); absent in older hosts. */
  singleClick: boolean;
  /** The "Other bookmarks" folder id, or OTHER_WIRE while it does not exist. */
  other: number;
  /** Folders whose links the snapshot carries. */
  expanded: number[];
  /** How many search results there are (the snapshot may list fewer). */
  total: number;
  /** Why the last change was refused, or empty. */
  error: string;
  /** What the last delete removed, for its Undo, or empty. */
  undo: string;
  /** The shown tab can be bookmarked from the panel. */
  canAdd: boolean;
  /** Host -> favicon data URI. */
  favs: Record<string, string>;
};

type RecordValue = Record<string, unknown>;
const isRecord = (value: unknown): value is RecordValue => typeof value === 'object' && value !== null && !Array.isArray(value);
const isId = (value: unknown): value is number => typeof value === 'number' && Number.isSafeInteger(value) && value > 0 && value <= MAX_ID;
const isParent = (value: unknown): value is number | null => value === null || isId(value);
const isText = (value: unknown, max: number): value is string => typeof value === 'string' && value.length <= max;

/** Validates the complete native snapshot before any part of it is rendered. */
export function validateBookmarkSnapshot(value: unknown): BookmarkSnapshot | null {
  if (!isRecord(value) || !Array.isArray(value.items) || typeof value.query !== 'string' || !(value.currentFolder === null || isId(value.currentFolder))) return null;
  if (value.singleClick !== undefined && typeof value.singleClick !== 'boolean') return null;
  if (value.other !== undefined && !isId(value.other)) return null;
  if (value.expanded !== undefined && (!Array.isArray(value.expanded) || !value.expanded.every(isId))) return null;
  if (value.total !== undefined && !(typeof value.total === 'number' && Number.isSafeInteger(value.total) && value.total >= 0)) return null;
  if (value.error !== undefined && !isText(value.error, 400)) return null;
  if (value.undo !== undefined && !isText(value.undo, 400)) return null;
  if (value.canAdd !== undefined && typeof value.canAdd !== 'boolean') return null;
  const favs: Record<string, string> = {};
  if (value.favs !== undefined) {
    if (!isRecord(value.favs)) return null;
    for (const [host, uri] of Object.entries(value.favs)) {
      if (host.length > 253 || typeof uri !== 'string' || !FAV_PNG.test(uri)) return null;
      favs[host] = uri;
    }
  }
  const items: BookmarkItem[] = [];
  const ids = new Set<number>();
  for (const raw of value.items) {
    if (!isRecord(raw) || !isId(raw.id) || !isParent(raw.parent) || typeof raw.title !== 'string' || (raw.kind !== 'folder' && raw.kind !== 'link')) return null;
    if (raw.kind === 'link' && typeof raw.url !== 'string') return null;
    if (raw.kind === 'folder' && raw.url !== undefined) return null;
    if (raw.host !== undefined && !isText(raw.host, 253)) return null;
    if (raw.n !== undefined && !(typeof raw.n === 'number' && Number.isSafeInteger(raw.n) && raw.n >= 0)) return null;
    if (raw.folderPath !== undefined && (!Array.isArray(raw.folderPath) || !raw.folderPath.every((part) => typeof part === 'string'))) return null;
    if (ids.has(raw.id)) return null;
    ids.add(raw.id);
    items.push({
      id: raw.id,
      parent: raw.parent,
      title: raw.title,
      kind: raw.kind,
      ...(typeof raw.url === 'string' ? { url: raw.url } : {}),
      ...(typeof raw.host === 'string' ? { host: raw.host } : {}),
      ...(typeof raw.n === 'number' ? { n: raw.n } : {}),
      ...(Array.isArray(raw.folderPath) ? { folderPath: raw.folderPath as string[] } : {}),
    });
  }
  return {
    items,
    query: value.query,
    currentFolder: value.currentFolder,
    singleClick: value.singleClick !== false,
    other: isId(value.other) ? value.other : OTHER_WIRE,
    expanded: Array.isArray(value.expanded) ? (value.expanded as number[]) : [],
    total: typeof value.total === 'number' ? value.total : 0,
    error: typeof value.error === 'string' ? value.error : '',
    undo: typeof value.undo === 'string' ? value.undo : '',
    canAdd: value.canAdd === true,
    favs,
  };
}

// ---- the tree ----

/** The items directly inside `folder`: BAR_ROOT is the top level without "Other bookmarks". */
export function childrenOf(snapshot: BookmarkSnapshot, folder: number): BookmarkItem[] {
  if (folder === BAR_ROOT) return snapshot.items.filter((item) => item.parent === null && item.id !== snapshot.other);
  return snapshot.items.filter((item) => item.parent === folder);
}

/** How many items folder `folder` holds, counting those the snapshot left out. */
export function countOf(snapshot: BookmarkSnapshot, folder: number): number {
  if (folder === BAR_ROOT) return childrenOf(snapshot, BAR_ROOT).length;
  const item = snapshot.items.find((candidate) => candidate.id === folder);
  return item?.n ?? childrenOf(snapshot, folder).length;
}

/** The wire parent of a folder row: BAR_ROOT is 0, everything else its own id. */
export function wireParent(folder: number): number {
  return folder === BAR_ROOT ? 0 : folder;
}

/** The root a folder's chain of parents ends in: BAR_ROOT or the "Other bookmarks" id. */
export function rootOf(snapshot: BookmarkSnapshot, item: BookmarkItem): number {
  const seen = new Set<number>();
  let at: BookmarkItem | undefined = item;
  while (at && at.parent !== null && !seen.has(at.id)) {
    seen.add(at.id);
    const parent: number = at.parent;
    at = snapshot.items.find((candidate) => candidate.id === parent);
  }
  return at && at.id === snapshot.other ? snapshot.other : BAR_ROOT;
}

/** Whether `candidate` is `ancestor` or inside it. */
export function isWithin(snapshot: BookmarkSnapshot, candidate: number, ancestor: number): boolean {
  const seen = new Set<number>();
  let at: number | null = candidate;
  while (at !== null && !seen.has(at)) {
    if (at === ancestor) return true;
    seen.add(at);
    const current: number = at;
    at = snapshot.items.find((item) => item.id === current)?.parent ?? null;
  }
  return false;
}

/** A link's host for display, or the address itself. */
export function hostOf(url: string): string {
  try { return new URL(url).host || url; } catch { return url; }
}

/**
 * What a person typed as a bookmark's address, as a web address: a bare host or path gets
 * https:// (as every browser's bookmark editor does), anything with a scheme stays as typed.
 */
export function normalizeAddress(text: string): string {
  const value = text.trim();
  if (value === '') return '';
  if (/^[a-z][a-z0-9+.-]*:/i.test(value) && !/^[^/:]+:\d+(\/|$)/.test(value)) return value;
  if (value.startsWith('//')) return `https:${value}`;
  return `https://${value}`;
}

// ---- commands ----

export type BookmarkAction = 'open' | 'newtab' | 'openall' | 'copy' | 'delete' | 'rename' | 'update' | 'addfolder' | 'addlink' | 'addcurrent' | 'sort' | 'undo';
export type BookmarkCommand = { v: 1; t: 'bm'; a: string; id?: number; parent?: number; before?: number; text?: string; url?: string; on?: boolean };

const ID_ACTIONS: readonly BookmarkAction[] = ['open', 'newtab', 'openall', 'copy', 'delete', 'rename', 'update'];
const PARENT_ACTIONS: readonly BookmarkAction[] = ['addfolder', 'addlink', 'addcurrent', 'sort'];

/** Builds only fields supported by the flat bookmark bridge protocol. `parent` null is the bar. */
export function bookmarkActionCommand(action: BookmarkAction, id?: number | null, fields: { parent?: number | null; text?: string; url?: string } = {}): BookmarkCommand | null {
  if (ID_ACTIONS.includes(action) && !isId(id)) return null;
  const parent = fields.parent === null || fields.parent === BAR_ROOT ? 0 : fields.parent;
  if (PARENT_ACTIONS.includes(action) && !(parent === 0 || isId(parent))) return null;
  if ((action === 'rename' || action === 'addfolder' || action === 'addlink') && typeof fields.text !== 'string') return null;
  if ((action === 'update' || action === 'addlink') && typeof fields.url !== 'string') return null;
  return {
    v: 1,
    t: 'bm',
    a: action,
    ...(id == null || !ID_ACTIONS.includes(action) ? {} : { id }),
    ...(PARENT_ACTIONS.includes(action) ? { parent } : {}),
    ...(fields.text === undefined || !(action === 'rename' || action === 'update' || action === 'addfolder' || action === 'addlink') ? {} : { text: fields.text }),
    ...(fields.url === undefined || !(action === 'update' || action === 'addlink') ? {} : { url: fields.url }),
  };
}

/** `before: 0` asks Rust to append at the destination's end. `parent` null or BAR_ROOT is the bar. */
export function bookmarkMoveCommand(id: number, parent: number | null, before: number): BookmarkCommand | null {
  const wire = parent === null || parent === BAR_ROOT ? 0 : parent;
  if (!isId(id) || !(wire === 0 || isId(wire)) || (before !== 0 && !isId(before))) return null;
  return { v: 1, t: 'bm', a: 'move', id, parent: wire, before };
}

/** Opens (or closes) folder `id`'s list, so its links come with the next snapshot. */
export function bookmarkExpandCommand(id: number, open: boolean): BookmarkCommand | null {
  if (!isId(id) || id === OTHER_WIRE) return null;
  return { v: 1, t: 'bm', a: 'expand', id, on: open };
}
