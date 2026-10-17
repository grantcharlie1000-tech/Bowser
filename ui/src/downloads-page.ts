// The downloads page (?view=downloads), drawn by Bowser over a `bowser://downloads` tab
// (docs/design/downloads.md "Page", styles/downloads.css), built the way the history page is. Rust
// sends `state` (theme) and `snap` (p "downloadspage"); the page sends `{t:"dpage",a,...}` commands
// only, and every one is checked again in Rust. Text from Rust reaches the DOM through textContent
// only. The page never changes its own URL and has no links: rows are buttons, and Bowser opens
// the file or the address. Live progress arrives as fresh snapshots (4/s while bytes move): rows
// are patched in place by id, never rebuilt.
import { onInbound, send, sendPanel, type Inbound, type PanelFields } from './protocol';
import { applyTheme } from './view';
import { el, icon, place, type IconName } from './dom';
import { textButton } from './panels/kit';
import { moveIndex } from './history-page-logic';
import {
  ActionGate, announceText, canAct, dayHeading, emptyView, countText, matchRanges, MAX_QUERY, pageOrder, parseDownloadsSnapshot, removable, summaryText,
  type DlAction, type DlDay, type DownloadsSnapshot, type EmptyView,
} from './downloads-logic';
import { fillRow, makeRow, rowParts, type RowNode } from './downloads-view';

const reduceMotion = (): boolean => matchMedia('(prefers-reduced-motion: reduce)').matches;
/** How long the search waits for the next keystroke before asking Bowser. */
const SEARCH_DELAY = 90;

/** `dpage` action per row action (docs/design/downloads.md "Page"). */
const PAGE_ACTION: Readonly<Record<DlAction, string>> = {
  pause: 'pause', resume: 'resume', cancel: 'cancel', open: 'open', show: 'show', remove: 'remove', copy: 'copy', retry: 'retry',
};

function cmd(fields: PanelFields): boolean {
  return sendPanel('dpage', fields);
}

/** Writes `text` into `target`, wrapping the search hits in `<mark>`; untouched when unchanged. */
function marked(target: HTMLElement, text: string, query: string): void {
  const key = `${query}\u0000${text}`;
  if (target.dataset.key === key) return;
  target.dataset.key = key;
  const ranges = matchRanges(text, query);
  if (ranges.length === 0) { target.textContent = text; return; }
  const parts: Node[] = [];
  let at = 0;
  for (const [start, end] of ranges) {
    if (start > at) parts.push(document.createTextNode(text.slice(at, start)));
    const hit = el('mark', 'dp-hit');
    hit.textContent = text.slice(start, end);
    parts.push(hit);
    at = end;
  }
  if (at < text.length) parts.push(document.createTextNode(text.slice(at)));
  target.replaceChildren(...parts);
}

interface DayNode {
  readonly root: HTMLElement;
  readonly title: HTMLElement;
  readonly caption: HTMLElement;
  readonly count: HTMLElement;
  readonly list: HTMLUListElement;
}

