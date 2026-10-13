// HTML select: a button that opens a listbox built from the shared .bw-menu styles. There is no
// native <select>: the list is ordinary DOM, so it follows the theme, scales with DPI and never
// shows the Windows dropdown. Select-only combobox pattern: focus stays on the button and the
// highlighted option is announced through aria-activedescendant.
import { el, icon } from '../dom';
import { navIndex } from './kit-logic';
import { isTypeKey, opensSelect, placeList, typeaheadIndex } from './select-logic';

export interface SelectOption<T extends string> {
  readonly value: T;
  readonly label: string;
  /** Optional leading glyph or swatch, drawn in the option and in the button. */
  readonly lead?: () => Node;
}

export interface SelectConfig<T extends string> {
  readonly options: readonly SelectOption<T>[];
  readonly value: T | null;
  /** Accessible name of the button. */
  readonly label: string;
  /** Shown while no option is chosen. */
  readonly placeholder: string;
  readonly onChange: (value: T) => void;
  /** When true the button returns to the placeholder after every pick (a preset picker). */
  readonly momentary?: boolean;
  /** Minimum width of the open list in px (the list is as wide as the button otherwise). */
  readonly minListWidth?: number;
}

export interface SelectField<T extends string> {
  readonly root: HTMLElement;
  readonly button: HTMLButtonElement;
  get(): T | null;
  set(value: T | null): void;
  close(): void;
}

let sequence = 0;
const TYPEAHEAD_MS = 700;
const MAX_LIST_PX = 288;

