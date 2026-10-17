export type Note = { id: number; title: string; body: string; addr: string; created: number; folder: number };
export type Task = { id: number; text: string; complete: boolean };
export type Folder = { id: number; name: string };
export const SORTS = ['manual', 'title', 'address', 'content', 'date'] as const;
export type Sort = typeof SORTS[number];
export const SORT_LABELS: Readonly<Record<Sort, string>> = { manual: 'Manual', title: 'Title', address: 'Address', content: 'Content', date: 'Date' };
export type NotesSnapshot = { notes: Note[]; tasks: Task[]; folders: Folder[]; sort: Sort };

export const MAX_BODY = 4000;
export const MAX_TITLE = 200;
export const MAX_FOLDER_NAME = 60;

const record = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const exact = (value: Record<string, unknown>, keys: readonly string[]): boolean => Object.keys(value).every(key => keys.includes(key)) && keys.every(key => key in value);
// Note and task ids are microsecond timestamps (about 1.8e15), so the bound is the safe-integer range, not u32.
const id = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) > 0;
const count = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) >= 0;
const text = (value: unknown, max = 2048): value is string => typeof value === 'string' && value.length <= max;

/** Reject the entire host payload before rendering any portion of it. */
export function parseNotesSnapshot(value: unknown): NotesSnapshot | null {
  if (!record(value) || !exact(value, ['notes', 'tasks', 'folders', 'sort']) || !Array.isArray(value.notes) || !Array.isArray(value.tasks) || !Array.isArray(value.folders)) return null;
  if (value.notes.length > 500 || value.tasks.length > 500 || value.folders.length > 100 || !(SORTS as readonly unknown[]).includes(value.sort)) return null;
  const notes: Note[] = [];
  const tasks: Task[] = [];
  const folders: Folder[] = [];
  const ids = new Set<number>();
  for (const raw of value.folders) {
    if (!record(raw) || !exact(raw, ['id', 'name']) || !id(raw.id) || !text(raw.name, 200) || ids.has(raw.id)) return null;
    ids.add(raw.id);
    folders.push({ id: raw.id, name: raw.name });
  }
  const folderIds = new Set(folders.map((folder) => folder.id));
  for (const raw of value.notes) {
    if (!record(raw) || !exact(raw, ['id', 'title', 'body', 'addr', 'created', 'folder']) || !id(raw.id) || !text(raw.title) || !text(raw.body, MAX_BODY + 96) || !text(raw.addr, 2100) || !count(raw.created) || !count(raw.folder) || ids.has(raw.id)) return null;
    if (raw.folder !== 0 && !folderIds.has(raw.folder)) return null;
    ids.add(raw.id);
    notes.push({ id: raw.id, title: raw.title, body: raw.body, addr: raw.addr, created: raw.created, folder: raw.folder });
  }
  for (const raw of value.tasks) {
    if (!record(raw) || !exact(raw, ['id', 'text', 'complete']) || !id(raw.id) || !text(raw.text) || typeof raw.complete !== 'boolean' || ids.has(raw.id)) return null;
    ids.add(raw.id);
    tasks.push({ id: raw.id, text: raw.text, complete: raw.complete });
  }
  return { notes, tasks, folders, sort: value.sort as Sort };
}

