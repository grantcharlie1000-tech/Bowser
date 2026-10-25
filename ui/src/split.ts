// Split view host (?view=split): the empty side's "choose a tab" surface, the drop-zone card shown
// while a tab is dragged over the page, and the private landing over a blank private tab
// (private-page.ts). Rust owns every model (see src/win32/split_ui.rs); this page renders and
// reports intent. Page-controlled text is written with textContent only.
import { onInbound, send, type SplitCommand, type SplitDropMsg, type SplitPickMsg } from './protocol';
import { validSplitCommand, type PickClosed, type PickTab, type SplitAt } from './split-protocol';
import { dropCard, filterPick, moveSelection, pickHeading } from './split-logic';
import { applyTheme } from './view';
import { el, icon } from './dom';
import { installTooltips } from './tooltip';
import { buildPrivatePage } from './private-page';
// --- start page ---
// The start page is its own bundle (dist/start-page.js), fetched the first time a blank tab shows it, so
// no other view and no other tab pays for it. Types only here.
import type { buildStartPage } from './start-page';
import { parseStartMessage, type StartMessage } from './start-page-logic';
// --- end start page ---

function sendSplit(cmd: SplitCommand): boolean {
  return validSplitCommand(cmd) && send(cmd);
}

type Row = { readonly kind: 'tab'; readonly tab: PickTab } | { readonly kind: 'closed'; readonly page: PickClosed };

function face(fav: string, priv: boolean): HTMLElement {
  const box = el('span', 'sv-face');
  box.setAttribute('aria-hidden', 'true');
  if (fav !== '') {
    const img = el('img', 'sv-fav');
    img.alt = '';
    img.draggable = false;
    img.src = fav;
    box.append(img);
  } else {
    box.append(icon(priv ? 'private' : 'workspace-globe'));
  }
  return box;
}

