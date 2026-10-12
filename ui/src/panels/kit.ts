// Shared building blocks for the side panels and dialog pages (history, bookmarks, notes,
// settings, privacy): header, search field, empty state, icon buttons, roving list keys.
import { el, icon, type IconName } from '../dom';
import { sendPanel } from '../protocol';
import { isNavKey, navIndex, tileLetter } from './kit-logic';

export function dismissPanel(): void {
  sendPanel('panel', { a: 'dismiss' });
}

/** A 20px icon button with an accessible name and native tooltip. */
export function iconButton(name: IconName, label: string, onClick: (event: MouseEvent) => void, extra = ''): HTMLButtonElement {
  const button = el('button', `bw-icon-btn ${extra}`.trim());
  button.type = 'button';
  button.setAttribute('data-tip', label);
  button.setAttribute('aria-label', label);
  button.append(icon(name));
  button.addEventListener('click', onClick);
  return button;
}

/** A text button, optionally led by an icon. `primary` fills with the accent. */
export function textButton(label: string, onClick: (event: MouseEvent) => void, options: { icon?: IconName; primary?: boolean; subtle?: boolean; aria?: string } = {}): HTMLButtonElement {
  const button = el('button', `bw-button${options.primary ? ' is-primary' : ''}${options.subtle ? ' is-subtle' : ''}`);
  button.type = 'button';
  if (options.icon) button.append(icon(options.icon));
  button.append(document.createTextNode(label));
  if (options.aria) button.setAttribute('aria-label', options.aria);
  button.addEventListener('click', onClick);
  return button;
}

export interface Header { readonly root: HTMLElement; readonly title: HTMLElement; readonly tools: HTMLElement }

/** Panel header: title in BodyStrong, optional tool buttons, and the close button. */
export function panelHeader(title: string, closeLabel = 'Close', level: 'h1' | 'h2' = 'h1'): Header {
  const root = el('header', 'pn-header');
  const heading = el(level, 'pn-title');
  heading.textContent = title;
  const tools = el('div', 'pn-tools');
  root.append(heading, tools, iconButton('close', closeLabel, dismissPanel));
  return { root, title: heading, tools };
}

export interface SearchField { readonly root: HTMLElement; readonly input: HTMLInputElement }

export function searchField(label: string): SearchField {
  const root = el('label', 'pn-search');
  const input = el('input', 'bw-input');
  input.type = 'search';
  input.placeholder = label;
  input.autocomplete = 'off';
  input.spellcheck = false;
  input.setAttribute('aria-label', label);
  root.append(icon('search'), input);
  return { root, input };
}

/** Icon plus one line. No illustration. */
export function emptyState(name: IconName, text: string): HTMLElement {
  const root = el('div', 'pn-empty');
  root.setAttribute('role', 'status');
  const line = el('p');
  line.textContent = text;
  root.append(icon(name, { size: 20 }), line);
  return root;
}

/** Placeholder favicon: a 16px tile with the first letter of the host or title. */
export function faviconTile(text: string): HTMLElement {
  const tile = el('span', 'pn-favicon');
  tile.setAttribute('aria-hidden', 'true');
  tile.textContent = tileLetter(text);
  return tile;
}

/** Sentence-case sticky section header. */
export function sectionHeader(text: string): HTMLElement {
  const heading = el('h2', 'pn-day');
  heading.textContent = text;
  return heading;
}

/**
 * Roving arrow navigation inside `container` over elements matching `selector`. Returns true when
 * the key moved focus. Text fields keep Home and End for the caret.
 */
export function moveFocus(container: HTMLElement, selector: string, event: KeyboardEvent): boolean {
  if (!isNavKey(event.key) || event.altKey || event.ctrlKey || event.metaKey) return false;
  const target = event.target;
  if ((target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) && (event.key === 'Home' || event.key === 'End')) return false;
  const items = Array.from(container.querySelectorAll<HTMLElement>(selector));
  if (items.length === 0) return false;
  const current = items.findIndex((item) => item === document.activeElement || item.contains(document.activeElement));
  const next = navIndex(items.length, current, event.key);
  if (next < 0) return false;
  event.preventDefault();
  if (next !== current) {
    items[next]?.focus();
    items[next]?.scrollIntoView({ block: 'nearest' });
  }
  return true;
}