type Fields = { id?: number; parent?: number; before?: number; n?: number; on?: boolean; text?: string };
const ACTIONS = new Set([
  'add-note', 'save-title', 'save-body', 'delete-note', 'add-task', 'save-task', 'delete-task', 'toggle-task',
  'sort', 'move-note', 'note-folder', 'folder-add', 'folder-rename', 'folder-delete', 'export', 'import',
]);
/** What each action carries: the fields it must have, and none other. */
const SHAPES: Readonly<Record<string, readonly (keyof Fields)[]>> = {
  'add-note': [], 'add-task': [], import: [],
  'save-title': ['id', 'text'], 'save-body': ['id', 'text'], 'save-task': ['id', 'text'],
  'delete-note': ['id'], 'delete-task': ['id'], 'toggle-task': ['id', 'on'],
  sort: ['text'], 'folder-add': ['text'], 'folder-rename': ['id', 'text'], 'folder-delete': ['id'],
  'note-folder': ['id', 'parent'], export: ['n', 'text'],
};
/** Candidate actions use only fields accepted by the existing PanelFields wire shape. */
export function notesCommand(a: string, idValue?: number, fields: Omit<Fields, 'id'> = {}): ({ a: string } & Fields) | null {
  if (!ACTIONS.has(a) || (idValue !== undefined && !id(idValue))) return null;
  const all: Fields = { ...(idValue === undefined ? {} : { id: idValue }), ...fields };
  if (a === 'move-note') {
    // The note goes before `before`, or to the end when there is none.
    const keys = Object.keys(all);
    if (!keys.includes('id') || keys.some((key) => key !== 'id' && key !== 'before') || (all.before !== undefined && !id(all.before))) return null;
    return { a, ...all };
  }
  const shape = SHAPES[a];
  if (!shape) return null;
  const keys = Object.keys(all) as (keyof Fields)[];
  if (keys.length !== shape.length || !shape.every((key) => key in all && all[key] !== undefined)) return null;
  if (all.on !== undefined && typeof all.on !== 'boolean') return null;
  if (all.text !== undefined && !text(all.text, a === 'save-body' ? MAX_BODY : 2048)) return null;
  if (all.parent !== undefined && !count(all.parent)) return null;
  if (a === 'sort' && !(SORTS as readonly string[]).includes(all.text ?? '')) return null;
  if (a === 'export') {
    if (!['md', 'txt'].includes(all.text ?? '') || !(all.n === 0 || all.n === 1 || all.n === 2)) return null;
    if ((all.n === 0) !== (idValue === undefined)) return null;
  }
  return { a, ...all };
}

/** `export` carries its note or folder as `id` and the scope as `n` (0 every note, 1 a folder, 2 one note). */
export function exportCommand(scope: 'all' | 'folder' | 'note', idValue: number | undefined, kind: 'md' | 'txt'): ({ a: string } & Fields) | null {
  const n = scope === 'all' ? 0 : scope === 'folder' ? 1 : 2;
  const all: Fields = { n, text: kind, ...(scope === 'all' || idValue === undefined ? {} : { id: idValue }) };
  if (scope !== 'all' && idValue === undefined) return null;
  if (idValue !== undefined && !id(idValue)) return null;
  return { a: 'export', ...all };
}

// ---- Markdown ----
// A small, safe subset: headings, bold, italic, code, links, lists, quotes and fenced code. The result
// is data, never HTML: the panel builds elements from it with textContent, so no note can inject markup.

export type Inline = { t: 'text' | 'b' | 'i' | 'code'; s: string } | { t: 'link'; s: string; href: string };
export type Block =
  | { k: 'h'; level: number; inline: Inline[] }
  | { k: 'p'; inline: Inline[] }
  | { k: 'ul' | 'ol'; items: Inline[][] }
  | { k: 'quote'; inline: Inline[] }
  | { k: 'code'; s: string };

