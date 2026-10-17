// The history page (?view=history), drawn by Bowser over a `bowser://history` tab
// (docs/design/history-page.md, styles/history.css). Rust sends `state` (theme) and `snap`
// (p "historypage"); the page sends `{t:"hpage",a,...}` commands only, and every one is checked
// again in Rust (chrome_host/history_page.rs). Text from Rust reaches the DOM through textContent
// only. The page never changes its own URL (the bridge checks it exactly) and has no links: rows
// are buttons, and Bowser opens the address.
import { onInbound, send, sendPanel, type Inbound, type PanelFields } from './protocol';
import { applyTheme } from './view';
import { el, icon, place, type IconName } from './dom';
import { textButton } from './panels/kit';
import { tileLetter } from './panels/kit-logic';
import {
  MAX_QUERY, addressParts, dayHeading, emptyView, focusAfterRemoval, matchRanges, moveIndex, opensNewTab,
  pagesText, parseHistorySnapshot, resultText, rowName, tileTone,
  type EmptyView, type HistoryDay, type HistoryEntry, type HistorySnapshot,
} from './history-page-logic';

const reduceMotion = (): boolean => matchMedia('(prefers-reduced-motion: reduce)').matches;
/** How long the search waits for the next keystroke before asking Bowser. */
const SEARCH_DELAY = 90;

function cmd(fields: PanelFields): boolean {
  return sendPanel('hpage', fields);
}

/** Writes `text` into `target`, wrapping the search hits in `<mark>`; untouched when unchanged. */
function marked(target: HTMLElement, text: string, query: string): void {
  const ranges = matchRanges(text, query);
  const key = `${query}\u0000${text}`;
  if (target.dataset.key === key) return;
  target.dataset.key = key;
  if (ranges.length === 0) { target.textContent = text; return; }
  const parts: Node[] = [];
  let at = 0;
  for (const [start, end] of ranges) {
    if (start > at) parts.push(document.createTextNode(text.slice(at, start)));
    const hit = el('mark', 'hp-hit');
    hit.textContent = text.slice(start, end);
    parts.push(hit);
    at = end;
  }
  if (at < text.length) parts.push(document.createTextNode(text.slice(at)));
  target.replaceChildren(...parts);
}

interface RowNode {
  readonly root: HTMLLIElement;
  readonly open: HTMLButtonElement;
  readonly lead: HTMLElement;
  readonly title: HTMLElement;
  readonly host: HTMLElement;
  readonly rest: HTMLElement;
  readonly time: HTMLElement;
  readonly newTab: HTMLButtonElement;
  readonly remove: HTMLButtonElement;
  entry: HistoryEntry;
  fav: string;
}

interface DayNode {
  readonly root: HTMLElement;
  readonly title: HTMLElement;
  readonly caption: HTMLElement;
  readonly count: HTMLElement;
  readonly list: HTMLUListElement;
}