export function startDownloadsPage(root: HTMLElement): void {
  root.classList.add('dp-root');
  let snapshot: DownloadsSnapshot | null = null;
  let entered = false;
  let query = '';
  let composing = false;
  let searchTimer = 0;
  /** The row that holds the keyboard stop (roving tabindex), by entry id. */
  let current: number | null = null;
  let pendingFocus: number | null | 'search' = null;
  let removedName = '';
  let renderedQuery = '';
  let confirming = false;
  const seen = new Set<number>();
  const gate = new ActionGate();

  // ---- skeleton ----
  const page = el('div', 'dp bw-scroll');
  const head = el('header', 'dp-head');
  const headIn = el('div', 'dp-head-in');
  const top = el('div', 'dp-top');
  const brand = el('div', 'dp-brand');
  const mark = el('span', 'dp-mark');
  mark.append(icon('download', { size: 20 }));
  const titles = el('div', 'dp-titles');
  const h1 = el('h1', 'dp-title');
  h1.textContent = 'Downloads';
  const summary = el('p', 'dp-summary');
  titles.append(h1, summary);
  brand.append(mark, titles);
  const tools = el('div', 'dp-tools');
  const openFolder = textButton('Open downloads folder', () => cmd({ a: 'folder' }), { icon: 'folder-download' });
  openFolder.classList.add('dp-folder');
  openFolder.setAttribute('aria-label', 'Open downloads folder');
  const clearAll = textButton('Clear all', () => {
    if (!confirming) { confirming = true; paintClear(); clearCancel.focus(); return; }
    confirming = false;
    paintClear();
    cmd({ a: 'clear' });
  }, { icon: 'trash' });
  clearAll.classList.add('dp-clear');
  const clearCancel = textButton('Cancel', () => { confirming = false; paintClear(); clearAll.focus(); });
  clearCancel.classList.add('dp-clear-cancel');
  const clearNote = el('span', 'dp-clear-note');
  clearNote.textContent = 'Files stay on disk.';
  clearNote.id = 'dp-clear-note';
  const settings = el('button', 'bw-icon-btn is-lg is-round dp-settings');
  settings.type = 'button';
  settings.setAttribute('aria-label', 'Download settings');
  settings.append(icon('gear'));
  settings.addEventListener('click', () => cmd({ a: 'settings' }));
  tools.append(clearNote, clearCancel, clearAll, openFolder, settings);
  top.append(brand, tools);

  const search = el('div', 'dp-search');
  search.setAttribute('role', 'search');
  const input = el('input', 'dp-search-input');
  input.type = 'search';
  input.placeholder = 'Search downloads';
  input.autocomplete = 'off';
  input.spellcheck = false;
  input.maxLength = MAX_QUERY;
  input.setAttribute('aria-label', 'Search downloads');
  input.setAttribute('aria-controls', 'dp-list');
  input.setAttribute('aria-describedby', 'dp-count');
  const clear = el('button', 'bw-icon-btn dp-search-clear');
  clear.type = 'button';
  clear.hidden = true;
  clear.setAttribute('aria-label', 'Clear search');
  clear.append(icon('close', { size: 12, fine: true }));
  const keycap = el('kbd', 'dp-key');
  keycap.textContent = '/';
  keycap.setAttribute('aria-hidden', 'true');
  search.append(icon('search', { className: 'dp-search-icon' }), input, keycap, clear);
  const guestNote = el('p', 'dp-note');
  guestNote.hidden = true;
  guestNote.append(icon('person', { size: 14 }), document.createTextNode('Guest window: this list is forgotten when the window closes. The files stay in your downloads folder.'));
  headIn.append(top, search, guestNote);
  head.append(headIn);

  const main = el('main', 'dp-main');
  main.setAttribute('aria-label', 'Downloads');
  const column = el('div', 'dp-col');
  const list = el('div', 'dp-list');
  list.id = 'dp-list';
  const empty = el('div', 'dp-empty');
  empty.hidden = true;
  const foot = el('p', 'dp-foot');
  foot.hidden = true;
  column.append(list, empty, foot);
  main.append(column);

  const count = el('p', 'dp-sr');
  count.id = 'dp-count';
  count.setAttribute('role', 'status');
  count.setAttribute('aria-live', 'polite');
  const toast = el('div', 'dp-toast');
  toast.setAttribute('role', 'status');
  toast.setAttribute('aria-live', 'polite');
  const toastIcon = el('span', 'dp-toast-icon');
  const toastText = el('span', 'dp-toast-text');
  const undo = el('button', 'dp-toast-undo');
  undo.type = 'button';
  undo.textContent = 'Undo';
  undo.setAttribute('aria-keyshortcuts', 'Control+Z');
  toast.append(toastIcon, toastText, undo);
  page.append(head, main, count, toast);
  root.replaceChildren(page);

  function paintClear(): void {
    clearAll.lastChild!.textContent = confirming ? 'Clear all downloads' : 'Clear all';
    clearAll.classList.toggle('is-danger', confirming);
    clearAll.toggleAttribute('aria-describedby', confirming);
    if (confirming) clearAll.setAttribute('aria-describedby', clearNote.id);
    clearCancel.hidden = !confirming;
    clearNote.hidden = !confirming;
    tools.classList.toggle('is-confirming', confirming);
  }
  paintClear();

  // ---- toast: "Removed", with Undo ----
  let toastTimer = 0;
  const hideToast = (): void => { toast.classList.remove('is-on'); undo.hidden = true; };
  const showToast = (text: string, kind: 'ok' | 'undo' | 'error'): void => {
    toastIcon.replaceChildren(icon(kind === 'error' ? 'warning' : kind === 'undo' ? 'close' : 'check', { size: 14 }));
    toastText.textContent = text;
    toast.classList.toggle('is-error', kind === 'error');
    undo.hidden = kind !== 'undo';
    toast.classList.remove('is-on');
    void toast.offsetWidth;
    toast.classList.add('is-on');
    window.clearTimeout(toastTimer);
    toastTimer = window.setTimeout(hideToast, kind === 'undo' ? 6000 : kind === 'error' ? 4200 : 1600);
  };
  const runUndo = (): void => {
    if (undo.hidden || !snapshot?.undo) return;
    window.clearTimeout(toastTimer);
    hideToast();
    cmd({ a: 'undo' });
  };
  undo.addEventListener('click', runUndo);

  // ---- rows ----
  const rows = new Map<number, RowNode>();
  const days = new Map<string, DayNode>();
  const order = (): number[] => (snapshot ? pageOrder(snapshot) : []);
  const rowOf = (target: EventTarget | null): RowNode | undefined => {
    const item = target instanceof HTMLElement ? target.closest<HTMLElement>('.dl-row') : null;
    return item ? rows.get(Number(item.dataset.id)) : undefined;
  };

  const act = (node: RowNode, action: DlAction): void => {
    const entry = node.entry;
    if (!entry || !canAct(entry, action) || !gate.take(`${entry.id}:${action}`)) return;
    setCurrent(entry.id);
    if (action === 'remove') {
      pendingFocus = list.contains(document.activeElement) ? nextAfter(entry.id) ?? 'search' : null;
      removedName = entry.name;
      if (!reduceMotion()) node.root.classList.add('is-leaving');
      seen.delete(entry.id);
    }
    cmd({ a: PAGE_ACTION[action], id: entry.id });
  };
  const nextAfter = (id: number): number | null => {
    const ids = order();
    const at = ids.indexOf(id);
    return at < 0 ? null : ids[at + 1] ?? ids[at - 1] ?? null;
  };
  const onRow = (node: RowNode, action: DlAction | 'open'): void => {
    const entry = node.entry;
    if (!entry) return;
    if (action === 'open') { if (entry.state === 'done') act(node, 'open'); else setCurrent(entry.id); return; }
    act(node, action);
  };

  /** Moves the roving stop to `id` without rebuilding anything. */
  function setCurrent(id: number | null): void {
    const previous = current === null ? undefined : rows.get(current);
    current = id;
    if (previous) for (const part of rowParts(previous)) part.tabIndex = -1;
    const next = id === null ? undefined : rows.get(id);
    if (next) for (const part of rowParts(next)) part.tabIndex = 0;
  }

  const makeDay = (key: string): DayNode => {
    const section = el('section', 'dp-day');
    const heading = el('h2', 'dp-day-h');
    heading.id = `dp-day-${key}`;
    const title = el('span', 'dp-day-title');
    const caption = el('span', 'dp-day-date');
    const tally = el('span', 'dp-day-count');
    heading.append(title, caption, tally);
    const ul = el('ul', 'dp-card');
    ul.setAttribute('aria-labelledby', heading.id);
    section.setAttribute('aria-labelledby', heading.id);
    section.append(heading, ul);
    return { root: section, title, caption, count: tally, list: ul };
  };
  const fillDay = (node: DayNode, day: DlDay, year: number): void => {
    const { title, caption } = dayHeading(day, year);
    if (node.title.textContent !== title) node.title.textContent = title;
    if (node.caption.textContent !== caption) node.caption.textContent = caption;
    node.caption.hidden = caption === '';
    const tally = countText(day.count);
    if (node.count.textContent !== tally) node.count.textContent = tally;
  };

  // ---- empty states ----
  const EMPTY_ICONS: Readonly<Record<EmptyView['kind'], IconName>> = { empty: 'download', nomatch: 'search', guest: 'person', error: 'warning' };
  let emptyKind = '';
  const paintEmpty = (view: EmptyView | null): void => {
    empty.hidden = view === null;
    page.classList.toggle('is-blank', view !== null);
    const key = view ? `${view.kind}\u0000${view.title}` : '';
    if (key === emptyKind) return;
    emptyKind = key;
    if (!view) { empty.replaceChildren(); return; }
    const badge = el('span', 'dp-empty-mark');
    badge.dataset.kind = view.kind;
    badge.append(icon(EMPTY_ICONS[view.kind], { size: 24 }));
    const title = el('h2', 'dp-empty-title');
    title.textContent = view.title;
    const body = el('p', 'dp-empty-body');
    body.textContent = view.body;
    empty.replaceChildren(badge, title, body);
    if (view.kind === 'nomatch') empty.append(textButton('Clear search', () => { clearSearch(); input.focus(); }));
    if (view.kind === 'empty' || view.kind === 'guest') empty.append(textButton('Open downloads folder', () => cmd({ a: 'folder' }), { icon: 'folder-download' }));
    if (view.kind === 'error') empty.append(textButton('Try again', () => cmd({ a: 'refresh' }), { icon: 'reload' }));
  };

  // ---- applying a snapshot ----
  const render = (s: DownloadsSnapshot, animate: boolean): void => {
    const year = new Date().getFullYear();
    const q = s.query;
    const active = document.activeElement;
    const focusedRow = list.contains(active) ? rowOf(active) : undefined;
    const focusedPart = focusedRow ? rowParts(focusedRow).indexOf(active as HTMLButtonElement) : -1;
    const motion = animate && !reduceMotion();
    const flip = motion ? new Map<number, number>() : null;
    if (flip) {
      const view = page.clientHeight;
      for (const [id, node] of rows) {
        const y = node.root.getBoundingClientRect().top;
        if (y > -80 && y < view + 80) flip.set(id, y);
      }
    }
    const used = new Set<number>();
    const usedDays = new Set<string>();
    const sections: HTMLElement[] = [];
    // A download that starts while the page is open arrives with a soft glow; a new search just shows.
    const glow = entered && q === renderedQuery && !reduceMotion();
    renderedQuery = q;
    const announce: string[] = [];
    for (const day of s.days) {
      const key = `${day.date}-${day.label === 'Today' ? 't' : day.label === 'Yesterday' ? 'y' : 'd'}`;
      usedDays.add(key);
      let dayNode = days.get(key);
      if (!dayNode) { dayNode = makeDay(key); days.set(key, dayNode); }
      fillDay(dayNode, day, year);
      const items: HTMLElement[] = [];
      for (const entry of day.entries) {
        used.add(entry.id);
        let node = rows.get(entry.id);
        if (!node) {
          node = makeRow('page', (n, action) => onRow(n, action));
          for (const part of [node.open]) part.tabIndex = -1;
          rows.set(entry.id, node);
          if (glow && !seen.has(entry.id)) node.root.classList.add('is-new');
        }
        node.root.classList.remove('is-leaving');
        seen.add(entry.id);
        if (fillRow(node, entry, 'page', (target, text) => marked(target, text, q), q) && (entry.state === 'done' || entry.state === 'failed')) announce.push(announceText(entry));
        for (const part of rowParts(node)) part.tabIndex = -1;
        items.push(node.root);
      }
      place(dayNode.list, items);
      sections.push(dayNode.root);
    }
    for (const id of [...rows.keys()]) if (!used.has(id)) rows.delete(id);
    for (const key of [...days.keys()]) if (!usedDays.has(key)) days.delete(key);
    place(list, sections);

    // The keyboard stop: the focused row, else the remembered one, else the first.
    const ids = pageOrder(s);
    const wantFocus = pendingFocus;
    pendingFocus = null;
    const stop = focusedRow?.entry && rows.has(focusedRow.entry.id) ? focusedRow.entry.id
      : typeof wantFocus === 'number' && rows.has(wantFocus) ? wantFocus
        : current !== null && rows.has(current) ? current : ids[0] ?? null;
    current = null;
    setCurrent(stop);
    if (typeof wantFocus === 'number' && stop === wantFocus) rows.get(stop)?.open.focus({ preventScroll: false });
    else if (wantFocus === 'search' || (focusedRow?.entry && !rows.has(focusedRow.entry.id))) input.focus();
    else if (focusedRow && stop !== null && focusedPart >= 0) {
      const node = rows.get(stop);
      const parts = node ? rowParts(node) : [];
      const target = parts[Math.min(focusedPart, parts.length - 1)];
      if (target && document.activeElement !== target) target.focus({ preventScroll: true });
    }

    // Header line, live count, empty state, footnote.
    const total = ids.length;
    const searching = q.trim() !== '';
    summary.textContent = summaryText(s, total);
    summary.hidden = summary.textContent === '';
    count.textContent = searching ? summaryText(s, total) : announce.join('. ');
    paintEmpty(emptyView(s));
    search.hidden = s.state === 'error';
    guestNote.hidden = s.state !== 'guest' || total === 0;
    const removableRows = s.days.some((day) => day.entries.some((entry) => removable(entry.state)));
    clearAll.disabled = s.state === 'error' || !removableRows;
    if (clearAll.disabled && confirming) { confirming = false; paintClear(); }
    foot.textContent = s.more ? 'Older downloads aren’t shown here. Search to find them.' : searching || total === 0 ? '' : 'Bowser keeps your 500 most recent downloads.';
    foot.hidden = foot.textContent === '' || s.state === 'error';

    if (flip) {
      for (const [id, before] of flip) {
        const node = rows.get(id);
        if (!node) continue;
        const delta = before - node.root.getBoundingClientRect().top;
        if (Math.abs(delta) < 1) continue;
        node.root.classList.remove('is-gliding');
        node.root.style.setProperty('--dl-from', `${delta}px`);
        void node.root.offsetWidth;
        node.root.classList.add('is-gliding');
      }
    }
  };
  list.addEventListener('animationend', (event) => {
    const item = event.target instanceof HTMLElement ? event.target : null;
    if (!item) return;
    if (event.animationName === 'dl-glide') item.classList.remove('is-gliding');
    if (event.animationName === 'dp-glow') item.classList.remove('is-new');
  });

  // ---- search ----
  const sendSearch = (): void => {
    window.clearTimeout(searchTimer);
    searchTimer = window.setTimeout(() => cmd({ a: 'search', text: query }), SEARCH_DELAY);
  };
  const paintSearch = (): void => {
    clear.hidden = query === '';
    search.classList.toggle('has-text', query !== '');
  };
  function clearSearch(): void {
    if (input.value === '' && query === '') return;
    input.value = '';
    query = '';
    paintSearch();
    window.clearTimeout(searchTimer);
    cmd({ a: 'search', text: '' });
  }
  input.addEventListener('input', () => {
    query = input.value.slice(0, MAX_QUERY);
    paintSearch();
    if (!composing) sendSearch();
  });
  input.addEventListener('compositionstart', () => { composing = true; });
  input.addEventListener('compositionend', () => { composing = false; query = input.value.slice(0, MAX_QUERY); sendSearch(); });
  input.addEventListener('keydown', (event) => {
    if (event.isComposing || event.keyCode === 229) return;
    if (event.key === 'Escape' && input.value !== '') { event.preventDefault(); event.stopPropagation(); clearSearch(); return; }
    if (event.key === 'ArrowDown' || event.key === 'Enter') {
      const ids = order();
      const target = rows.get(current !== null && ids.includes(current) ? current : ids[0] ?? -1);
      if (!target) return;
      event.preventDefault();
      target.open.focus();
      target.root.scrollIntoView({ block: 'nearest' });
    }
  });
  clear.addEventListener('click', () => { clearSearch(); input.focus(); });

  // ---- keyboard in the list ----
  list.addEventListener('focusin', (event) => {
    const node = rowOf(event.target);
    if (node?.entry && node.entry.id !== current) setCurrent(node.entry.id);
  });
  list.addEventListener('keydown', (event) => {
    if (event.isComposing || event.altKey) return;
    const node = rowOf(event.target);
    if (!node?.entry) return;
    const ids = order();
    const index = ids.indexOf(node.entry.id);
    const pageSize = Math.max(1, Math.floor(page.clientHeight / Math.max(1, node.root.offsetHeight || 64)) - 1);
    if (!event.ctrlKey && !event.metaKey) {
      const next = moveIndex(ids.length, index, event.key, pageSize);
      if (next !== null) {
        event.preventDefault();
        if (event.key === 'ArrowUp' && index === 0) { input.focus(); input.select(); return; }
        const target = rows.get(ids[next] ?? -1);
        target?.open.focus();
        target?.root.scrollIntoView({ block: 'nearest' });
        return;
      }
    }
    if (event.key === 'Enter' && event.target === node.open) { event.preventDefault(); if (node.entry.state === 'done') act(node, 'open'); return; }
    if (event.key === 'Delete' && !event.ctrlKey) { event.preventDefault(); if (removable(node.entry.state)) act(node, 'remove'); return; }
    // Left and Right step between the row and its buttons.
    const parts = rowParts(node);
    const at = parts.indexOf(event.target as HTMLButtonElement);
    if (at >= 0 && (event.key === 'ArrowRight' || event.key === 'ArrowLeft')) {
      event.preventDefault();
      parts[Math.max(0, Math.min(parts.length - 1, at + (event.key === 'ArrowRight' ? 1 : -1)))]?.focus();
    }
  });

  // `/` and Ctrl+F reach the search, Escape clears it (or backs out of Clear all), Ctrl+Z undoes a removal.
  document.addEventListener('keydown', (event) => {
    if (event.isComposing) return;
    const typing = event.target instanceof HTMLInputElement;
    if ((event.key === 'f' && event.ctrlKey && !event.shiftKey && !event.altKey) || (event.key === '/' && !typing && !event.ctrlKey && !event.altKey)) {
      if (search.hidden) return;
      event.preventDefault();
      input.focus();
      input.select();
      return;
    }
    if (event.key === 'Escape' && confirming) { event.preventDefault(); confirming = false; paintClear(); clearAll.focus(); return; }
    if (event.key === 'Escape' && !typing && query !== '') { event.preventDefault(); clearSearch(); input.focus(); return; }
    if (event.key === 'z' && event.ctrlKey && !event.shiftKey && !typing && !undo.hidden) { event.preventDefault(); runUndo(); }
  });

  // The header gets its hairline once the list scrolls under it.
  let scrollFrame = 0;
  page.addEventListener('scroll', () => {
    if (scrollFrame) return;
    scrollFrame = requestAnimationFrame(() => {
      scrollFrame = 0;
      head.classList.toggle('is-scrolled', page.scrollTop > 2);
    });
  }, { passive: true });

  // A fresh snapshot when the page comes back to the front (a file may have been moved or deleted).
  let lastRefresh = 0;
  window.addEventListener('focus', () => {
    const now = Date.now();
    if (snapshot && now - lastRefresh > 1500) { lastRefresh = now; cmd({ a: 'refresh' }); }
  });

  // ---- Bowser's messages ----
  const receive = (message: Inbound): void => {
    if (message.t === 'state') { applyTheme(message.theme, message.solid, message.dark, undefined, undefined, message.density, message.compactMenus); return; }
    if (message.t !== 'snap' || message.p !== 'downloadspage') return;
    const parsed = parseDownloadsSnapshot(message.d);
    if (!parsed) return;
    gate.clear();
    const animate = parsed.ack === 'removed' || parsed.ack === 'restored' || parsed.ack === 'cleared' || (snapshot !== null && pageOrder(snapshot).join() !== pageOrder(parsed).join());
    snapshot = parsed;
    render(parsed, animate && entered);
    if (parsed.ack === 'removed') showToast(removedName !== '' ? `Removed “${removedName}” from the list` : 'Removed from the list', parsed.undo ? 'undo' : 'ok');
    else if (parsed.ack === 'restored') showToast(removedName !== '' ? `Restored “${removedName}”` : 'Restored', 'ok');
    else if (parsed.ack === 'cleared') showToast('Cleared the download list', parsed.undo ? 'undo' : 'ok');
    else if (parsed.ack === 'failed') showToast('The download list could not be changed.', 'error');
    if (parsed.ack === 'failed') for (const node of rows.values()) node.root.classList.remove('is-leaving');
    if (!entered) {
      entered = true;
      column.classList.add('bw-enter', 'is-rise');
      head.classList.add('is-entered');
      mark.classList.add('is-drop');
    }
  };
  onInbound(receive);
  send({ v: 1, t: 'ready' });
}
