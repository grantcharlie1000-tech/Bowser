// The Sessions panel (`?view=dialog&p=sessions`): saved sessions, saving the open tabs (or the selected
// ones) as a new one, opening one with a double click, deleting. Rust owns the files; the page sends
// `sess` commands and draws what the `sessions` snapshot says. Names arrive as text, never HTML.
import { sendPanel, type Inbound, type PanelFields } from '../protocol';
import { el, icon } from '../dom';
import {
  deleteCommand, openCommand, openTabsText, parseSessionsSnapshot, saveCommand, summaryText, MAX_NAME,
  type SessionItem, type SessionsCommand, type SessionsSnapshot,
} from './sessions-logic';
import { emptyState, enter, iconButton, moveFocus, panelHeader, textButton } from './kit';

function emit(command: SessionsCommand | null): void {
  if (command) sendPanel('sess', command as PanelFields);
}

/** How long a Delete waits for its confirming second click. */
const CONFIRM_MS = 3000;

export function startSessionsPanel(root: HTMLElement): (message: Inbound) => void {
  let data: SessionsSnapshot | null = null;
  let saving = false;
  let selectedOnly = false;
  let confirming: string | null = null;
  let confirmTimer = 0;
  let lastFocus: string | null = null;
  /** The saved name a second Save is waiting to replace. */
  let armed: string | null = null;

  const header = panelHeader('Sessions', 'Close sessions');
  const info = el('p', 'pn-status ps-info');
  info.setAttribute('role', 'status');
  const saveButton = textButton('Save tabs as session', () => beginSave(), { icon: 'save' }); // sentence case and secondary, like Notes' Add note
  const bar = el('div', 'pn-bar ps-bar');
  bar.append(saveButton);

  // The inline form: a name and, when tabs are selected, which tabs the session keeps.
  const form = el('form', 'ps-form');
  form.hidden = true;
  const nameLabel = el('label', 'pn-label');
  nameLabel.textContent = 'Session name';
  const name = el('input', 'bw-input');
  name.id = 'ps-name';
  name.maxLength = MAX_NAME;
  name.autocomplete = 'off';
  name.spellcheck = false;
  nameLabel.htmlFor = name.id;
  const which = el('div', 'bw-segmented ps-which');
  which.setAttribute('role', 'radiogroup');
  which.setAttribute('aria-label', 'Tabs to save');
  const allBtn = el('button');
  const selBtn = el('button');
  for (const [button, value] of [[allBtn, false], [selBtn, true]] as const) {
    button.type = 'button';
    button.setAttribute('role', 'radio');
    button.addEventListener('click', () => { selectedOnly = value; paintWhich(); });
  }
  which.append(allBtn, selBtn);
  const error = el('p', 'pn-status ps-error');
  error.setAttribute('role', 'alert');
  const actions = el('div', 'ps-actions');
  const confirm = textButton('Save', () => submit(), { primary: true });
  confirm.type = 'submit';
  actions.append(confirm, textButton('Cancel', () => endSave(true)));
  form.append(nameLabel, name, which, error, actions);

  const list = el('div', 'pn-scroll bw-scroll ps-list');
  list.setAttribute('role', 'list');
  list.setAttribute('aria-label', 'Saved sessions');
  const host = el('section', 'pn ps');
  host.append(header.root, info, bar, form, list);
  enter(host);
  root.replaceChildren(host);

  const paintWhich = (): void => {
    const sel = data?.selected ?? 0;
    which.hidden = sel < 2;
    allBtn.textContent = `All ${data?.tabs ?? 0} tabs`;
    selBtn.textContent = `${sel} selected tabs`;
    if (sel < 2) selectedOnly = false;
    allBtn.setAttribute('aria-checked', String(!selectedOnly));
    selBtn.setAttribute('aria-checked', String(selectedOnly));
  };
  const beginSave = (): void => {
    if (!data?.allowed) return;
    saving = true;
    form.hidden = false;
    bar.hidden = true;
    error.textContent = '';
    armed = null;
    name.value = data.suggest;
    selectedOnly = data.selected > 1;
    paintWhich();
    name.focus();
    name.select();
  };
  const endSave = (focusBack: boolean): void => {
    saving = false;
    form.hidden = true;
    bar.hidden = false;
    if (focusBack) saveButton.focus();
  };
  const submit = (): void => {
    const command = saveCommand(name.value, selectedOnly);
    if (!command) { error.textContent = name.value.trim() === '' ? 'Enter a name.' : 'That name is not allowed.'; return; }
    // A name that is already saved replaces that session, so say so once and wait for a second Save.
    if (data?.items.some((item) => item.name === command.text) && armed !== command.text) {
      armed = command.text;
      error.textContent = `A session named “${command.text}” is already saved. Save again to replace it.`;
      return;
    }
    armed = null;
    emit(command);
    endSave(true);
  };
  name.addEventListener('input', () => { armed = null; error.textContent = ''; });
  form.addEventListener('submit', (event) => { event.preventDefault(); submit(); });
  form.addEventListener('keydown', (event) => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); endSave(true); } });

  const clearConfirm = (): void => {
    window.clearTimeout(confirmTimer);
    confirming = null;
  };
  const makeRow = (item: SessionItem): HTMLElement => {
    const row = el('div', 'bw-row bw-focus-inset pn-row ps-row');
    row.setAttribute('role', 'listitem');
    row.tabIndex = 0;
    row.dataset.name = item.name;
    const lead = el('span', 'pn-lead');
    lead.append(icon('folder'));
    const text = el('span', 'bw-row-text');
    const title = el('span', 'bw-row-title');
    title.textContent = item.name;
    title.dir = 'auto';
    const meta = el('span', 'bw-row-meta');
    meta.textContent = summaryText(item);
    text.append(title, meta);
    const tools = el('span', 'pn-actions');
    tools.append(
      iconButton('folder-open', 'Open in this window', (event) => { event.stopPropagation(); emit(openCommand(item.name, false)); }),
      iconButton('swap', 'Replace the open tabs', (event) => { event.stopPropagation(); emit(openCommand(item.name, true)); }),
    );
    const del = iconButton('trash', 'Delete session', (event) => {
      event.stopPropagation();
      if (confirming === item.name) { clearConfirm(); lastFocus = nextName(item.name); emit(deleteCommand(item.name)); return; }
      clearConfirm();
      confirming = item.name;
      confirmTimer = window.setTimeout(() => { confirming = null; paintList(); }, CONFIRM_MS);
      paintList();
    });
    if (confirming === item.name) {
      del.setAttribute('aria-label', 'Confirm: delete this session');
      del.setAttribute('data-tip', 'Click again to delete');
      del.classList.add('is-danger');
      row.classList.add('is-confirming');
    }
    tools.append(del);
    const trail = el('span', 'pn-trail');
    trail.append(tools);
    row.append(lead, text, trail);
    // A double click (or Enter) opens the session in this window.
    row.addEventListener('dblclick', () => emit(openCommand(item.name, false)));
    row.addEventListener('keydown', (event) => {
      if (event.target !== row) return;
      if (event.key === 'Enter') { event.preventDefault(); emit(openCommand(item.name, false)); }
      else if (event.key === 'Delete') { event.preventDefault(); del.click(); }
    });
    return row;
  };
  const nextName = (current: string): string | null => {
    const items = data?.items ?? [];
    const at = items.findIndex((item) => item.name === current);
    return (items[at + 1] ?? items[at - 1])?.name ?? null;
  };
  const paintList = (): void => {
    if (!data) return;
    if (!data.allowed) { list.replaceChildren(emptyState('person', 'A guest window does not keep sessions.')); return; }
    if (data.items.length === 0) { list.replaceChildren(emptyState('folder', 'No saved sessions yet. Save the open tabs to come back to them later.')); return; }
    list.replaceChildren(...data.items.map(makeRow));
    if (lastFocus !== null) {
      Array.from(list.querySelectorAll<HTMLElement>('.ps-row')).find((row) => row.dataset.name === lastFocus)?.focus();
      lastFocus = null;
    }
  };
  list.addEventListener('keydown', (event) => { moveFocus(list, '.ps-row', event); });

  const apply = (snapshot: SessionsSnapshot): void => {
    data = snapshot;
    if (confirming !== null && !snapshot.items.some((item) => item.name === confirming)) clearConfirm();
    info.textContent = snapshot.allowed ? openTabsText(snapshot.tabs, snapshot.selected) : '';
    saveButton.disabled = !snapshot.allowed || snapshot.tabs === 0;
    if (!snapshot.allowed && saving) endSave(false);
    if (saving) paintWhich();
    paintList();
  };

  return (message) => {
    if (message.t !== 'snap' || message.p !== 'sessions') return;
    const parsed = parseSessionsSnapshot(message.d);
    if (parsed) apply(parsed);
  };
}
