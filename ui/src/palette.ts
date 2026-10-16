// Command palette page in the popup host (`{t:"palette"}`). Styles: styles/palette.css.
//
// One page, two modes. `palette` is Quick Commands: search open tabs, recently closed tabs,
// workspaces, bookmarks, notes, history, commands and page actions, with filter prefixes
// (`bkm:`, `tb:`, `cmd:` ...), a calculator row and an inline completion that Shift or Right arrow
// accepts. `shortcuts` is the Ctrl+F1 cheat sheet: every command with a shortcut, grouped by
// category and filtered as you type; nothing in it can be picked.
//
// Markup contract:
//   .popup-page.bw-palette                 page root (child of #app.popup-pages)
//     .bw-palette-search > svg, .bw-palette-field > (.bw-palette-ghost, input.bw-palette-input[role=combobox])
//     .bw-palette-list[role=listbox].bw-scroll
//       .bw-palette-section                section heading (sentence case)
//       .bw-palette-row[role=option][aria-selected][data-id] > svg, .bw-palette-text >
//           .bw-palette-label + .bw-palette-detail, .bw-palette-accel
//     .bw-palette-empty                    "No matches"
// Page text reaches the DOM through textContent only.
import { icon, type IconName } from './icons';
import { sendPopupDismiss, sendPopupPick, sendPopupSize } from './protocol';
import {
  CALC_ID, commandGlyph, completion, filterPalette, filterShortcuts, flatten, moveSelection, MAX_QUERY, rowLabel, validPalettePayload,
  type FilterOptions, type PaletteGroup, type PaletteItem, type PaletteMessage, type ResultType,
} from './palette-logic';

/** Width from the reference spec; Rust centres the popup 12% down the window. */
export const PALETTE_WIDTH = 560;

/** Fallback glyph while a row has no favicon (commands pick their own, see commandGlyph). */
const SECTION_ICON: Readonly<Record<ResultType, IconName>> = {
  tab: 'workspace-globe',
  closed: 'tab-close',
  workspace: 'workspace',
  command: 'command',
  action: 'window',
  bookmark: 'bookmark',
  note: 'notes',
  history: 'clock',
  calc: 'grid',
};

/** Leading visual of a row: the cached favicon for pages, else the command's or section's glyph. */
function leading(item: PaletteItem): Element {
  if (item.fav !== undefined && item.s !== 'command' && item.s !== 'action') {
    const img = el('img', 'bw-palette-fav');
    img.alt = '';
    img.draggable = false;
    img.src = item.fav;
    return img;
  }
  const name = item.s === 'command' ? (commandGlyph(item.label) as IconName) : SECTION_ICON[item.s];
  return icon(name);
}

export interface PageView {
  /** Shows a message of this page type; false (and nothing changes) when it is invalid. */
  show(raw: unknown): boolean;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  return node;
}

/** Puts `text` on the clipboard: the async API first, then the old copy command from a hidden field. */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // fall through to the copy command
  }
  try {
    const field = document.createElement('textarea');
    field.value = text;
    field.setAttribute('aria-hidden', 'true');
    field.style.cssText = 'position:fixed;opacity:0;pointer-events:none';
    document.body.append(field);
    field.select();
    const ok = document.execCommand('copy');
    field.remove();
    return ok;
  } catch {
    return false;
  }
}

/** What the palette typed last, for the "keep the last typed value" option. Lives as long as the window. */
let lastQuery = '';