/** An address a note's link may point at: web pages and mail only. */
export function safeHref(href: string): string | null {
  const trimmed = href.trim();
  return /^(https?:\/\/|mailto:)[^\s<>"]+$/i.test(trimmed) && trimmed.length <= 2048 ? trimmed : null;
}

const INLINE = /(\*\*([^*\n]+)\*\*|__([^_\n]+)__|\*([^*\n]+)\*|_([^_\n]+)_|`([^`\n]+)`|!\[([^\]\n]*)\]\(([^)\s]*)\)|\[([^\]\n]+)\]\(([^)\s]+)\)|<(https?:\/\/[^>\s]+)>)/g;

export function parseInline(source: string): Inline[] {
  const out: Inline[] = [];
  let last = 0;
  for (const match of source.matchAll(INLINE)) {
    const at = match.index ?? 0;
    if (at > last) out.push({ t: 'text', s: source.slice(last, at) });
    const [, , bold1, bold2, em1, em2, code, imageAlt, , label, href, auto] = match;
    if (bold1 !== undefined || bold2 !== undefined) out.push({ t: 'b', s: bold1 ?? bold2 ?? '' });
    else if (em1 !== undefined || em2 !== undefined) out.push({ t: 'i', s: em1 ?? em2 ?? '' });
    else if (code !== undefined) out.push({ t: 'code', s: code });
    else if (imageAlt !== undefined) out.push({ t: 'text', s: `[${imageAlt || 'image'}]` });
    else if (label !== undefined && href !== undefined) {
      const safe = safeHref(href);
      out.push(safe ? { t: 'link', s: label, href: safe } : { t: 'text', s: label });
    } else if (auto !== undefined) {
      const safe = safeHref(auto);
      out.push(safe ? { t: 'link', s: auto, href: safe } : { t: 'text', s: auto });
    }
    last = at + match[0].length;
  }
  if (last < source.length) out.push({ t: 'text', s: source.slice(last) });
  return out;
}

export function parseMarkdown(source: string): Block[] {
  const lines = source.replace(/\r\n?/g, '\n').split('\n');
  const blocks: Block[] = [];
  let para: string[] = [];
  const flush = (): void => { if (para.length) { blocks.push({ k: 'p', inline: parseInline(para.join(' ')) }); para = []; } };
  for (let i = 0; i < lines.length && blocks.length < 400; i++) {
    const line = lines[i] ?? '';
    if (/^```/.test(line)) {
      flush();
      const code: string[] = [];
      for (i++; i < lines.length && !/^```/.test(lines[i] ?? ''); i++) code.push(lines[i] ?? '');
      blocks.push({ k: 'code', s: code.join('\n') });
      continue;
    }
    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    if (heading) { flush(); blocks.push({ k: 'h', level: (heading[1] ?? '#').length, inline: parseInline(heading[2] ?? '') }); continue; }
    const bullet = /^\s*[-*+]\s+(.*)$/.exec(line);
    const numbered = /^\s*\d+[.)]\s+(.*)$/.exec(line);
    if (bullet || numbered) {
      flush();
      const kind = bullet ? 'ul' : 'ol';
      const last = blocks[blocks.length - 1];
      const item = parseInline((bullet ?? numbered)?.[1] ?? '');
      if (last && last.k === kind) last.items.push(item); else blocks.push({ k: kind, items: [item] });
      continue;
    }
    const quote = /^>\s?(.*)$/.exec(line);
    if (quote) { flush(); blocks.push({ k: 'quote', inline: parseInline(quote[1] ?? '') }); continue; }
    if (line.trim() === '') { flush(); continue; }
    para.push(line.trim());
  }
  flush();
  return blocks;
}

/** Notes grouped for the list: the unfiled ones first, then each folder (empty folders too), in the snapshot's order. */
export function groupNotes(data: Pick<NotesSnapshot, 'notes' | 'folders'>): Array<{ folder: Folder | null; notes: Note[] }> {
  const groups: Array<{ folder: Folder | null; notes: Note[] }> = [{ folder: null, notes: data.notes.filter((note) => note.folder === 0) }];
  for (const folder of data.folders) groups.push({ folder, notes: data.notes.filter((note) => note.folder === folder.id) });
  return groups;
}

/** The host of a note's address, for the line under its title. */
export function addrHost(addr: string): string {
  try { return new URL(addr).host; } catch { return ''; }
}

/** "3 Mar 2026" for seconds since 1970; empty when unknown. */
export function dateText(seconds: number): string {
  if (!seconds) return '';
  const date = new Date(seconds * 1000);
  return Number.isNaN(date.getTime()) ? '' : date.toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' });
}