/** Builds the picker into `root` and returns its message handlers. */
function buildPicker(root: HTMLElement): { show(msg: SplitPickMsg, first: boolean): void } {
  const column = el('div', 'sv-column');
  const head = el('div', 'sv-head');
  const title = el('h1', 'sv-title');
  title.id = 'sv-title';
  const close = el('button', 'bw-icon-btn is-lg sv-close');
  close.type = 'button';
  close.setAttribute('aria-label', 'Close this side');
  close.dataset.tip = 'Close this side';
  close.append(icon('close'));
  head.append(title, close);

  const search = el('label', 'sv-search');
  search.append(icon('search', { className: 'sv-search-icon' }));
  const input = el('input', 'bw-input sv-input');
  input.type = 'text';
  input.spellcheck = false;
  input.autocomplete = 'off';
  input.placeholder = 'Search tabs or enter an address';
  input.setAttribute('role', 'combobox');
  input.setAttribute('aria-autocomplete', 'list');
  input.setAttribute('aria-controls', 'sv-list');
  input.setAttribute('aria-expanded', 'true');
  input.setAttribute('aria-label', 'Search tabs or enter an address');
  search.append(input);

  const list = el('div', 'sv-list');
  list.id = 'sv-list';
  list.setAttribute('role', 'listbox');
  list.setAttribute('aria-labelledby', 'sv-title');
  const empty = el('p', 'sv-empty');
  const hint = el('p', 'sv-hint');
  hint.textContent = 'Enter opens the highlighted tab, or goes to the address you typed.';
  column.append(head, search, list, empty, hint);

  let tabs: readonly PickTab[] = [];
  let closed: readonly PickClosed[] = [];
  let rows: Row[] = [];
  let sel = -1;
  let typed = false;

  const pick = (row: Row): void => {
    if (row.kind === 'tab') sendSplit({ v: 1, t: 'split', a: 'pick', id: row.tab.id });
    else sendSplit({ v: 1, t: 'split', a: 'reopen', n: row.page.n });
  };

  const highlight = (index: number): void => {
    sel = index;
    const options = list.querySelectorAll<HTMLElement>('[role="option"]');
    options.forEach((option, i) => option.setAttribute('aria-selected', String(i === sel)));
    const current = options[sel];
    if (current) {
      input.setAttribute('aria-activedescendant', current.id);
      current.scrollIntoView({ block: 'nearest' });
    } else {
      input.removeAttribute('aria-activedescendant');
    }
  };

  const section = (label: string): HTMLElement => {
    const header = el('div', 'bw-section-header sv-section');
    header.setAttribute('role', 'presentation');
    header.textContent = label;
    return header;
  };

  const option = (row: Row, index: number): HTMLElement => {
    const node = el('div', 'bw-row sv-row');
    node.id = `sv-row-${index}`;
    node.setAttribute('role', 'option');
    node.setAttribute('aria-selected', 'false');
    const text = el('span', 'bw-row-text');
    const name = el('span', 'bw-row-title');
    name.dir = 'auto';
    const meta = el('span', 'bw-row-meta');
    meta.dir = 'auto';
    if (row.kind === 'tab') {
      name.textContent = row.tab.title;
      meta.textContent = row.tab.private ? `${row.tab.host} · Private` : row.tab.host;
      node.append(face(row.tab.fav, row.tab.private), text);
    } else {
      name.textContent = row.page.title;
      meta.textContent = row.page.host;
      const clock = el('span', 'sv-face');
      clock.setAttribute('aria-hidden', 'true');
      clock.append(icon('clock'));
      node.append(clock, text);
    }
    text.append(name, meta);
    node.addEventListener('pointerenter', () => highlight(index));
    node.addEventListener('click', () => pick(row));
    return node;
  };

  const render = (): void => {
    const shown = filterPick(tabs, closed, input.value);
    rows = [
      ...shown.tabs.map((tab): Row => ({ kind: 'tab', tab })),
      ...shown.closed.map((page): Row => ({ kind: 'closed', page })),
    ];
    const nodes: HTMLElement[] = [];
    if (shown.tabs.length > 0) nodes.push(section('Open tabs'));
    let index = 0;
    for (const tab of shown.tabs) nodes.push(option({ kind: 'tab', tab }, index++));
    if (shown.closed.length > 0) nodes.push(section('Recently closed'));
    for (const page of shown.closed) nodes.push(option({ kind: 'closed', page }, index++));
    list.replaceChildren(...nodes);
    const query = input.value.trim();
    empty.hidden = rows.length > 0;
    empty.textContent = query === '' ? 'No other tabs in this workspace. Type an address to open one here.' : 'No tab matches. Press Enter to go to the address.';
    // A deliberately cleared highlight (Up from the first row while typing) survives host updates.
    highlight(rows.length === 0 || (typed && sel < 0) ? -1 : Math.min(Math.max(sel, 0), rows.length - 1));
  };

  input.addEventListener('input', () => {
    typed = true;
    sel = 0;
    render();
  });
  input.addEventListener('keydown', (ev) => {
    if (ev.isComposing) return;
    if (ev.key === 'ArrowUp' && sel === 0 && input.value.trim() !== '') {
      // Up from the first match leaves the typed address as the Enter target.
      ev.preventDefault();
      highlight(-1);
    } else if (ev.key === 'ArrowDown' || ev.key === 'ArrowUp' || ((ev.key === 'Home' || ev.key === 'End') && ev.ctrlKey)) {
      ev.preventDefault();
      highlight(moveSelection(rows.length, sel, ev.key as 'ArrowUp' | 'ArrowDown' | 'Home' | 'End'));
    } else if (ev.key === 'Enter') {
      ev.preventDefault();
      const row = rows[sel];
      if (row) pick(row);
      else if (input.value.trim() !== '') sendSplit({ v: 1, t: 'split', a: 'go', text: input.value.slice(0, 2048) });
    } else if (ev.key === 'Escape') {
      ev.preventDefault();
      if (input.value !== '') {
        input.value = '';
        typed = false;
        sel = 0;
        render();
      } else {
        sendSplit({ v: 1, t: 'split', a: 'close' });
      }
    }
  });
  close.addEventListener('click', () => sendSplit({ v: 1, t: 'split', a: 'close' }));

  return {
    show(msg: SplitPickMsg, first: boolean): void {
      tabs = msg.tabs;
      closed = msg.closed;
      title.textContent = pickHeading(msg.side);
      root.setAttribute('aria-label', pickHeading(msg.side));
      if (first) {
        input.value = '';
        typed = false;
        sel = 0;
        column.classList.remove('bw-enter', 'is-scale');
        void column.offsetWidth;
        column.classList.add('bw-enter', 'is-scale');
        root.replaceChildren(column);
      }
      render();
      if (document.hasFocus() && document.activeElement !== input) input.focus({ preventScroll: true });
    },
  };
}

const EDGE_CLASS = { l: 'is-left', r: 'is-right', t: 'is-top', b: 'is-bottom', tl: 'is-top-left', tr: 'is-top-right', bl: 'is-bottom-left', br: 'is-bottom-right' } as const;

/**
 * The drop-zone card. Rust sizes the host to exactly the pane the dragged tab will take: a half
 * (left, right, top or bottom) or a quarter (`tl` ... `br`).
 */