export function createPaletteView(root: HTMLElement): PageView {
  let message: PaletteMessage | null = null;
  let rows: PaletteItem[] = [];
  let selected = -1;
  let closed = true;
  let lastSize = '';
  let ghost: string | null = null;
  let shiftTap = false;

  const page = el('div', 'popup-page bw-palette');
  const search = el('div', 'bw-palette-search');
  const field = el('div', 'bw-palette-field');
  const ghostText = el('span', 'bw-palette-ghost');
  ghostText.setAttribute('aria-hidden', 'true');
  const ghostTyped = el('span', 'bw-palette-ghost-typed');
  const ghostRest = el('span', 'bw-palette-ghost-rest');
  ghostText.append(ghostTyped, ghostRest);
  const input = el('input', 'bw-palette-input');
  input.type = 'text';
  input.spellcheck = false;
  input.autocomplete = 'off';
  input.maxLength = MAX_QUERY;
  input.dir = 'auto';
  input.setAttribute('role', 'combobox');
  input.setAttribute('aria-autocomplete', 'both');
  input.setAttribute('aria-expanded', 'true');
  input.setAttribute('aria-controls', 'bw-palette-list');
  input.setAttribute('aria-label', 'Command palette');
  field.append(ghostText, input);
  search.append(icon('search'), field);
  const list = el('div', 'bw-palette-list bw-scroll');
  list.id = 'bw-palette-list';
  list.setAttribute('role', 'listbox');
  list.setAttribute('aria-label', 'Results');
  const empty = el('div', 'bw-palette-empty');
  empty.textContent = 'No matches';
  empty.setAttribute('role', 'status');
  page.append(search, list);

  const filterOptions = (): FilterOptions => ({
    types: message?.types ?? [],
    ranked: message?.ranked ?? true,
    limit: message?.limit ?? 12,
  });

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

  const paintGhost = (): void => {
    const top = rows[0];
    const next = message?.mode === 'palette' && top && top.s !== 'calc' && input.value !== '' ? completion(input.value, rowLabel(top)) : null;
    ghost = next;
    ghostText.hidden = next === null;
    ghostTyped.textContent = next === null ? '' : input.value;
    ghostRest.textContent = next === null ? '' : next.slice(input.value.length);
  };

  const dismiss = (): void => {
    if (!message || closed) return;
    closed = true;
    sendPopupDismiss(message.tok);
  };

  const pick = (index: number): void => {
    const item = rows[index];
    if (!message || closed || !item || message.mode === 'shortcuts') return;
    if (item.id === CALC_ID) {
      // The calculator row is the page's own: copy the result and leave.
      const value = item.label;
      void copyText(value).then((ok) => {
        if (ok) dismiss();
        else { input.value = value; input.select(); render(); }
      });
      return;
    }
    closed = true;
    sendPopupPick(message.tok, item.id);
  };

  function render(): void {
    if (!message) return;
    const groups: PaletteGroup[] = message.mode === 'shortcuts'
      ? filterShortcuts(message.items, input.value)
      : filterPalette(message.items, input.value, filterOptions());
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
        label.textContent = rowLabel(item);
        label.dir = 'auto';
        text.append(label);
        if (item.detail) {
          const detail = el('span', 'bw-palette-detail');
          detail.textContent = item.detail;
          detail.dir = 'auto';
          text.append(detail);
        }
        row.append(leading(item), text);
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
    paintGhost();
    measure();
  }

  /** Completes the box to the top result (Shift tapped, or Right arrow at the end). */
  const accept = (): void => {
    if (ghost === null) return;
    input.value = ghost;
    input.setSelectionRange(ghost.length, ghost.length);
    remember();
    render();
  };

  /** Keeps what was typed for the next opening (used only when the option is on). */
  const remember = (): void => {
    if (message?.mode === 'palette') lastQuery = input.value;
  };

  input.addEventListener('input', () => { remember(); render(); });
  page.addEventListener('keydown', (event) => {
    if (event.isComposing) return;
    if (event.key === 'Shift') shiftTap = !event.repeat;
    else shiftTap = false;
    switch (event.key) {
      case 'ArrowDown':
      case 'ArrowUp':
      case 'PageDown':
      case 'PageUp':
        event.preventDefault();
        selected = moveSelection(selected, rows.length, event.key);
        markSelected();
        break;
      case 'ArrowRight':
        if (ghost !== null && input.selectionStart === input.value.length && input.selectionEnd === input.value.length) {
          event.preventDefault();
          accept();
        }
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
  // Shift pressed and released on its own completes the box (a Shift held for a capital letter does not).
  page.addEventListener('keyup', (event) => {
    if (event.key === 'Shift' && shiftTap) accept();
    shiftTap = false;
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
      shiftTap = false;
      input.placeholder = raw.placeholder;
      input.value = raw.mode === 'palette' && raw.keep ? lastQuery : '';
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
      // A kept query is selected, so the first key typed replaces it.
      if (input.value !== '') input.select();
      return true;
    },
  };
}