export function startHistoryPage(root: HTMLElement): void {
  root.classList.add('hp-root');
  let snapshot: HistorySnapshot | null = null;
  let entered = false;
  /** The search as typed; Bowser answers for `snapshot.query`, which may lag a keystroke behind. */
  let query = '';
  let composing = false;
  let searchTimer = 0;
  /** The row that holds the keyboard stop (roving tabindex), by entry id. */
  let current: number | null = null;
  /** Where focus goes once the next snapshot lands (after a removal). */
  let pendingFocus: number | null | 'search' = null;
  let removedName = '';
  /** Entry ids already shown, and the search they were shown for (see `glow` in render). */
  const seen = new Set<number>();
  let renderedQuery = '';

  // ---- skeleton ----
  const page = el('div', 'hp bw-scroll');
  const head = el('header', 'hp-head');
  const headIn = el('div', 'hp-head-in');
  const top = el('div', 'hp-top');
  const brand = el('div', 'hp-brand');
  const mark = el('span', 'hp-mark');
  mark.append(icon('clock', { size: 20 }));
  const titles = el('div', 'hp-titles');
  const h1 = el('h1', 'hp-title');
  h1.textContent = 'History';
  const summary = el('p', 'hp-summary');
  titles.append(h1, summary);
  brand.append(mark, titles);
  const clearData = textButton('Clear browsing data', () => cmd({ a: 'privacy' }), { icon: 'shield' });
  clearData.classList.add('hp-clear-data');
  clearData.setAttribute('aria-label', 'Clear browsing data');
  top.append(brand, clearData);

  const search = el('div', 'hp-search');
  search.setAttribute('role', 'search');
  const input = el('input', 'hp-search-input');
  input.type = 'search';
  input.placeholder = 'Search history';
  input.autocomplete = 'off';
  input.spellcheck = false;
  input.maxLength = MAX_QUERY;
  input.setAttribute('aria-label', 'Search history');
  input.setAttribute('aria-controls', 'hp-list');
  input.setAttribute('aria-describedby', 'hp-count');
  const clear = el('button', 'bw-icon-btn hp-search-clear');
  clear.type = 'button';
  clear.hidden = true;
  clear.setAttribute('aria-label', 'Clear search');
  clear.append(icon('close', { size: 12, fine: true }));
  const keycap = el('kbd', 'hp-key');
  keycap.textContent = '/';
  keycap.setAttribute('aria-hidden', 'true');
  search.append(icon('search', { className: 'hp-search-icon' }), input, keycap, clear);
  headIn.append(top, search);
  head.append(headIn);

  const main = el('main', 'hp-main');
  main.setAttribute('aria-label', 'History');
  const column = el('div', 'hp-col');
  const list = el('div', 'hp-list');
  list.id = 'hp-list';
  const empty = el('div', 'hp-empty');
  empty.hidden = true;
  const foot = el('p', 'hp-foot');
  foot.hidden = true;
  column.append(list, empty, foot);
  main.append(column);

  const count = el('p', 'hp-sr');
  count.id = 'hp-count';
  count.setAttribute('role', 'status');
  count.setAttribute('aria-live', 'polite');
  const toast = el('div', 'hp-toast');
  toast.setAttribute('role', 'status');
  toast.setAttribute('aria-live', 'polite');
  const toastIcon = el('span', 'hp-toast-icon');
  const toastText = el('span', 'hp-toast-text');
  const undo = el('button', 'hp-toast-undo');
  undo.type = 'button';
  undo.textContent = 'Undo';
  undo.setAttribute('aria-keyshortcuts', 'Control+Z');
  toast.append(toastIcon, toastText, undo);
  page.append(head, main, count, toast);
  root.replaceChildren(page);

  // ---- toast: "Removed", with Undo ----
  let toastTimer = 0;
  const hideToast = (): void => { toast.classList.remove('is-on'); undo.hidden = true; };
  const showToast = (text: string, kind: 'ok' | 'undo' | 'error'): void => {
    toastIcon.replaceChildren(icon(kind === 'error' ? 'warning' : kind === 'undo' ? 'trash' : 'check', { size: 14 }));
    toastText.textContent = text;
    toast.classList.toggle('is-error', kind === 'error');
    undo.hidden = kind !== 'undo';
    toast.classList.remove('is-on');
    void toast.offsetWidth; // restart the entrance when a second answer lands during the first toast
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
  const order = (): number[] => (snapshot?.days ?? []).flatMap((day) => day.entries.map((entry) => entry.id));
  const rowOf = (target: EventTarget | null): RowNode | undefined => {
    const item = target instanceof HTMLElement ? target.closest<HTMLElement>('.hp-row') : null;
    return item ? rows.get(Number(item.dataset.id)) : undefined;
  };

  const openEntry = (id: number, newTab: boolean): void => { cmd({ a: newTab ? 'newtab' : 'open', id }); };
  const removeEntry = (id: number): void => {
    const node = rows.get(id);
    if (!node) return;
    const inList = list.contains(document.activeElement);
    pendingFocus = inList ? focusAfterRemoval(order(), id) ?? 'search' : null;
    removedName = rowName(node.entry);
    if (!reduceMotion()) node.root.classList.add('is-leaving');
    // Put back by Undo, it comes back with the glow of a new row.
    seen.delete(id);
    cmd({ a: 'remove', id });
  };

  /** Moves the roving stop to `id` without rebuilding anything. */
  const setCurrent = (id: number | null): void => {
    const previous = current === null ? undefined : rows.get(current);
    current = id;
    for (const node of previous ? [previous] : []) {
      node.open.tabIndex = -1; node.newTab.tabIndex = -1; node.remove.tabIndex = -1;
    }
    const next = id === null ? undefined : rows.get(id);
    if (next) { next.open.tabIndex = 0; next.newTab.tabIndex = 0; next.remove.tabIndex = 0; }
  };

  const makeRow = (entry: HistoryEntry): RowNode => {
    const item = el('li', 'hp-row');
    item.dataset.id = String(entry.id);
    const open = el('button', 'hp-open');
    open.type = 'button';
    open.tabIndex = -1;
    const lead = el('span', 'hp-lead');
    const text = el('span', 'hp-text');
    const title = el('span', 'hp-name');
    const addr = el('span', 'hp-addr');
    const host = el('span', 'hp-host');
    const rest = el('span', 'hp-rest');
    addr.append(host, rest);
    text.append(title, addr);
    const time = el('span', 'hp-time');
    open.append(lead, text, time);
    const acts = el('span', 'hp-acts');
    const newTab = el('button', 'bw-icon-btn is-lg hp-act');
    newTab.type = 'button';
    newTab.tabIndex = -1;
    newTab.append(icon('external'));
    const remove = el('button', 'bw-icon-btn is-lg hp-act is-remove');
    remove.type = 'button';
    remove.tabIndex = -1;
    remove.setAttribute('aria-keyshortcuts', 'Delete');
    remove.append(icon('trash'));
    acts.append(newTab, remove);
    item.append(open, acts);
    const node: RowNode = { root: item, open, lead, title, host, rest, time, newTab, remove, entry, fav: '\u0000' };
    const id = (): number => node.entry.id;
    open.addEventListener('click', (event) => { setCurrent(id()); openEntry(id(), opensNewTab(event)); });
    // A middle click opens a new tab (and never starts auto-scroll).
    open.addEventListener('mousedown', (event) => { if (event.button === 1) event.preventDefault(); });
    open.addEventListener('auxclick', (event) => { if (event.button === 1) { event.preventDefault(); setCurrent(id()); openEntry(id(), true); } });
    newTab.addEventListener('click', (event) => { event.stopPropagation(); setCurrent(id()); openEntry(id(), true); });
    remove.addEventListener('click', (event) => { event.stopPropagation(); removeEntry(id()); });
    // The glow (on ::after) outlasts the drop-in; the row is ordinary again once it has faded.
    item.addEventListener('animationend', (event) => { if (event.target === item && event.animationName === 'hp-glow') item.classList.remove('is-new'); });
    return node;
  };

  const fillRow = (node: RowNode, entry: HistoryEntry, favs: Readonly<Record<string, string>>, q: string): void => {
    node.entry = entry;
    const name = rowName(entry);
    marked(node.title, name, q);
    const parts = addressParts(entry.url, entry.host);
    marked(node.host, parts.host, q);
    marked(node.rest, parts.rest, q);
    node.rest.hidden = parts.rest === '';
    if (node.time.textContent !== entry.clock) node.time.textContent = entry.clock;
    node.newTab.setAttribute('aria-label', `Open ${name} in a new tab`);
    node.remove.setAttribute('aria-label', `Remove ${name} from history`);
    const uri = favs[entry.host] ?? '';
    if (node.fav !== uri) {
      node.fav = uri;
      if (uri !== '') {
        const img = el('img', 'hp-fav is-image');
        img.alt = '';
        img.draggable = false;
        img.src = uri;
        node.lead.replaceChildren(img);
        node.lead.removeAttribute('data-tone');
      } else {
        const tile = el('span', 'hp-fav');
        tile.textContent = tileLetter(entry.host || name);
        node.lead.replaceChildren(tile);
        node.lead.dataset.tone = String(tileTone(entry.host || name));
      }
      node.lead.setAttribute('aria-hidden', 'true');
    }
  };

  const makeDay = (key: string): DayNode => {
    const section = el('section', 'hp-day');
    const heading = el('h2', 'hp-day-h');
    heading.id = `hp-day-${key}`;
    const title = el('span', 'hp-day-title');
    const caption = el('span', 'hp-day-date');
    const tally = el('span', 'hp-day-count');
    heading.append(title, caption, tally);
    const ul = el('ul', 'hp-card');
    ul.setAttribute('aria-labelledby', heading.id);
    section.setAttribute('aria-labelledby', heading.id);
    section.append(heading, ul);
    return { root: section, title, caption, count: tally, list: ul };
  };

  const fillDay = (node: DayNode, day: HistoryDay, year: number): void => {
    const { title, caption } = dayHeading(day, year);
    if (node.title.textContent !== title) node.title.textContent = title;
    if (node.caption.textContent !== caption) node.caption.textContent = caption;
    node.caption.hidden = caption === '';
    const tally = pagesText(day.count);
    if (node.count.textContent !== tally) node.count.textContent = tally;
  };

  // ---- empty states ----
  const EMPTY_ICONS: Readonly<Record<EmptyView['kind'], IconName>> = { empty: 'clock', nomatch: 'search', off: 'clock', guest: 'person', error: 'warning' };
  let emptyKind = '';
  const paintEmpty = (view: EmptyView | null): void => {
    empty.hidden = view === null;
    page.classList.toggle('is-blank', view !== null);
    const key = view ? `${view.kind}\u0000${view.title}` : '';
    if (key === emptyKind) return;
    emptyKind = key;
    if (!view) { empty.replaceChildren(); return; }
    const badge = el('span', 'hp-empty-mark');
    badge.dataset.kind = view.kind;
    badge.append(icon(EMPTY_ICONS[view.kind], { size: 24 }));
    const title = el('h2', 'hp-empty-title');
    title.textContent = view.title;
    const body = el('p', 'hp-empty-body');
    body.textContent = view.body;
    empty.replaceChildren(badge, title, body);
    if (view.kind === 'nomatch') empty.append(textButton('Clear search', () => { clearSearch(); input.focus(); }));
    if (view.kind === 'off') empty.append(textButton('Open privacy settings', () => cmd({ a: 'settings' }), { icon: 'gear', primary: true }));
    if (view.kind === 'error') empty.append(textButton('Try again', () => cmd({ a: 'refresh' }), { icon: 'reload' }));
  };

  // ---- applying a snapshot ----
  const render = (s: HistorySnapshot, animate: boolean): void => {
    const year = new Date().getFullYear();
    const q = s.query;
    const active = document.activeElement;
    const focusedRow = list.contains(active) ? rowOf(active) : undefined;
    const focusedPart = focusedRow ? (active === focusedRow.newTab ? 'newTab' : active === focusedRow.remove ? 'remove' : 'open') : null;
    // FLIP: where the rows on screen are now, so the ones that move can glide to their new place.
    const flip = animate && !reduceMotion() ? new Map<number, number>() : null;
    if (flip) {
      const view = page.clientHeight;
      for (const [id, node] of rows) {
        const top = node.root.getBoundingClientRect().top;
        if (top > -80 && top < view + 80) flip.set(id, top);
      }
    }
    const used = new Set<number>();
    const usedDays = new Set<string>();
    const sections: HTMLElement[] = [];
    // Only rows that arrive under the same search are news; a new search simply shows its results.
    const glow = entered && q === renderedQuery && !reduceMotion();
    renderedQuery = q;
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
          node = makeRow(entry);
          rows.set(entry.id, node);
          // A visit recorded while the page is open (or one put back) arrives with a soft glow.
          if (glow && !seen.has(entry.id)) node.root.classList.add('is-new');
        }
        node.root.classList.remove('is-leaving');
        seen.add(entry.id);
        fillRow(node, entry, s.favs, q);
        items.push(node.root);
      }
      place(dayNode.list, items);
      sections.push(dayNode.root);
    }
    for (const id of [...rows.keys()]) if (!used.has(id)) rows.delete(id);
    for (const key of [...days.keys()]) if (!usedDays.has(key)) days.delete(key);
    place(list, sections);

    // The keyboard stop: the focused row, else the remembered one, else the first.
    const ids = s.days.flatMap((day) => day.entries.map((entry) => entry.id));
    const wantFocus = pendingFocus;
    pendingFocus = null;
    const stop = focusedRow && rows.has(focusedRow.entry.id) ? focusedRow.entry.id : typeof wantFocus === 'number' && rows.has(wantFocus) ? wantFocus : current !== null && rows.has(current) ? current : ids[0] ?? null;
    current = null;
    for (const node of rows.values()) { node.open.tabIndex = -1; node.newTab.tabIndex = -1; node.remove.tabIndex = -1; }
    setCurrent(stop);
    if (typeof wantFocus === 'number' && stop === wantFocus) rows.get(stop)?.open.focus({ preventScroll: false });
    else if (wantFocus === 'search' || (focusedRow && !rows.has(focusedRow.entry.id))) input.focus();
    else if (focusedRow && focusedPart && stop !== null) {
      const node = rows.get(stop);
      const target = node ? node[focusedPart] : undefined;
      if (target && document.activeElement !== target) target.focus({ preventScroll: true });
    }

    // Header line, live count, empty state, footnote.
    const total = ids.length;
    const searching = q.trim() !== '';
    summary.textContent = s.state !== 'on' ? '' : searching ? resultText(total, q) : total === 0 ? '' : `${pagesText(total)}${s.more ? ' shown' : ''}`;
    summary.hidden = summary.textContent === '';
    count.textContent = searching ? resultText(total, q) : '';
    paintEmpty(emptyView(s));
    search.hidden = s.state !== 'on';
    clearData.hidden = s.state === 'guest';
    foot.hidden = s.state !== 'on' || total === 0;
    foot.textContent = s.more ? 'Older pages aren’t shown here. Search to find them.' : searching ? '' : 'History keeps your 500 most recent pages.';
    if (foot.textContent === '') foot.hidden = true;

    if (flip) {
      for (const [id, before] of flip) {
        const node = rows.get(id);
        if (!node) continue;
        const delta = before - node.root.getBoundingClientRect().top;
        if (Math.abs(delta) < 1) continue;
        node.root.classList.remove('is-gliding');
        node.root.style.setProperty('--hp-from', `${delta}px`);
        void node.root.offsetWidth;
        node.root.classList.add('is-gliding');
      }
    }
  };
  list.addEventListener('animationend', (event) => {
    const item = event.target instanceof HTMLElement ? event.target : null;
    if (event.animationName === 'hp-glide' && item?.classList.contains('is-gliding')) item.classList.remove('is-gliding');
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
    if (node && node.entry.id !== current) setCurrent(node.entry.id);
  });
  list.addEventListener('keydown', (event) => {
    if (event.isComposing || event.altKey) return;
    const node = rowOf(event.target);
    if (!node) return;
    const ids = order();
    const index = ids.indexOf(node.entry.id);
    const pageSize = Math.max(1, Math.floor(page.clientHeight / Math.max(1, node.root.offsetHeight || 52)) - 1);
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
    if (event.key === 'Enter' && event.target === node.open) { event.preventDefault(); openEntry(node.entry.id, opensNewTab(event)); return; }
    if (event.key === 'Delete' && !event.ctrlKey) { event.preventDefault(); removeEntry(node.entry.id); return; }
    // Left and Right step between the row and its two buttons.
    const parts = [node.open, node.newTab, node.remove];
    const at = parts.indexOf(event.target as HTMLButtonElement);
    if (at >= 0 && (event.key === 'ArrowRight' || event.key === 'ArrowLeft')) {
      event.preventDefault();
      parts[Math.max(0, Math.min(parts.length - 1, at + (event.key === 'ArrowRight' ? 1 : -1)))]?.focus();
    }
  });

  // `/` and Ctrl+F reach the search, Escape clears it, Ctrl+Z undoes a removal.
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

  // A fresh snapshot when the page comes back to the front (another window may have visited or
  // cleared something; the privacy dashboard may have cleared history).
  let lastRefresh = 0;
  window.addEventListener('focus', () => {
    const now = Date.now();
    if (snapshot && now - lastRefresh > 1500) { lastRefresh = now; cmd({ a: 'refresh' }); }
  });

  // ---- Bowser's messages ----
  const receive = (message: Inbound): void => {
    if (message.t === 'state') { applyTheme(message.theme, message.solid, message.dark, message.fontScale, message.uiFont, message.density, message.compactMenus, message.look); return; }
    if (message.t !== 'snap' || message.p !== 'historypage') return;
    const parsed = parseHistorySnapshot(message.d);
    if (!parsed) return;
    const animate = parsed.ack === 'removed' || parsed.ack === 'restored';
    snapshot = parsed;
    render(parsed, animate && entered);
    if (parsed.ack === 'removed') showToast(removedName !== '' ? `Removed “${removedName}”` : 'Removed from history', parsed.undo ? 'undo' : 'ok');
    else if (parsed.ack === 'restored') showToast(removedName !== '' ? `Restored “${removedName}”` : 'Restored', 'ok');
    else if (parsed.ack === 'failed') showToast('History could not be changed.', 'error');
    if (parsed.ack === 'failed') for (const node of rows.values()) node.root.classList.remove('is-leaving');
    if (!entered) {
      entered = true;
      column.classList.add('bw-enter', 'is-rise');
      head.classList.add('is-entered');
      mark.classList.add('is-turn');
    }
  };
  onInbound(receive);
  send({ v: 1, t: 'ready' });
}
