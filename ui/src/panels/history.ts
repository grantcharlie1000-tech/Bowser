import { sendPanel, type Inbound, type SnapshotMsg } from '../protocol';
import { historyOpenCommand, moveHistorySelection } from './history-logic';

type Entry = { id: number; title: string; host: string; clock: string };
type Day = { label: string; count: number };
type Row = Entry | Day;
const isEntry = (row: Row): row is Entry => 'id' in row;

export function startHistoryPanel(root: HTMLElement): (message: Inbound) => void {
  let rows: Row[] = [];
  let selected: number | null = null;
  let composing = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const search = document.createElement('input');
  search.type = 'search';
  search.placeholder = 'Search history';
  search.setAttribute('aria-label', 'Search history');
  const list = document.createElement('div');
  list.className = 'history-list';
  list.tabIndex = 0;
  list.setAttribute('role', 'listbox');
  const empty = document.createElement('p');
  empty.className = 'history-empty';
  const clear = document.createElement('div');
  clear.className = 'history-clear';
  for (const [label, range] of [['Last hour', 0], ['Today', 1], ['All time', 2]] as const) {
    const button = document.createElement('button');
    button.type = 'button';
    button.textContent = `Clear ${label.toLowerCase()}`;
    button.addEventListener('click', () => sendPanel('hist', { a: 'clear', n: range }));
    clear.append(button);
  }
  const host = document.createElement('section');
  host.className = 'history-panel';
  host.append(search, clear, list, empty);
  root.replaceChildren(host);

  const render = (): void => {
    list.replaceChildren();
    const ids = rows.filter(isEntry).map((row) => row.id);
    if (selected !== null && !ids.includes(selected)) selected = null;
    for (const row of rows) {
      if (!isEntry(row)) {
        const heading = document.createElement('h2');
        heading.textContent = `${row.label} (${row.count})`;
        list.append(heading);
        continue;
      }
      const item = document.createElement('div');
      item.className = 'history-row';
      item.setAttribute('role', 'option');
      item.setAttribute('aria-selected', String(selected === row.id));
      item.dataset.id = String(row.id);
      const favicon = document.createElement('span');
      favicon.className = 'history-favicon';
      favicon.setAttribute('aria-hidden', 'true');
      const text = document.createElement('span');
      text.className = 'history-text';
      const title = document.createElement('span');
      title.className = 'history-title';
      title.textContent = row.title || row.host;
      const details = document.createElement('span');
      details.className = 'history-details';
      details.textContent = `${row.host} · ${row.clock}`;
      text.append(title, details);
      const remove = document.createElement('button');
      remove.type = 'button';
      remove.textContent = 'Remove';
      remove.setAttribute('aria-label', `Remove ${row.title || row.host} from history`);
      remove.addEventListener('click', (event) => {
        event.stopPropagation();
        sendPanel('hist', { a: 'remove', id: row.id });
      });
      const removeSite = document.createElement('button');
      removeSite.type = 'button';
      removeSite.textContent = 'Remove site';
      removeSite.setAttribute('aria-label', `Remove all visits to ${row.host} from history`);
      removeSite.addEventListener('click', (event) => {
        event.stopPropagation();
        sendPanel('hist', { a: 'removesite', id: row.id });
      });
      item.append(favicon, text, remove, removeSite);
      item.addEventListener('click', () => {
        selected = row.id;
        const command = historyOpenCommand(row.id, false);
        if (command) sendPanel('hist', { a: command.a, id: command.id });
        render();
      });
      list.append(item);
    }
    empty.textContent = rows.length === 0 ? (search.value.trim() === '' ? 'Nothing in your history yet. Pages you visit appear here.' : 'No matches. Try a shorter word or check the spelling.') : '';
  };

  search.addEventListener('input', () => {
    if (composing) return;
    if (timer !== undefined) clearTimeout(timer);
    timer = setTimeout(() => sendPanel('hist', { a: 'search', text: search.value }), 120);
  });
  search.addEventListener('compositionstart', () => { composing = true; });
  search.addEventListener('compositionend', () => {
    composing = false;
    if (timer !== undefined) clearTimeout(timer);
    timer = setTimeout(() => sendPanel('hist', { a: 'search', text: search.value }), 120);
  });
  host.addEventListener('keydown', (event) => {
    if (event.isComposing || event.keyCode === 229) return;
    const ids = rows.filter(isEntry).map((row) => row.id);
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      selected = moveHistorySelection(ids, selected, event.key);
      render();
      list.querySelector(`[data-id="${selected ?? ''}"]`)?.scrollIntoView({ block: 'nearest' });
    } else if (event.key === 'Enter' && selected !== null) {
      const row = rows.find((item): item is Entry => isEntry(item) && item.id === selected);
      const command = row && historyOpenCommand(selected, event.ctrlKey);
      if (command) sendPanel('hist', { a: command.a, id: command.id });
    } else if (event.key === 'Delete' && selected !== null) {
      const row = rows.find((item): item is Entry => isEntry(item) && item.id === selected);
      if (row) sendPanel('hist', { a: 'remove', id: selected });
    } else if (event.key === 'Escape') {
      event.preventDefault();
      if (search.value !== '') {
        search.value = '';
        sendPanel('hist', { a: 'search', text: '' });
      } else sendPanel('panel', { a: 'dismiss' });
    }
  });

  const receive = (message: SnapshotMsg): void => {
    if (message.p !== 'history') return;
    const incoming = message.d.rows;
    if (!Array.isArray(incoming)) return;
    rows = incoming.filter((value): value is Row => {
      if (typeof value !== 'object' || value === null || Array.isArray(value)) return false;
      const row = value as Record<string, unknown>;
      if (typeof row.id === 'number') return Number.isSafeInteger(row.id) && row.id > 0 && row.id <= 0xffffffff && typeof row.title === 'string' && typeof row.host === 'string' && typeof row.clock === 'string';
      return typeof row.label === 'string' && typeof row.count === 'number' && Number.isSafeInteger(row.count) && row.count >= 0;
    });
    render();
  };
  return (message) => { if (message.t === 'snap') receive(message); };
}
