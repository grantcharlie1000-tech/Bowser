// Command palette page in the popup host (`{t:"palette"}`). Styles: styles/palette.css.
//
// Markup contract:
//   .popup-page.bw-palette                 page root (child of #app.popup-pages)
//     .bw-palette-search > svg, input.bw-palette-input[role=combobox]
//     .bw-palette-list[role=listbox].bw-scroll
//       .bw-palette-section                section heading (sentence case)
//       .bw-palette-row[role=option][aria-selected][data-id] > svg, .bw-palette-text >
//           .bw-palette-label + .bw-palette-detail, .bw-palette-accel
//     .bw-palette-empty                    "No matches"
// Page text reaches the DOM through textContent only.
import { icon, type IconName } from './icons';
import { sendPopupDismiss, sendPopupPick, sendPopupSize } from './protocol';
import {
  filterPalette, flatten, moveSelection, MAX_QUERY, validPalettePayload,
  type PaletteItem, type PaletteMessage, type PaletteSection,
} from './palette-logic';

/** Width from the reference spec; Rust centres the popup 12% down the window. */
export const PALETTE_WIDTH = 560;

const SECTION_ICON: Readonly<Record<PaletteSection, IconName>> = {
  tab: 'app-tile',
  command: 'command',
  bookmark: 'bookmark',
  history: 'clock',
};

export interface PageView {
  /** Shows a message of this page type; false (and nothing changes) when it is invalid. */
  show(raw: unknown): boolean;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  return node;
}

export function createPaletteView(root: HTMLElement): PageView {
  let message: PaletteMessage | null = null;
  let rows: PaletteItem[] = [];
  let selected = -1;
  let closed = true;
  let lastSize = '';

  const page = el('div', 'popup-page bw-palette');
  const search = el('div', 'bw-palette-search');
  const input = el('input', 'bw-palette-input');
  input.type = 'text';
  input.spellcheck = false;
  input.autocomplete = 'off';
  input.maxLength = MAX_QUERY;
  input.dir = 'auto';
  input.setAttribute('role', 'combobox');
  input.setAttribute('aria-autocomplete', 'list');
  input.setAttribute('aria-expanded', 'true');
  input.setAttribute('aria-controls', 'bw-palette-list');
  input.setAttribute('aria-label', 'Command palette');
  search.append(icon('search'), input);
  const list = el('div', 'bw-palette-list bw-scroll');
  list.id = 'bw-palette-list';
  list.setAttribute('role', 'listbox');
  list.setAttribute('aria-label', 'Results');
  const empty = el('div', 'bw-palette-empty');
  empty.textContent = 'No matches';
  empty.setAttribute('role', 'status');
  page.append(search, list);

  const measure = (): void => {
    if (!message) return;
    const tok = message.tok;
    requestAnimationFrame(() => {
      if (!message || message.tok !== tok || closed) return;
      // Layout height: the entrance transform must not shrink the measurement.
      const height = Math.max(1, page.offsetHeight);
      const key = `${tok}:${height}`;
      if (key === lastSize) return;
      lastSize = key;
      sendPopupSize(tok, PALETTE_WIDTH, height);
    });
  };

  const markSelected = (): void => {
    input.removeAttribute('aria-activedescendant');
    for (const row of Array.from(list.querySelectorAll<HTMLElement>('.bw-palette-row'))) {
      const on = Number(row.dataset.index) === selected;
      row.setAttribute('aria-selected', String(on));
      if (on) {
        input.setAttribute('aria-activedescendant', row.id);
        row.scrollIntoView({ block: 'nearest' });
      }
    }
  };

  const pick = (index: number): void => {
    const item = rows[index];
    if (!message || closed || !item) return;
    closed = true;
    sendPopupPick(message.tok, item.id);
  };

  const dismiss = (): void => {
    if (!message || closed) return;
    closed = true;
    sendPopupDismiss(message.tok);
  };

  const render = (): void => {
    if (!message) return;
    const groups = filterPalette(message.items, input.value);
    rows = flatten(groups);
    selected = rows.length > 0 ? 0 : -1;
    const nodes: HTMLElement[] = [];
    let index = 0;
    for (const group of groups) {
      const heading = el('div', 'bw-palette-section');
      heading.setAttribute('role', 'presentation');
      heading.textContent = group.label;
      nodes.push(heading);
      for (const item of group.items) {
        const row = el('div', 'bw-palette-row');
        const at = index;
        row.id = `bw-palette-row-${at}`;
        row.dataset.index = String(at);
        row.dataset.id = String(item.id);
        row.setAttribute('role', 'option');
        const text = el('span', 'bw-palette-text');
        const label = el('span', 'bw-palette-label');
        label.textContent = item.label;
        label.dir = 'auto';
        text.append(label);
        if (item.detail) {
          const detail = el('span', 'bw-palette-detail');
          detail.textContent = item.detail;
          detail.dir = 'auto';
          text.append(detail);
        }
        row.append(icon(SECTION_ICON[item.s]), text);
        if (item.acc) {
          const accel = el('span', 'bw-palette-accel');
          accel.textContent = item.acc;
          row.setAttribute('aria-keyshortcuts', item.acc);
          row.append(accel);
        }
        row.addEventListener('mousemove', () => {
          if (selected === at) return;
          selected = at;
          markSelected();
        });
        row.addEventListener('click', () => pick(at));
        nodes.push(row);
        index += 1;
      }
    }
    if (nodes.length === 0) nodes.push(empty);
    list.replaceChildren(...nodes);
    markSelected();
    measure();
  };

  input.addEventListener('input', render);
  page.addEventListener('keydown', (event) => {
    if (event.isComposing) return;
    switch (event.key) {
      case 'ArrowDown':
      case 'ArrowUp':
      case 'PageDown':
      case 'PageUp':
        event.preventDefault();
        selected = moveSelection(selected, rows.length, event.key);
        markSelected();
        break;
      case 'Enter':
        event.preventDefault();
        pick(selected);
        break;
      case 'Escape':
        event.preventDefault();
        dismiss();
        break;
      case 'Tab':
        event.preventDefault();
        break;
    }
    // Keys stay on this page; the menu view listening on #app never sees them.
    event.stopPropagation();
  });
  for (const type of ['mousedown', 'mouseup', 'contextmenu'] as const) {
    page.addEventListener(type, (event) => {
      if (type === 'contextmenu') event.preventDefault();
      // A click on the list must not take focus from the search box.
      if (type === 'mousedown' && event.target !== input) event.preventDefault();
      event.stopPropagation();
    });
  }

  return {
    show(raw: unknown): boolean {
      if (!validPalettePayload(raw)) return false;
      message = raw;
      closed = false;
      lastSize = '';
      input.placeholder = raw.placeholder;
      input.value = '';
      root.className = 'popup-pages';
      root.removeAttribute('role');
      root.removeAttribute('aria-activedescendant');
      root.removeAttribute('aria-label');
      page.classList.remove('bw-enter', 'is-rise');
      root.replaceChildren(page);
      // Restart the entrance (fade + 8px rise) for each showing.
      void page.offsetWidth;
      page.classList.add('bw-enter', 'is-rise');
      render();
      input.focus({ preventScroll: true });
      return true;
    },
  };
}
