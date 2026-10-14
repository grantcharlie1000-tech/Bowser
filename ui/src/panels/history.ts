import { sendPanel, type Inbound, type SnapshotMsg } from '../protocol';
import { el } from '../dom';
import { historyClickOpensNewTab, historyOpenCommand } from './history-logic';
import { emptyState, enter, faviconTile, iconButton, isFavUri, moveFocus, panelHeader, searchField, sectionHeader, textButton } from './kit';

type Entry = { id: number; title: string; host: string; clock: string };
type Day = { label: string; count: number };
type Row = Entry | Day;
const isEntry = (row: Row): row is Entry => 'id' in row;

export function startHistoryPanel(root: HTMLElement): (message: Inbound) => void {
  let rows: Row[] = [];
  /** Cached favicons by host (the snapshot's `favs`); a host without one gets the letter tile. */
  let favs: Record<string, string> = {};
  let selected: number | null = null;
  let pendingFocus: number | null = null;
  let composing = false;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const header = panelHeader('History', 'Close history');
  const search = searchField('Search history');
  const bar = el('div', 'pn-bar');
  bar.append(search.root);
  const list = el('div', 'pn-scroll bw-scroll');
  list.setAttribute('role', 'listbox');
  list.setAttribute('aria-label', 'History');
  const footer = el('footer', 'pn-footer');
  const footerLabel = el('span', 'pn-footer-label');
  footerLabel.textContent = 'Clear history';
  footer.append(footerLabel);
  for (const [label, range] of [['Last hour', 0], ['Today', 1], ['All time', 2]] as const) {
    footer.append(textButton(label, () => sendPanel('hist', { a: 'clear', n: range }), { subtle: true, aria: `Clear history: ${label.toLowerCase()}` }));
  }
  const host = el('section', 'pn');
  host.append(header.root, bar, list, footer);
  enter(host);
  root.replaceChildren(host);

  const entries = (): Entry[] => rows.filter(isEntry);
  const rowEls = (): HTMLElement[] => Array.from(list.querySelectorAll<HTMLElement>('.pn-row'));
  const rowFor = (id: number | null): HTMLElement | undefined => rowEls().find((node) => node.dataset.id === String(id));
  const openRow = (id: number, newTab: boolean): void => {
    const command = historyOpenCommand(id, newTab);
    if (command) sendPanel('hist', { a: command.a, id: command.id });
  };
  /** Moves the roving tab stop and selection to `id` without rebuilding the list. */
  const select = (id: number | null): void => {
    selected = id;
    const items = rowEls();
    const stop = items.find((node) => node.dataset.id === String(id)) ?? items[0];
    for (const node of items) {
      const on = node === stop;
      node.tabIndex = on ? 0 : -1;
      node.setAttribute('aria-selected', String(node.dataset.id === String(id)));
      for (const button of Array.from(node.querySelectorAll('button'))) button.tabIndex = on ? 0 : -1;
    }
  };
  const removeAndAdvance = (id: number, command: 'remove' | 'removesite'): void => {
    const all = entries();
    const index = all.findIndex((row) => row.id === id);
    pendingFocus = (all[index + 1] ?? all[index - 1])?.id ?? null;
    sendPanel('hist', { a: command, id });
  };

  const render = (): void => {
    const focused = list.contains(document.activeElement) && document.activeElement instanceof HTMLElement ? document.activeElement.closest<HTMLElement>('.pn-row')?.dataset.id : undefined;
    const ids = entries().map((row) => row.id);
    if (selected !== null && !ids.includes(selected)) selected = null;
    const nodes: HTMLElement[] = [];
    for (const row of rows) {
      if (!isEntry(row)) {
        const heading = sectionHeader(row.label);
        const count = el('span', 'pn-day-count');
        count.textContent = String(row.count);
        heading.append(count);
        nodes.push(heading);
        continue;
      }
      const item = el('div', 'bw-row bw-focus-inset pn-row');
      item.setAttribute('role', 'option');
      item.dataset.id = String(row.id);
      const name = row.title || row.host;
      const title = el('span', 'pn-row-title');
      title.textContent = name;
      title.setAttribute('data-tip', row.title && row.host ? `${row.title}\n${row.host}` : name);
      const trail = el('span', 'pn-trail');
      const meta = el('span', 'pn-meta');
      const where = el('span', 'pn-meta-host');
      where.textContent = row.host;
      const when = el('span', 'pn-meta-when');
      when.textContent = row.clock;
      meta.append(where, when);
      const actions = el('span', 'pn-actions');
      actions.append(
        iconButton('trash', `Remove ${name} from history`, (event) => { event.stopPropagation(); removeAndAdvance(row.id, 'remove'); }),
        iconButton('workspace-globe', `Remove all visits to ${row.host} from history`, (event) => { event.stopPropagation(); removeAndAdvance(row.id, 'removesite'); }),
      );
      trail.append(meta, actions);
      item.append(faviconTile(row.host || name, favs[row.host]), title, trail);
      item.addEventListener('click', (event) => { select(row.id); openRow(row.id, historyClickOpensNewTab(event)); });
      item.addEventListener('auxclick', (event) => { if (historyClickOpensNewTab(event)) { event.preventDefault(); select(row.id); openRow(row.id, true); } });
      nodes.push(item);
    }
    const searching = search.input.value.trim() !== '';
    if (rows.length === 0) {
      nodes.push(searching ? emptyState('search', 'No matches. Try a shorter word.') : emptyState('history', 'Pages you visit appear here.'));
    }
    list.replaceChildren(...nodes);
    footer.hidden = rows.length === 0 && !searching;
    select(selected);
    const wanted = pendingFocus ?? (focused ? Number(focused) : null);
    pendingFocus = null;
    if (wanted !== null) rowFor(wanted)?.focus();
  };

  const sendSearch = (): void => {
    if (timer !== undefined) clearTimeout(timer);
    timer = setTimeout(() => sendPanel('hist', { a: 'search', text: search.input.value }), 120);
  };
  search.input.addEventListener('input', () => { if (!composing) sendSearch(); });
  search.input.addEventListener('compositionstart', () => { composing = true; });
  search.input.addEventListener('compositionend', () => { composing = false; sendSearch(); });
  list.addEventListener('focusin', (event) => {
    const item = event.target instanceof HTMLElement ? event.target.closest<HTMLElement>('.pn-row') : null;
    if (item && item.dataset.id !== String(selected)) select(Number(item.dataset.id));
  });

  host.addEventListener('keydown', (event) => {
    if (event.isComposing || event.keyCode === 229) return;
    const target = event.target instanceof HTMLElement ? event.target : null;
    const item = target?.closest<HTMLElement>('.pn-row') ?? null;
    const id = item ? Number(item.dataset.id) : null;
    if (event.key === 'Escape') {
      if (search.input.value !== '') {
        event.preventDefault();
        search.input.value = '';
        sendPanel('hist', { a: 'search', text: '' });
      }
      return;
    }
    if (target === search.input) {
      if (event.key === 'ArrowDown') { event.preventDefault(); (rowFor(selected) ?? rowEls()[0])?.focus(); }
      else if (event.key === 'Enter' && selected !== null) openRow(selected, event.ctrlKey);
      return;
    }
    if (id === null) return;
    if (event.key === 'ArrowUp' && item === rowEls()[0]) { event.preventDefault(); search.input.focus(); return; }
    if (target === item && (event.key === 'Enter' || event.key === ' ')) { event.preventDefault(); openRow(id, event.ctrlKey); return; }
    if (target === item && event.key === 'Delete') { event.preventDefault(); removeAndAdvance(id, 'remove'); return; }
    moveFocus(list, '.pn-row', event);
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
    const incomingFavs = message.d.favs;
    favs = {};
    if (typeof incomingFavs === 'object' && incomingFavs !== null && !Array.isArray(incomingFavs)) {
      for (const [host, uri] of Object.entries(incomingFavs)) if (isFavUri(uri)) favs[host] = uri;
    }
    render();
  };
  render();
  return (message) => { if (message.t === 'snap') receive(message); };
}
