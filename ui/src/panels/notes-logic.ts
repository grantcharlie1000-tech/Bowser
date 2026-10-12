export type Note = { id: number; title: string; body: string };
export type Task = { id: number; text: string; complete: boolean };
export type NotesSnapshot = { notes: Note[]; tasks: Task[] };

const record = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const exact = (value: Record<string, unknown>, keys: readonly string[]): boolean => Object.keys(value).every(key => keys.includes(key)) && keys.every(key => key in value);
const id = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) > 0 && (value as number) <= 0xffff_ffff;
const text = (value: unknown): value is string => typeof value === 'string' && value.length <= 2048;

/** Reject the entire host payload before rendering any portion of it. */
export function parseNotesSnapshot(value: unknown): NotesSnapshot | null {
  if (!record(value) || !exact(value, ['notes', 'tasks']) || !Array.isArray(value.notes) || !Array.isArray(value.tasks) || value.notes.length > 500 || value.tasks.length > 500) return null;
  const notes: Note[] = [];
  const tasks: Task[] = [];
  const ids = new Set<number>();
  for (const raw of value.notes) {
    if (!record(raw) || !exact(raw, ['id', 'title', 'body']) || !id(raw.id) || !text(raw.title) || !text(raw.body) || ids.has(raw.id)) return null;
    ids.add(raw.id);
    notes.push({ id: raw.id, title: raw.title, body: raw.body });
  }
  for (const raw of value.tasks) {
    if (!record(raw) || !exact(raw, ['id', 'text', 'complete']) || !id(raw.id) || !text(raw.text) || typeof raw.complete !== 'boolean' || ids.has(raw.id)) return null;
    ids.add(raw.id);
    tasks.push({ id: raw.id, text: raw.text, complete: raw.complete });
  }
  return { notes, tasks };
}

type Fields = { id?: number; on?: boolean; text?: string };
const ACTIONS = new Set(['add-note', 'save-title', 'save-body', 'delete-note', 'add-task', 'save-task', 'delete-task', 'toggle-task']);
/** Candidate actions use only fields accepted by the existing PanelFields wire shape. */
export function notesCommand(a: string, idValue?: number, fields: Omit<Fields, 'id'> = {}): ({ a: string } & Fields) | null {
  if (!ACTIONS.has(a) || (idValue !== undefined && !id(idValue))) return null;
  if (fields.on !== undefined && typeof fields.on !== 'boolean') return null;
  if (fields.text !== undefined && !text(fields.text)) return null;
  const needsId = ['save-title', 'save-body', 'delete-note', 'save-task', 'delete-task', 'toggle-task'].includes(a);
  if (needsId !== (idValue !== undefined)) return null;
  if (a === 'toggle-task' && typeof fields.on !== 'boolean') return null;
  if (['save-title', 'save-body', 'save-task'].includes(a) && typeof fields.text !== 'string') return null;
  if (fields.on !== undefined && a !== 'toggle-task') return null;
  if (fields.text !== undefined && !['save-title', 'save-body', 'save-task'].includes(a)) return null;
  return { a, ...(idValue === undefined ? {} : { id: idValue }), ...fields };
}
