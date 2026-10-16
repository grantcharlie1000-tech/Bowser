const MAX_ID = 0xffff_ffff;

export type BookmarkItem = {
  id: number;
  parent: number | null;
  title: string;
  kind: 'folder' | 'link';
  url?: string;
  folderPath?: string[];
};
export type BookmarkSnapshot = { items: BookmarkItem[]; query: string; currentFolder: number | null; /** Items open with one click (else two); absent in older hosts. */ singleClick: boolean };

type RecordValue = Record<string, unknown>;
const isRecord = (value: unknown): value is RecordValue => typeof value === 'object' && value !== null && !Array.isArray(value);
const isId = (value: unknown): value is number => typeof value === 'number' && Number.isSafeInteger(value) && value > 0 && value <= MAX_ID;
const isParent = (value: unknown): value is number | null => value === null || isId(value);

/** Validates the complete native snapshot before any part of it is rendered. */
export function validateBookmarkSnapshot(value: unknown): BookmarkSnapshot | null {
  if (!isRecord(value) || !Array.isArray(value.items) || typeof value.query !== 'string' || !(value.currentFolder === null || isId(value.currentFolder))) return null;
  if (value.singleClick !== undefined && typeof value.singleClick !== 'boolean') return null;
  const items: BookmarkItem[] = [];
  const ids = new Set<number>();
  for (const raw of value.items) {
    if (!isRecord(raw) || !isId(raw.id) || !isParent(raw.parent) || typeof raw.title !== 'string' || (raw.kind !== 'folder' && raw.kind !== 'link')) return null;
    if (raw.kind === 'link' && typeof raw.url !== 'string') return null;
    if (raw.kind === 'folder' && raw.url !== undefined) return null;
    if (raw.folderPath !== undefined && (!Array.isArray(raw.folderPath) || !raw.folderPath.every((part) => typeof part === 'string'))) return null;
    if (ids.has(raw.id)) return null;
    ids.add(raw.id);
    items.push({
      id: raw.id,
      parent: raw.parent,
      title: raw.title,
      kind: raw.kind,
      ...(typeof raw.url === 'string' ? { url: raw.url } : {}),
      ...(Array.isArray(raw.folderPath) ? { folderPath: raw.folderPath as string[] } : {}),
    });
  }
  return { items, query: value.query, currentFolder: value.currentFolder, singleClick: value.singleClick !== false };
}
export type BookmarkAction = 'open' | 'newtab' | 'openall' | 'delete' | 'rename' | 'update' | 'addfolder' | 'addlink';
export type BookmarkCommand = { v: 1; t: 'bm'; a: string; id?: number; parent?: number; before?: number; text?: string; url?: string };

/** Builds only fields supported by the existing flat bookmark bridge protocol. */
export function bookmarkActionCommand(action: BookmarkAction, id?: number, fields: { parent?: number | null; text?: string; url?: string } = {}): BookmarkCommand | null {
  if ((action === 'open' || action === 'newtab' || action === 'openall' || action === 'delete' || action === 'rename' || action === 'update') && !isId(id)) return null;
  if ((action === 'addfolder' || action === 'addlink') && !(fields.parent === null || isId(fields.parent))) return null;
  if ((action === 'rename' || action === 'addfolder' || action === 'addlink') && typeof fields.text !== 'string') return null;
  if ((action === 'update' || action === 'addlink') && typeof fields.url !== 'string') return null;
  return {
    v: 1,
    t: 'bm',
    a: action,
    ...(id == null ? {} : { id }),
    ...((action === 'addfolder' || action === 'addlink') ? { parent: fields.parent === null ? 0 : fields.parent } : {}),
    ...(fields.text === undefined ? {} : { text: fields.text }),
    ...(fields.url === undefined ? {} : { url: fields.url }),
  };
}

/** `before: 0` asks Rust to append at the destination's end. */
export function bookmarkMoveCommand(id: number, parent: number | null, before: number): BookmarkCommand | null {
  if (!isId(id) || !(parent === null || isId(parent)) || (before !== 0 && !isId(before))) return null;
  return { v: 1, t: 'bm', a: 'move', id, parent: parent === null ? 0 : parent, before };
}
