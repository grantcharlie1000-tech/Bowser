// --- hover layer ---
// The hover view (?view=hover): tooltips, the tab hover card and the island menu, drawn in the
// no-activate hover popup. Rust posts content with a token; the view renders it, measures it and
// replies with its size. Page text is written with textContent only; favicons are validated data:
// URIs. Stable class names: .bw-tooltip, .bw-card, .bw-island-menu.
import { el, icon } from './dom';
import {
  MAX_ISLAND_NAME,
  onInbound,
  send,
  sendHoverAct,
  sendPopupSize,
  type HoverActFields,
  type HoverCardMsg,
  type HoverIslandMsg,
  type HoverMsg,
  type HoverRowMsg,
} from './protocol';
import { applyTheme } from './view';

type Act = (fields: HoverActFields) => void;

/** Marker names, in Rust's `tab_card::MARKERS` order. */
const MARKER_NAMES = ['Heart', 'Flame', 'Bag', 'Palm', 'Eyes'] as const;

function favicon(uri: string, className: string): HTMLElement {
  if (uri === '') {
    const dot = el('span', `${className} is-empty`);
    dot.setAttribute('aria-hidden', 'true');
    return dot;
  }
  const img = el('img', className);
  img.alt = '';
  img.draggable = false;
  img.src = uri;
  return img;
}

function rowList(rows: readonly HoverRowMsg[], more: number, prefix: string, act: Act): HTMLElement[] {
  const out: HTMLElement[] = [];
  if (rows.length > 0) {
    const list = el('div', `${prefix}-rows`);
    list.setAttribute('role', 'list');
    rows.forEach((row, index) => {
      const button = el('button', `${prefix}-row`);
      button.type = 'button';
      button.setAttribute('role', 'listitem');
      button.classList.toggle('is-active', row.active);
      button.classList.toggle('is-linked', row.linked);
      const title = el('span', `${prefix}-row-title`);
      title.dir = 'auto';
      title.textContent = row.title;
      button.append(favicon(row.fav, `${prefix}-row-fav`), title);
      button.addEventListener('click', () => act({ a: 'row', n: index }));
      list.append(button);
    });
    out.push(list);
  }
  if (more > 0) {
    const line = el('div', `${prefix}-more`);
    line.textContent = `+${more} more`;
    out.push(line);
  }
  return out;
}

function renderTooltip(text: string): HTMLElement {
  const tip = el('div', 'bw-tooltip');
  tip.setAttribute('role', 'tooltip');
  tip.dir = 'auto';
  tip.textContent = text;
  return tip;
}

function renderCard(msg: HoverCardMsg, act: Act): HTMLElement {
  const card = el('div', 'bw-card');
  const markers = el('div', 'bw-card-markers');
  msg.markers.forEach((colour, index) => {
    const button = el('button', 'bw-card-marker');
    button.type = 'button';
    button.setAttribute('aria-label', `${MARKER_NAMES[index] ?? 'Marker'} marker`);
    button.setAttribute('aria-pressed', String(msg.marker === index));
    button.classList.toggle('is-selected', msg.marker === index);
    if (colour !== '') button.style.setProperty('--bw-marker', colour);
    button.addEventListener('click', () => act({ a: 'marker', n: index }));
    markers.append(button);
  });
  const next = el('button', 'bw-card-marker-next');
  next.type = 'button';
  next.setAttribute('aria-label', 'Next marker');
  next.append(icon('plus'));
  next.addEventListener('click', () => act({ a: 'next' }));
  markers.append(next);

  const text = el('div', 'bw-card-text');
  const title = el('div', 'bw-card-title');
  title.dir = 'auto';
  title.textContent = msg.title;
  text.append(title);
  if (msg.host !== '' || msg.sleeping) {
    const meta = el('div', 'bw-card-meta');
    if (msg.host !== '') {
      const host = el('span', 'bw-card-host');
      host.dir = 'auto';
      host.textContent = msg.host;
      meta.append(host);
    }
    if (msg.sleeping) {
      const badge = el('span', 'bw-card-badge');
      badge.textContent = 'Sleeping';
      meta.append(badge);
    }
    text.append(meta);
  }
  const head = el('div', 'bw-card-head');
  head.append(favicon(msg.fav, 'bw-card-fav'), text);
  card.append(markers, head, ...rowList(msg.rows, msg.more, 'bw-card', act));
  return card;
}

/** The island menu's DOM. The name field is built once and kept while the menu refreshes. */
interface IslandView {
  readonly menu: HTMLElement;
  readonly field: HTMLElement;
  readonly input: HTMLInputElement;
  /** The name as Rust last stored it; the draft is compared with it. */
  committed: string;
}

