import { sendPanel, type Inbound, type PanelFields, type SnapshotMsg } from '../protocol';
import { el, icon, place } from '../dom';
import { notesCommand, parseNotesSnapshot, type Note, type NotesSnapshot, type Task } from './notes-logic';
import { emptyState, enter, iconButton, moveFocus, panelHeader, sectionHeader, textButton } from './kit';
import { preview } from './kit-logic';

function emit(a: string, id?: number, fields: { on?: boolean; text?: string } = {}): void {
  const command = notesCommand(a, id, fields);
  if (command) sendPanel('notes', command as PanelFields);
}

type NoteView = { root: HTMLElement; row: HTMLElement; name: HTMLElement; peek: HTMLElement; title: HTMLInputElement; body: HTMLTextAreaElement; note: Note };
type TaskView = { root: HTMLElement; check: HTMLButtonElement; input: HTMLInputElement; task: Task };

let uid = 0;

/** Writes a value into a field the user is not editing right now. */
function sync(field: HTMLInputElement | HTMLTextAreaElement, value: string): void {
  if (document.activeElement !== field && field.value !== value) field.value = value;
}

export function startNotesPanel(root: HTMLElement): (message: Inbound) => void {
  let data: NotesSnapshot | null = null;
  let active: 'notes' | 'tasks' = 'notes';
  let openNote: number | null = null;
  let issue = '';
  let awaiting: 'notes' | 'tasks' | null = null;
  let known = new Set<number>();
  let pendingFocus: number | null = null;
  const noteViews = new Map<number, NoteView>();
  const taskViews = new Map<number, TaskView>();

  const header = panelHeader('Notes and tasks', 'Close notes and tasks');
  const tabs = el('div', 'bw-segmented pn-tabs');
  tabs.setAttribute('role', 'tablist');
  tabs.setAttribute('aria-label', 'Notes or tasks');
  const tabNotes = el('button');
  const tabTasks = el('button');
  for (const [tab, key, label] of [[tabNotes, 'notes', 'Notes'], [tabTasks, 'tasks', 'Tasks']] as const) {
    tab.type = 'button';
    tab.setAttribute('role', 'tab');
    tab.id = `pn-tab-${key}`;
    tab.setAttribute('aria-controls', 'pn-notes-list');
    tab.dataset.label = label;
    tab.addEventListener('click', () => switchTo(key));
  }
  tabs.append(tabNotes, tabTasks);
  const add = textButton('Add note', () => {
    if (!data) return;
    awaiting = active;
    known = new Set([...data.notes.map((note) => note.id), ...data.tasks.map((task) => task.id)]);
    emit(active === 'notes' ? 'add-note' : 'add-task');
  }, { icon: 'plus' });
  const bar = el('div', 'pn-bar pn-bar-row');
  bar.append(tabs, add);
  const status = el('p', 'pn-status');
  status.setAttribute('role', 'status');
  const list = el('div', 'pn-scroll bw-scroll');
  list.id = 'pn-notes-list';
  list.setAttribute('role', 'tabpanel');
  const host = el('section', 'pn pn-notes');
  host.append(header.root, bar, status, list);
  enter(host);
  root.replaceChildren(host);

  const switchTo = (key: 'notes' | 'tasks'): void => { active = key; render(); };

  const makeNote = (note: Note): NoteView => {
    const id = ++uid;
    const wrap = el('div', 'pn-note');
    const row = el('div', 'bw-row bw-focus-inset pn-row pn-note-row');
    row.tabIndex = 0;
    row.setAttribute('role', 'button');
    row.dataset.id = String(note.id);
    const lead = el('span', 'pn-lead');
    lead.append(icon('notes'));
    const name = el('span', 'pn-row-title');
    const peek = el('span', 'pn-peek');
    const actions = el('span', 'pn-actions is-solo');
    actions.append(iconButton('trash', 'Delete note', (event) => { event.stopPropagation(); pendingFocus = neighbour(note.id); emit('delete-note', note.id); }));
    const trail = el('span', 'pn-trail');
    trail.append(actions);
    row.append(lead, name, peek, trail);
    const editor = el('div', 'pn-editor');
    const titleLabel = el('label', 'pn-label');
    titleLabel.htmlFor = `pn-title-${id}`;
    titleLabel.textContent = 'Title';
    const title = el('input', 'bw-input');
    title.id = `pn-title-${id}`;
    title.autocomplete = 'off';
    title.spellcheck = false;
    const bodyLabel = el('label', 'pn-label');
    bodyLabel.htmlFor = `pn-body-${id}`;
    bodyLabel.textContent = 'Note';
    const body = el('textarea', 'bw-input pn-textarea');
    body.id = `pn-body-${id}`;
    body.rows = 6;
    editor.append(titleLabel, title, bodyLabel, body);
    wrap.append(row, editor);
    const view: NoteView = { root: wrap, row, name, peek, title, body, note };
    const toggle = (): void => { openNote = openNote === view.note.id ? null : view.note.id; applyOpen(); if (openNote !== null) view.title.focus(); };
    row.addEventListener('click', toggle);
    row.addEventListener('keydown', (event) => {
      if (event.target !== row) return;
      if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); toggle(); }
      else if (event.key === 'Delete') { event.preventDefault(); pendingFocus = neighbour(view.note.id); emit('delete-note', view.note.id); }
    });
    title.addEventListener('change', () => emit('save-title', view.note.id, { text: title.value }));
    body.addEventListener('change', () => emit('save-body', view.note.id, { text: body.value }));
    editor.addEventListener('keydown', (event) => {
      if (event.key === 'Enter' && event.ctrlKey && event.target instanceof HTMLElement) { event.preventDefault(); event.target.dispatchEvent(new Event('change')); }
      else if (event.key === 'Escape' && openNote === view.note.id) { event.preventDefault(); event.stopPropagation(); openNote = null; applyOpen(); row.focus(); }
    });
    return view;
  };
  const paintNote = (view: NoteView, note: Note): void => {
    view.note = note;
    view.name.textContent = note.title || 'Untitled note';
    view.peek.textContent = preview(note.body, 60);
    sync(view.title, note.title);
    sync(view.body, note.body);
    view.title.setAttribute('aria-label', 'Note title');
  };
  const applyOpen = (): void => {
    for (const [id, view] of noteViews) {
      const on = id === openNote;
      view.root.classList.toggle('is-open', on);
      view.row.setAttribute('aria-expanded', String(on));
      view.row.classList.toggle('is-selected', on);
    }
  };
  const neighbour = (id: number): number | null => {
    const all = data?.[active] ?? [];
    const index = all.findIndex((item) => item.id === id);
    return (all[index + 1] ?? all[index - 1])?.id ?? null;
  };

  const makeTask = (task: Task): TaskView => {
    const root = el('div', 'bw-row pn-task');
    const check = el('button', 'pn-check');
    check.type = 'button';
    check.setAttribute('role', 'checkbox');
    check.append(icon('check', { size: 12 }));
    const input = el('input', 'pn-task-input');
    input.autocomplete = 'off';
    input.spellcheck = false;
    input.setAttribute('aria-label', 'Task');
    const actions = el('span', 'pn-actions is-solo');
    actions.append(iconButton('trash', 'Delete task', () => { pendingFocus = neighbour(view.task.id); emit('delete-task', view.task.id); }));
    const trail = el('span', 'pn-trail');
    trail.append(actions);
    root.append(check, input, trail);
    const view: TaskView = { root, check, input, task };
    check.addEventListener('click', () => emit('toggle-task', view.task.id, { on: !view.task.complete }));
    input.addEventListener('change', () => emit('save-task', view.task.id, { text: input.value }));
    input.addEventListener('keydown', (event) => {
      if (event.key === 'Enter') { event.preventDefault(); input.dispatchEvent(new Event('change')); input.blur(); }
    });
    return view;
  };
  const paintTask = (view: TaskView, task: Task): void => {
    view.task = task;
    view.root.classList.toggle('is-done', task.complete);
    view.check.setAttribute('aria-checked', String(task.complete));
    view.check.setAttribute('aria-label', `Mark ${task.text || 'task'} ${task.complete ? 'not done' : 'done'}`);
    sync(view.input, task.text);
  };

  const render = (): void => {
    const count = (n: number): string => (n > 0 ? ` (${n})` : '');
    tabNotes.textContent = `Notes${count(data?.notes.length ?? 0)}`;
    tabTasks.textContent = `Tasks${count(data?.tasks.filter((task) => !task.complete).length ?? 0)}`;
    for (const [tab, key] of [[tabNotes, 'notes'], [tabTasks, 'tasks']] as const) {
      tab.setAttribute('aria-selected', String(active === key));
      tab.tabIndex = active === key ? 0 : -1;
    }
    list.setAttribute('aria-labelledby', `pn-tab-${active}`);
    const addLabel = active === 'notes' ? 'Add note' : 'Add task';
    const addText = el('span', 'pn-add-label');
    addText.textContent = addLabel;
    add.replaceChildren(icon('plus'), addText);
    add.setAttribute('aria-label', addLabel);
    add.setAttribute('data-tip', addLabel);
    status.textContent = issue;
    status.hidden = issue === '';
    if (!data) {
      if (!issue) { status.hidden = false; status.textContent = 'Loading…'; }
      list.replaceChildren();
      return;
    }
    const nodes: HTMLElement[] = [];
    if (active === 'notes') {
      const live = new Set<number>();
      for (const note of data.notes) {
        live.add(note.id);
        let view = noteViews.get(note.id);
        if (!view) { view = makeNote(note); noteViews.set(note.id, view); }
        paintNote(view, note);
        nodes.push(view.root);
      }
      for (const id of [...noteViews.keys()]) if (!live.has(id)) noteViews.delete(id);
      if (openNote !== null && !live.has(openNote)) openNote = null;
      if (nodes.length === 0) nodes.push(emptyState('notes', 'No notes yet. Add one to get started.'));
    } else {
      const live = new Set<number>();
      const todo: HTMLElement[] = [];
      const done: HTMLElement[] = [];
      for (const task of data.tasks) {
        live.add(task.id);
        let view = taskViews.get(task.id);
        if (!view) { view = makeTask(task); taskViews.set(task.id, view); }
        paintTask(view, task);
        (task.complete ? done : todo).push(view.root);
      }
      for (const id of [...taskViews.keys()]) if (!live.has(id)) taskViews.delete(id);
      if (todo.length) nodes.push(sectionHeader('To do'), ...todo);
      if (done.length) nodes.push(sectionHeader(`Completed (${done.length})`), ...done);
      if (nodes.length === 0) nodes.push(emptyState('check', 'No tasks yet. Add one to get started.'));
    }
    place(list, nodes);
    applyOpen();
    if (pendingFocus !== null) {
      const view = noteViews.get(pendingFocus)?.row ?? taskViews.get(pendingFocus)?.input;
      pendingFocus = null;
      view?.focus();
    }
  };

  tabs.addEventListener('keydown', (event) => {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
    event.preventDefault();
    switchTo(active === 'notes' ? 'tasks' : 'notes');
    (active === 'notes' ? tabNotes : tabTasks).focus();
  });
  list.addEventListener('keydown', (event) => {
    const target = event.target;
    if (!(target instanceof HTMLElement)) return;
    if (target.classList.contains('pn-note-row')) moveFocus(list, '.pn-note-row', event);
    else if (target.classList.contains('pn-check')) moveFocus(list, '.pn-check', event);
    else if (target.classList.contains('pn-task-input') && (event.key === 'ArrowDown' || event.key === 'ArrowUp')) moveFocus(list, '.pn-task-input', event);
  });

  render();
  return (message: Inbound): void => {
    if (message.t !== 'snap' || message.p !== 'notes') return;
    const incoming = parseNotesSnapshot((message as SnapshotMsg).d);
    if (!incoming) { data = null; issue = 'Notes could not be loaded because the response was invalid.'; render(); return; }
    issue = '';
    data = incoming;
    if (awaiting) {
      const fresh = (awaiting === 'notes' ? incoming.notes : incoming.tasks).find((item) => !known.has(item.id));
      awaiting = null;
      if (fresh) {
        if (active === 'notes') openNote = fresh.id;
        render();
        const target = active === 'notes' ? noteViews.get(fresh.id)?.title : taskViews.get(fresh.id)?.input;
        target?.focus();
        target?.select();
        return;
      }
    }
    render();
  };
}