export function selectField<T extends string>(config: SelectConfig<T>): SelectField<T> {
  const { options } = config;
  const id = `bw-select-${++sequence}`;
  let value = config.value;
  let active = -1;
  let list: HTMLElement | null = null;
  let query = '';
  let queryTimer = 0;

  const root = el('div', 'bw-select');
  const button = el('button', 'bw-select-button');
  button.type = 'button';
  button.setAttribute('role', 'combobox');
  button.setAttribute('aria-haspopup', 'listbox');
  button.setAttribute('aria-expanded', 'false');
  button.setAttribute('aria-label', config.label);
  const face = el('span', 'bw-select-value');
  button.append(face, icon('chevron-down', { size: 12, fine: true }));
  root.append(button);

  const indexOf = (item: T | null): number => options.findIndex((option) => option.value === item);

  function paint(): void {
    const chosen = options[indexOf(value)];
    face.replaceChildren();
    face.classList.toggle('is-placeholder', !chosen);
    if (chosen?.lead) face.append(wrapLead(chosen.lead()));
    face.append(document.createTextNode(chosen ? chosen.label : config.placeholder));
  }

  function wrapLead(node: Node): HTMLElement {
    const lead = el('span', 'bw-select-lead');
    lead.append(node);
    return lead;
  }

  function highlight(index: number, scroll = true): void {
    if (!list) return;
    const rows = list.children;
    for (let i = 0; i < rows.length; i += 1) rows[i]?.classList.toggle('is-active', i === index);
    active = index;
    const row = rows[index] as HTMLElement | undefined;
    if (row) {
      button.setAttribute('aria-activedescendant', row.id);
      if (scroll) row.scrollIntoView({ block: 'nearest' });
    } else button.removeAttribute('aria-activedescendant');
  }

  function place(): void {
    if (!list) return;
    const rect = button.getBoundingClientRect();
    list.style.setProperty('max-height', 'none');
    const placement = placeList(rect, { width: window.innerWidth, height: window.innerHeight }, Math.min(list.scrollHeight, MAX_LIST_PX), 4, 8, config.minListWidth ?? 0);
    list.style.setProperty('top', `${placement.top}px`);
    list.style.setProperty('left', `${placement.left}px`);
    list.style.setProperty('width', `${placement.width}px`);
    list.style.setProperty('max-height', `${Math.min(placement.maxHeight, MAX_LIST_PX)}px`);
    list.classList.toggle('is-above', placement.above);
  }

  function onOutside(event: Event): void {
    if (event.target instanceof Node && (root.contains(event.target) || list?.contains(event.target))) return;
    close();
  }
  function onScroll(event: Event): void {
    if (event.target instanceof Node && list?.contains(event.target)) return;
    close();
  }

  function open(initial: number): void {
    if (list) return;
    list = el('div', 'bw-menu bw-listbox bw-scroll bw-enter is-down');
    list.id = `${id}-list`;
    list.setAttribute('role', 'listbox');
    list.setAttribute('aria-label', config.label);
    options.forEach((option, index) => {
      const row = el('div', 'bw-menu-item');
      row.id = `${id}-o${index}`;
      row.setAttribute('role', 'option');
      row.setAttribute('aria-selected', String(option.value === value));
      if (option.lead) row.append(wrapLead(option.lead()));
      const text = el('span', 'bw-menu-label');
      text.textContent = option.label;
      row.append(text);
      if (option.value === value) {
        const check = el('span', 'bw-menu-check');
        check.append(icon('check'));
        row.append(check);
      }
      row.addEventListener('pointermove', () => { if (active !== index) highlight(index, false); });
      row.addEventListener('click', () => choose(index));
      list?.append(row);
    });
    // Keep focus on the button while the pointer works the list.
    list.addEventListener('pointerdown', (event) => event.preventDefault());
    root.append(list);
    button.setAttribute('aria-expanded', 'true');
    button.setAttribute('aria-controls', list.id);
    place();
    highlight(initial);
    document.addEventListener('pointerdown', onOutside, true);
    window.addEventListener('scroll', onScroll, true);
    window.addEventListener('resize', close);
    window.addEventListener('blur', close);
  }

  function close(): void {
    if (!list) return;
    list.remove();
    list = null;
    active = -1;
    query = '';
    button.setAttribute('aria-expanded', 'false');
    button.removeAttribute('aria-controls');
    button.removeAttribute('aria-activedescendant');
    document.removeEventListener('pointerdown', onOutside, true);
    window.removeEventListener('scroll', onScroll, true);
    window.removeEventListener('resize', close);
    window.removeEventListener('blur', close);
  }

  function choose(index: number): void {
    const option = options[index];
    close();
    button.focus();
    if (!option) return;
    value = config.momentary ? null : option.value;
    paint();
    config.onChange(option.value);
  }

  function typeahead(char: string): void {
    query += char;
    window.clearTimeout(queryTimer);
    queryTimer = window.setTimeout(() => { query = ''; }, TYPEAHEAD_MS);
    const hit = typeaheadIndex(options.map((option) => option.label), active, query);
    if (hit >= 0) highlight(hit);
  }

  button.addEventListener('click', () => {
    if (list) close();
    else open(Math.max(indexOf(value), 0));
  });
  button.addEventListener('blur', close);
  button.addEventListener('keydown', (event) => {
    if (!list) {
      if (!opensSelect(event.key) || event.altKey || event.ctrlKey || event.metaKey) return;
      event.preventDefault();
      if (event.key === 'Home') open(0);
      else if (event.key === 'End') open(options.length - 1);
      else open(Math.max(indexOf(value), 0));
      return;
    }
    switch (event.key) {
      case 'Escape':
        // Closes the list only; the panel host must not also dismiss itself.
        event.preventDefault();
        event.stopPropagation();
        close();
        return;
      case 'Tab':
        close();
        return;
      case 'Enter':
      case ' ':
        event.preventDefault();
        if (active >= 0) choose(active);
        return;
      case 'ArrowDown': case 'ArrowUp': case 'Home': case 'End':
        event.preventDefault();
        highlight(navIndex(options.length, active, event.key));
        return;
      default:
        if (isTypeKey(event)) {
          event.preventDefault();
          typeahead(event.key);
        }
    }
  });

  paint();
  return {
    root,
    button,
    get: () => value,
    set(next) { value = next; paint(); },
    close,
  };
}