function buildIsland(msg: HoverIslandMsg, act: Act): IslandView {
  const menu = el('div', 'bw-island-menu');
  const field = el('div', 'bw-island-field');
  const input = el('input', 'bw-island-name');
  input.type = 'text';
  input.spellcheck = false;
  input.autocomplete = 'off';
  input.dir = 'auto';
  input.maxLength = MAX_ISLAND_NAME;
  input.setAttribute('aria-label', 'Tab island name');
  input.value = msg.name;
  const clear = el('button', 'bw-island-clear');
  clear.type = 'button';
  clear.tabIndex = -1;
  clear.setAttribute('aria-label', 'Clear name');
  clear.append(icon('close'));
  field.append(input, clear);
  menu.append(field);
  const view: IslandView = { menu, field, input, committed: msg.name };

  const syncClear = (): void => { field.classList.toggle('is-empty', input.value === ''); };
  const commit = (): void => {
    if (input.value === view.committed) return;
    view.committed = input.value;
    act({ a: 'name', text: input.value });
  };
  syncClear();
  // The popup never activates on its own: the field asks Rust for the keyboard when clicked.
  input.addEventListener('pointerdown', () => act({ a: 'focus' }));
  input.addEventListener('input', syncClear);
  input.addEventListener('blur', commit);
  input.addEventListener('keydown', (ev) => {
    if (ev.isComposing) return;
    if (ev.key === 'Enter') {
      ev.preventDefault();
      commit();
      act({ a: 'dismiss' });
    } else if (ev.key === 'Escape') {
      ev.preventDefault();
      input.value = view.committed;
      syncClear();
      act({ a: 'dismiss' });
    }
  });
  clear.addEventListener('pointerdown', (ev) => {
    ev.preventDefault();
    input.value = '';
    syncClear();
    act({ a: 'focus' });
    input.focus();
  });
  menu.addEventListener('click', (ev) => {
    // Every other control commits a changed draft first, as the native menu does on close.
    if (ev.target instanceof Element && !field.contains(ev.target)) commit();
  }, true);
  return view;
}

/** Rebuilds everything after the name field; the field (and any draft in it) stays. */
function fillIsland(view: IslandView, msg: HoverIslandMsg, act: Act, keepDraft: boolean): void {
  view.input.placeholder = msg.hint;
  if (!keepDraft) {
    view.input.value = msg.name;
    view.field.classList.toggle('is-empty', msg.name === '');
  }
  view.committed = msg.name;
  while (view.field.nextSibling) view.field.nextSibling.remove();

  const swatches = el('div', 'bw-island-swatches');
  msg.swatches.forEach((colour, index) => {
    const button = el('button', 'bw-island-swatch');
    button.type = 'button';
    button.setAttribute('aria-label', index === 0 ? 'Theme colour' : `Colour ${index}`);
    button.setAttribute('aria-pressed', String(msg.slot === index));
    button.classList.toggle('is-selected', msg.slot === index);
    if (colour !== '') button.style.setProperty('--bw-swatch', colour);
    button.addEventListener('click', () => act({ a: 'swatch', n: index }));
    swatches.append(button);
  });
  const close = el('button', 'bw-island-close');
  close.type = 'button';
  close.textContent = msg.close;
  close.addEventListener('click', () => act({ a: 'close' }));
  view.menu.append(swatches, ...rowList(msg.rows, msg.more, 'bw-island', act), close);
}

/** Builds the hover view into `root` and wires it to the host. */
export function startHover(root: HTMLElement): void {
  root.replaceChildren();
  root.className = 'bw-hover-root';
  document.body.classList.add('bw-hover-body');
  let tok = 0;
  let island: IslandView | null = null;
  // Actions always carry the token of the content now shown.
  const act: Act = (fields) => { if (tok !== 0) sendHoverAct(tok, fields); };

  const render = (msg: HoverMsg): void => {
    tok = msg.tok;
    let content: HTMLElement;
    if (msg.kind === 'island') {
      const typing = island !== null && document.activeElement === island.input;
      if (!island || !typing) island = buildIsland(msg, act);
      fillIsland(island, msg, act, typing);
      content = island.menu;
    } else {
      island = null;
      content = msg.kind === 'tooltip' ? renderTooltip(msg.text) : renderCard(msg, act);
    }
    document.body.dataset.hoverKind = msg.kind;
    if (root.firstElementChild !== content || root.childElementCount !== 1) root.replaceChildren(content);
    // Layout is synchronous: the popup is hidden, so no animation frame may run before Rust shows it.
    const box = content.getBoundingClientRect();
    sendPopupSize(msg.tok, Math.max(1, Math.min(4000, Math.ceil(box.width))), Math.max(1, Math.min(4000, Math.ceil(box.height))));
  };

  onInbound((msg) => {
    switch (msg.t) {
      case 'state':
        applyTheme(msg.theme, msg.solid, msg.dark);
        break;
      case 'hover':
        render(msg);
        break;
      case 'hoverhide':
        // A draft still in the field is committed before the token goes; Rust accepts it for the
        // island it was typed for.
        if (island && island.input.value !== island.committed) act({ a: 'name', text: island.input.value });
        tok = 0;
        island = null;
        delete document.body.dataset.hoverKind;
        root.replaceChildren();
        break;
    }
  });
  send({ v: 1, t: 'ready' });
}
// --- end hover layer ---