/** Marks the panel root for the shared enter animation (opacity + 4px rise, 167ms). */
export function enter(node: HTMLElement): void {
  node.classList.add('bw-enter', 'pn-enter');
}

// ---- Settings-style pages (settings, privacy): sections of 48px rows, label left, control right.

let rowSeq = 0;

export interface PageSection { readonly root: HTMLElement; readonly body: HTMLElement }

/** A titled group of rows separated by hairlines. No card, no uppercase eyebrow. */
export function pageSection(title: string): PageSection {
  const root = el('section', 'pg-section');
  const heading = el('h2', 'pg-heading');
  heading.textContent = title;
  const body = el('div', 'pg-rows');
  root.append(heading, body);
  return { root, body };
}

/** 48px row: label and optional description on the left, control (or plain value) on the right. */
export function settingRow(label: string, description: string | null, control?: HTMLElement | string): HTMLElement {
  const row = el('div', 'bw-row is-setting pg-row');
  const text = el('div', 'pg-text');
  const name = el('span', 'pg-label');
  const id = ++rowSeq;
  name.id = `pg-label-${id}`;
  name.textContent = label;
  text.append(name);
  if (description) {
    const hint = el('span', 'pg-desc');
    hint.id = `pg-desc-${id}`;
    hint.textContent = description;
    text.append(hint);
  }
  row.append(text);
  if (typeof control === 'string') {
    const value = el('span', 'pg-value');
    value.textContent = control;
    row.append(value);
  } else if (control) {
    const wrap = el('div', 'pg-control');
    wrap.append(control);
    if (!control.hasAttribute('aria-label')) control.setAttribute('aria-labelledby', name.id);
    if (description) control.setAttribute('aria-describedby', `pg-desc-${id}`);
    row.append(wrap);
  }
  return row;
}

/** `<button role="switch">` toggle. Calls `onChange` with the new state. */
export function toggleSwitch(checked: boolean, onChange: (on: boolean) => void): HTMLButtonElement {
  const button = el('button', 'bw-toggle');
  button.type = 'button';
  button.setAttribute('role', 'switch');
  button.setAttribute('aria-checked', String(checked));
  button.addEventListener('click', () => {
    const next = button.getAttribute('aria-checked') !== 'true';
    button.setAttribute('aria-checked', String(next));
    onChange(next);
  });
  return button;
}

/** Radio-style segmented control with roving tabindex and arrow keys. */
export function segmented<T extends string>(options: ReadonlyArray<readonly [T, string]>, current: T | null, onSelect: (value: T) => void, label: string): HTMLElement {
  const group = el('div', 'bw-segmented');
  group.setAttribute('role', 'radiogroup');
  group.setAttribute('aria-label', label);
  const buttons: HTMLButtonElement[] = [];
  const paint = (value: T | null): void => {
    buttons.forEach((button, index) => {
      const on = options[index]?.[0] === value;
      button.setAttribute('aria-checked', String(on));
      button.tabIndex = on || (value === null && index === 0) ? 0 : -1;
    });
  };
  for (const [value, text] of options) {
    const button = el('button');
    button.type = 'button';
    button.setAttribute('role', 'radio');
    button.textContent = text;
    button.addEventListener('click', () => { paint(value); onSelect(value); });
    buttons.push(button);
  }
  group.addEventListener('keydown', (event) => {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight' && event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return;
    event.preventDefault();
    const index = buttons.findIndex((button) => button === document.activeElement);
    const step = event.key === 'ArrowLeft' || event.key === 'ArrowUp' ? -1 : 1;
    const next = buttons[(index + step + buttons.length) % buttons.length];
    next?.focus();
    next?.click();
  });
  group.append(...buttons);
  paint(current);
  return group;
}