function buildDrop(msg: SplitDropMsg): HTMLElement {
  const words = dropCard(msg.side);
  const zone = el('div', `sv-drop bw-enter is-scale ${EDGE_CLASS[msg.side]}`);
  zone.dataset.side = msg.side;
  const card = el('div', 'sv-drop-card');
  const badge = el('span', 'sv-drop-icon');
  badge.setAttribute('aria-hidden', 'true');
  badge.append(icon(words.glyph, { size: 24 }));
  const label = el('strong', 'sv-drop-label');
  label.textContent = words.label;
  const name = el('span', 'sv-drop-title');
  name.dir = 'auto';
  name.textContent = msg.title;
  card.append(badge, label, name);
  zone.append(card);
  zone.setAttribute('role', 'status');
  zone.setAttribute('aria-label', `${label.textContent}: ${msg.title}`);
  return zone;
}

// --- start page ---
type StartPageHandle = ReturnType<typeof buildStartPage>;

/** Imports the start page bundle (a variable specifier, so esbuild leaves the import() to the browser). */
function loadStartPage(): Promise<{ buildStartPage: typeof buildStartPage }> {
  const file = './start-page.js' as string;
  return (import(file) as Promise<{ buildStartPage: typeof buildStartPage }>).catch((err: unknown) => {
    document.title = 'page-load-failed';
    throw err;
  });
}
// --- end start page ---

export function startSplit(root: HTMLElement): void {
  root.classList.add('split-view');
  root.setAttribute('role', 'region');
  installTooltips(root);
  const picker = buildPicker(root);
  const privatePage = buildPrivatePage();
  let mode: 'none' | 'pick' | 'drop' | 'private' | 'start' = 'none';
  // --- start page ---
  let startPage: StartPageHandle | null = null;
  let startLoading = false;
  let startQueue: StartMessage[] = [];
  const leaveStart = (): void => {
    if (mode === 'start') startPage?.hidden();
    root.classList.remove('is-start');
  };
  const showStart = (message: StartMessage): void => {
    if (startPage) {
      if (mode !== 'start') {
        leavePrivate();
        mode = 'start';
        side = null;
        root.classList.add('is-start');
        root.setAttribute('aria-label', 'Start page');
        root.replaceChildren(startPage.root);
      }
      startPage.show(message);
      return;
    }
    // Messages that arrive while the bundle loads are kept, in order, and shown once it is in.
    startQueue.push(message);
    if (startLoading) return;
    startLoading = true;
    void loadStartPage().then((page) => {
      startPage = page.buildStartPage();
      const queued = startQueue;
      startQueue = [];
      // A hide that arrived meanwhile cancels the show.
      if (mode === 'none') for (const next of queued) showStart(next);
    }).catch(() => { startLoading = false; });
  };
  // --- end start page ---
  const leavePrivate = (): void => {
    if (mode === 'private') privatePage.hidden();
    root.classList.remove('is-private');
    leaveStart(); // --- start page ---
  };
  let side: SplitAt | null = null;
  // Any press inside the empty side makes it the focused side (the toolbar then follows it).
  root.addEventListener('pointerdown', () => {
    if (mode === 'pick') sendSplit({ v: 1, t: 'split', a: 'focus' });
  }, true);
  window.addEventListener('focus', () => {
    if (mode !== 'pick') return;
    sendSplit({ v: 1, t: 'split', a: 'focus' });
    const input = root.querySelector<HTMLInputElement>('.sv-input');
    if (input && document.activeElement !== input) input.focus({ preventScroll: true });
  });
  onInbound((msg) => {
    switch (msg.t) {
      case 'state':
        applyTheme(msg.theme, msg.solid, msg.dark, msg.fontScale, msg.uiFont, msg.density, msg.compactMenus, msg.look);
        break;
      case 'splitpick': {
        leavePrivate();
        const first = mode !== 'pick' || side !== msg.side;
        mode = 'pick';
        side = msg.side;
        picker.show(msg, first);
        break;
      }
      case 'splitdrop':
        leavePrivate();
        mode = 'drop';
        side = msg.side;
        root.replaceChildren(buildDrop(msg));
        root.setAttribute('aria-label', 'Split view drop target');
        break;
      case 'privatepage':
        if (mode !== 'private') {
          // Rust sends no splithide between the start page and the private page (U9).
          leaveStart();
          startQueue = [];
          mode = 'private';
          side = null;
          root.classList.add('is-private');
          root.setAttribute('aria-label', 'Private tab');
          root.replaceChildren(privatePage.root);
        }
        privatePage.show(msg);
        break;
      case 'splithide':
        leavePrivate();
        mode = 'none';
        side = null;
        startQueue = []; // --- start page ---
        root.replaceChildren();
        break;
      // --- start page --- a `snap` with p "startpage": the speed dials and widgets over a blank new tab
      case 'snap': {
        if (msg.p !== 'startpage') break;
        const message = parseStartMessage(msg.d);
        if (message) showStart(message);
        break;
      }
      // --- end start page ---
      default:
        break;
    }
  });
  send({ v: 1, t: 'ready' });
}
