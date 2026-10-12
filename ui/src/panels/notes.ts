import { sendPanel, type Inbound, type PanelFields, type SnapshotMsg } from '../protocol';
import { notesCommand, parseNotesSnapshot, type Note, type NotesSnapshot, type Task } from './notes-logic';

function button(label: string, action: () => void): HTMLButtonElement {
  const el = document.createElement('button'); el.type = 'button'; el.textContent = label; el.addEventListener('click', action); return el;
}
function emit(a: string, id?: number, fields: { on?: boolean; text?: string } = {}): void {
  const command = notesCommand(a, id, fields);
  if (command) sendPanel('notes', command as PanelFields);
}

export function startNotesPanel(root: HTMLElement): (message: Inbound) => void {
  let data: NotesSnapshot | null = null;
  let active: 'notes' | 'tasks' = 'notes';
  let selectedNote: number | null = null;
  let selectedTask: number | null = null;
  let issue = '';
  const host = document.createElement('section'); host.className = 'notes-panel';
  const tabs = document.createElement('nav');
  const status = document.createElement('p'); status.setAttribute('role', 'status');
  const list = document.createElement('div'); list.setAttribute('role', 'list');
  host.append(tabs, status, list); root.replaceChildren(host);
  const render = (): void => {
    tabs.replaceChildren(button('Notes', () => { active = 'notes'; render(); }), button('Tasks', () => { active = 'tasks'; render(); }));
    list.replaceChildren();
    if (!data) { status.textContent = issue || 'Loading notes and tasks…'; return; }
    status.textContent = issue;
    if (active === 'notes') {
      tabs.append(button('Add note', () => emit('add-note')));
      for (const note of data.notes) renderNote(note);
      if (!data.notes.length) status.textContent = 'No notes yet.';
    } else {
      tabs.append(button('Add task', () => emit('add-task')));
      for (const task of data.tasks) renderTask(task);
      if (!data.tasks.length) status.textContent = 'No tasks yet.';
    }
  };
  const renderNote = (note: Note): void => {
    const row = document.createElement('article'); row.setAttribute('role', 'listitem');
    const title = document.createElement('input'); title.value = note.title; title.setAttribute('aria-label', 'Note title');
    const body = document.createElement('textarea'); body.value = note.body; body.setAttribute('aria-label', 'Note body');
    const save = button('Save', () => { emit('save-title', note.id, { text: title.value }); emit('save-body', note.id, { text: body.value }); });
    row.append(title, body, save, button('Delete', () => emit('delete-note', note.id))); list.append(row);
  };
  const renderTask = (task: Task): void => {
    const row = document.createElement('article'); row.setAttribute('role', 'listitem');
    const check = document.createElement('input'); check.type = 'checkbox'; check.checked = task.complete; check.setAttribute('aria-label', 'Complete task');
    check.addEventListener('change', () => emit('toggle-task', task.id, { on: check.checked }));
    const text = document.createElement('input'); text.value = task.text; text.setAttribute('aria-label', 'Task text');
    row.append(check, text, button('Save', () => emit('save-task', task.id, { text: text.value })), button('Delete', () => emit('delete-task', task.id))); list.append(row);
  };
  render();
  return (message: Inbound): void => {
    if (message.t !== 'snap' || message.p !== 'notes') return;
    const incoming = parseNotesSnapshot((message as SnapshotMsg).d);
    if (!incoming) { data = null; issue = 'Notes could not be loaded because the response was invalid.'; render(); return; }
    issue = ''; data = incoming;
    if (!incoming.notes.some(note => note.id === selectedNote)) selectedNote = incoming.notes[0]?.id ?? null;
    if (!incoming.tasks.some(task => task.id === selectedTask)) selectedTask = incoming.tasks[0]?.id ?? null;
    render();
  };
}
