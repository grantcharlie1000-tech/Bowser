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
  sendSuggestPick, // --- omnibox suggest ---
  type HoverActFields,
  type HoverCardMsg,
  type HoverIslandMsg,
  type HoverMsg,
  type HoverRowMsg,
  type SuggestMsg,
} from './protocol';
import { suggestAction, suggestBadge, suggestEmphasis, suggestGlyph, suggestNeedle } from './suggest-logic'; // --- omnibox suggest ---
import { applyTheme } from './view';

type Act = (fields: HoverActFields) => void;

/** Marker names, in Rust's `tab_card::MARKERS` order. */
const MARKER_NAMES = ['Heart', 'Flame', 'Bag', 'Palm', 'Eyes'] as const;

function favicon(uri: string, className: string): HTMLElement {
  if (uri === '') {
    // No cached favicon: the family's globe glyph (never a bare bullet).
    const glyph = el('span', `${className} is-empty`);
    glyph.setAttribute('aria-hidden', 'true');
    glyph.append(icon('workspace-globe'));
    return glyph;
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

/** "41 MB", or "1.2 GB" from 1000 MB up. */
export function memoryText(mb: number): string {
  return mb >= 1000 ? `${(mb / 1024).toFixed(1)} GB` : `${mb} MB`;
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
    if (colour !== '') button.style.setProperty('--hover-marker', colour);
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
  if (msg.host !== '' || msg.sleeping || msg.mem !== undefined) {
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
    // --- tab options --- "Show memory use in tab previews".
    if (msg.mem !== undefined) {
      const memory = el('span', 'bw-card-badge bw-card-memory');
      memory.textContent = memoryText(msg.mem);
      meta.append(memory);
    }
    // --- end tab options ---
    text.append(meta);
  }
  const head = el('div', 'bw-card-head');
  head.append(favicon(msg.fav, 'bw-card-fav'), text);
  card.append(markers, head);
  // --- tab options --- "Show a thumbnail in tab previews".
  if (msg.thumb !== undefined) {
    const thumb = el('img', 'bw-card-thumb');
    thumb.alt = '';
    thumb.draggable = false;
    thumb.src = msg.thumb;
    card.append(thumb);
  }
  // --- end tab options ---
  card.append(...rowList(msg.rows, msg.more, 'bw-card', act));
  return card;
}

// --- omnibox suggest ---
/** `text` as a span of `className`, with the match of `needle` (if any) wrapped in <mark>. Text only. */
function matchedText(className: string, text: string, needle: string): HTMLElement {
  const span = el('span', className);
  span.dir = 'auto';
  const hit = suggestEmphasis(text, needle);
  if (!hit) {
    span.textContent = text;
    return span;
  }
  const mark = el('mark', 'bw-suggest-match');
  mark.textContent = text.slice(hit[0], hit[1]);
  span.append(document.createTextNode(text.slice(0, hit[0])), mark, document.createTextNode(text.slice(hit[1])));
  return span;
}

/** Moves the dropdown's selection to row `sel`: the row's state and the indicator layer under it. */
function selectSuggestRow(list: HTMLElement, sel: number): void {
  list.querySelectorAll('.bw-suggest-row').forEach((row, index) => {
    row.setAttribute('aria-selected', String(index === sel));
  });
  list.style.setProperty('--suggest-sel', String(sel));
}

/**
 * The omnibox dropdown: one row per suggestion. Rows never take focus; the toolbar keeps the
 * keyboard. Markup: .bw-suggest[role=listbox] > .bw-suggest-row[role=option][aria-selected]
 * [data-kind] > .bw-suggest-lead (favicon or glyph), .bw-suggest-text > .bw-suggest-title +
 * svg.bw-suggest-mark (a bookmark with a favicon) + .bw-suggest-url (title and address may each hold
 * one mark.bw-suggest-match), .bw-suggest-action, .bw-suggest-key; then one .bw-suggest-sel, the
 * selection indicator, after the rows.
 */
function renderSuggest(msg: SuggestMsg): HTMLElement {
  const list = el('div', 'bw-suggest');
  list.setAttribute('role', 'listbox');
  list.setAttribute('aria-label', 'Suggestions');
  list.style.width = `${msg.w}px`;
  const needle = suggestNeedle(msg.q);
  msg.items.forEach((item, index) => {
    const row = el('div', 'bw-suggest-row');
    row.setAttribute('role', 'option');
    row.dataset.kind = item.k;
    const lead = el('span', 'bw-suggest-lead');
    lead.setAttribute('aria-hidden', 'true');
    lead.append(item.fav !== '' ? favicon(item.fav, 'bw-suggest-fav') : icon(suggestGlyph(item.k)));
    const title = item.title !== '' ? item.title : item.text;
    const text = el('span', 'bw-suggest-text');
    text.append(matchedText('bw-suggest-title', title, needle));
    const badge = suggestBadge(item.k, item.fav !== '');
    if (badge) text.append(icon(badge.icon, { size: 12, fine: true, label: badge.label, className: 'bw-suggest-mark' }));
    if (item.url !== '' && item.url !== title) text.append(matchedText('bw-suggest-url', item.url, needle));
    row.append(lead, text);
    const action = suggestAction(item.k);
    if (action !== '') {
      const tag = el('span', 'bw-suggest-action');
      tag.textContent = action;
      row.append(tag);
    }
    // The Enter keycap, shown on the selected row only (decorative: the row is aria-selected).
    const key = el('span', 'bw-suggest-key');
    key.setAttribute('aria-hidden', 'true');
    key.append(icon('enter', { size: 12, fine: true }));
    row.append(key);
    // A press must not move the keyboard away from the omnibox.
    row.addEventListener('pointerdown', (ev) => { ev.preventDefault(); });
    row.addEventListener('click', () => { sendSuggestPick(msg.seq, index); });
    list.append(row);
  });
  const indicator = el('span', 'bw-suggest-sel');
  indicator.setAttribute('aria-hidden', 'true');
  list.append(indicator);
  selectSuggestRow(list, msg.sel);
  return list;
}
// --- end omnibox suggest ---

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
    if (colour !== '') button.style.setProperty('--hover-swatch', colour);
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
  let suggestSeq = -1; // --- omnibox suggest ---
  // Actions always carry the token of the content now shown.
  const act: Act = (fields) => { if (tok !== 0) sendHoverAct(tok, fields); };

  const render = (msg: HoverMsg): void => {
    tok = msg.tok;
    suggestSeq = -1; // --- omnibox suggest ---
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

  // --- omnibox suggest ---
  const renderSuggestMsg = (msg: SuggestMsg): void => {
    if (msg.tok === 0 || msg.items.length === 0) return;
    // Only the list's first appearance moves; later edits swap rows in place, so typing never animates.
    const opening = suggestSeq === -1;
    tok = msg.tok;
    suggestSeq = msg.seq;
    island = null;
    const content = renderSuggest(msg);
    document.body.dataset.hoverKind = 'suggest';
    root.replaceChildren(content);
    // Measured at the pill's CSS width before the entrance class: a transform would shrink the box.
    const box = content.getBoundingClientRect();
    sendPopupSize(msg.tok, Math.max(1, Math.min(4000, Math.ceil(box.width))), Math.max(1, Math.min(4000, Math.ceil(box.height))));
    // Rust sizes the popup to the painted pill's device pixels, which can differ from w CSS px by a
    // fraction; filling the viewport keeps the side borders on the popup's (and the pill's) edges.
    // Same task as the measurement, so no frame ever paints the measuring width.
    content.style.width = '100vw';
    if (opening) content.classList.add('is-entering');
  };
  // --- end omnibox suggest ---

  onInbound((msg) => {
    switch (msg.t) {
      case 'state':
        applyTheme(msg.theme, msg.solid, msg.dark);
        break;
      // --- omnibox suggest ---
      case 'suggest':
        renderSuggestMsg(msg);
        break;
      case 'suggestSel': {
        if (msg.seq !== suggestSeq) break;
        const list = root.querySelector<HTMLElement>('.bw-suggest');
        if (!list) break;
        // From the first move on, the indicator slides (83ms); a fresh list places it at once.
        list.classList.add('is-tracking');
        selectSuggestRow(list, msg.sel);
        break;
      }
      // --- end omnibox suggest ---
      case 'hover':
        render(msg);
        break;
      case 'hoverhide':
        // A draft still in the field is committed before the token goes; Rust accepts it for the
        // island it was typed for.
        if (island && island.input.value !== island.committed) act({ a: 'name', text: island.input.value });
        tok = 0;
        suggestSeq = -1; // --- omnibox suggest ---
        island = null;
        delete document.body.dataset.hoverKind;
        root.replaceChildren();
        break;
    }
  });
  send({ v: 1, t: 'ready' });
}
// --- end hover layer ---
