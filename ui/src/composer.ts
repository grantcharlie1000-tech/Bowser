// Anchored composer page in the popup host (`{t:"composer"}`): New workspace, Add web app and Edit
// web app. It sits on the menu surface (#app.html-menu[data-kind=composer], styles in menu.css) so
// it has the menu's radius, border, shadow, size reporting and enter motion; its content is one
// wrapper so the menu view listening on #app never sees its keys.
//
// Markup contract:
//   #app.html-menu[data-kind=composer] > .html-composer[role=dialog][aria-modal][aria-labelledby]
//     .html-menu-section#html-composer-heading          "New workspace" | "Add web app" | "Edit web app"
//     .html-composer-field[data-field] > .html-menu-lead (glyph) + input.bw-input.html-composer-input
//     .html-composer-error[aria-live=polite]            under its field; empty (and 0 high) without an error
//     .html-menu-separator
//     button.html-menu-row.html-composer-confirm        verb label + "Enter" in .html-menu-shortcut
// Enter in a field (not while an IME composes), or the confirm row, sends `compose`; Rust validates
// again and answers the same page with an `error` or `done`. Escape fades the page out (100ms) and
// dismisses it; a click away just hides it (Rust closes the popup). Nothing is created until Rust
// accepts a submit.
import { icon, type IconName } from './icons';
import { sendPopupCompose, sendPopupDismiss, sendPopupSize } from './protocol';
import {
  ADDRESS_LABEL, ADDRESS_PLACEHOLDER, COMPOSER_COPY, COMPOSER_EXIT_MS, firstComposerError, hasAddress,
  trapStep, validComposerPayload, type ComposerError, type ComposerField, type ComposerMessage,
} from './composer-logic';
import type { PageView } from './palette';

interface FieldNode {
  readonly field: ComposerField;
  readonly row: HTMLElement;
  readonly input: HTMLInputElement;
  readonly error: HTMLElement;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className: string, text?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

const reduced = (): boolean => matchMedia('(prefers-reduced-motion: reduce)').matches;

export function createComposerView(root: HTMLElement): PageView {
  let message: ComposerMessage | null = null;
  // True before the first page, and once the page has dismissed itself.
  let closed = true;
  let leaving = false;
  // A submit is with Rust; its verdict is the next message under the same token.
  let sending = false;
  let sendTimer = 0;
  let lastSize = '';
  let fields: FieldNode[] = [];
  let confirm: HTMLButtonElement | null = null;
  const wrap = el('div', 'html-composer');
  wrap.setAttribute('role', 'dialog');
  wrap.setAttribute('aria-modal', 'true');
  wrap.tabIndex = -1;

  const field = (name: ComposerField): FieldNode | undefined => fields.find(node => node.field === name);

  const stops = (): HTMLElement[] => [...fields.map(node => node.input), ...(confirm ? [confirm] : [])];

  const focusStop = (node: HTMLElement | undefined): void => {
    if (!node) return;
    node.focus({ preventScroll: true });
    if (node instanceof HTMLInputElement) node.select();
  };

  // Measures the natural size and reports it; Rust caps, flips and clamps the window. The first
  // report is when the window is shown, so the enter motion starts then.
  const measure = (first: boolean): void => {
    if (!message) return;
    const tok = message.tok;
    requestAnimationFrame(() => {
      if (!message || message.tok !== tok || closed) return;
      root.style.maxHeight = 'none';
      const rect = root.getBoundingClientRect();
      root.style.maxHeight = '100vh';
      const width = Math.max(1, Math.ceil(rect.width));
      const height = Math.max(1, Math.ceil(rect.height));
      const key = `${tok}:${width}x${height}`;
      if (key !== lastSize) {
        lastSize = key;
        sendPopupSize(tok, width, height);
      }
      if (first) {
        root.classList.add('is-entering');
        focusStop(fields[0]?.input);
      }
    });
  };

  const showError = (error: ComposerError | null): void => {
    for (const node of fields) {
      const on = error !== null && node.field === error.field;
      node.input.setAttribute('aria-invalid', String(on));
      if (!on) {
        node.error.textContent = '';
        node.error.classList.remove('is-shown');
        continue;
      }
      node.error.textContent = error.text;
      // Only the text fades in; restart it for a second error on the same field.
      node.error.classList.remove('is-shown');
      void node.error.offsetWidth;
      node.error.classList.add('is-shown');
    }
    if (error) focusStop(field(error.field)?.input);
    measure(false);
  };

  const finish = (): void => {
    if (!message || closed) return;
    closed = true;
    sendPopupDismiss(message.tok);
  };

  // Exit on Escape, or after Rust accepts: opacity out over 100ms (ease-in), then dismiss. Input
  // is not blocked meanwhile; a click away hides the window instantly and never comes here.
  const exit = (): void => {
    if (!message || closed || leaving) return;
    leaving = true;
    if (reduced()) {
      finish();
      return;
    }
    root.classList.remove('is-entering');
    root.classList.add('is-leaving');
    setTimeout(finish, COMPOSER_EXIT_MS);
  };

  const submit = (): void => {
    if (!message || closed || leaving || sending) return;
    const name = field('name')?.input.value ?? '';
    const url = field('url')?.input.value ?? '';
    const error = firstComposerError(message.mode, name, url, message.max);
    if (error) {
      showError(error);
      return;
    }
    // Rust answers with the page under the same token; a dropped message must not lock the form.
    sending = true;
    wrap.setAttribute('aria-busy', 'true');
    const tok = message.tok;
    const sent = sendPopupCompose(tok, name.trim(), hasAddress(message.mode) ? url.trim() : undefined);
    if (!sent) {
      sending = false;
      wrap.removeAttribute('aria-busy');
      return;
    }
    clearTimeout(sendTimer);
    sendTimer = window.setTimeout(() => {
      if (message?.tok === tok) {
        sending = false;
        wrap.removeAttribute('aria-busy');
      }
    }, 4000);
  };

  const fieldRow = (name: ComposerField, glyph: IconName, label: string, placeholder: string, value: string, errorId: string): FieldNode => {
    const row = el('div', 'html-composer-field');
    row.dataset.field = name;
    const lead = el('span', 'html-menu-lead');
    lead.setAttribute('aria-hidden', 'true');
    lead.append(icon(glyph));
    const input = el('input', 'bw-input html-composer-input');
    input.type = 'text';
    input.value = value;
    input.placeholder = placeholder;
    input.spellcheck = false;
    input.autocomplete = 'off';
    input.dir = 'auto';
    if (name === 'url') {
      input.inputMode = 'url';
      input.setAttribute('autocapitalize', 'off');
    }
    input.setAttribute('aria-label', label);
    input.setAttribute('aria-describedby', errorId);
    input.setAttribute('aria-invalid', 'false');
    input.addEventListener('input', () => {
      // Editing a field retires its error (the next submit checks again).
      if (input.getAttribute('aria-invalid') === 'true') {
        const own = field(name);
        if (own) {
          own.error.textContent = '';
          own.error.classList.remove('is-shown');
        }
        input.setAttribute('aria-invalid', 'false');
        measure(false);
      }
    });
    row.append(lead, input);
    const error = el('div', 'html-composer-error');
    error.id = errorId;
    error.setAttribute('aria-live', 'polite');
    return { field: name, row, input, error };
  };

  const build = (msg: ComposerMessage): void => {
    const copy = COMPOSER_COPY[msg.mode];
    root.className = 'html-menu bw-menu-surface';
    root.removeAttribute('role');
    root.removeAttribute('aria-label');
    root.removeAttribute('aria-activedescendant');
    root.tabIndex = -1;
    root.style.minWidth = '';
    delete root.dataset.page;
    delete root.dataset.drill;
    root.dataset.kind = 'composer';
    root.dataset.input = 'key';
    wrap.removeAttribute('aria-busy');
    leaving = false;
    sending = false;
    lastSize = '';

    const heading = el('div', 'html-menu-section', copy.heading);
    heading.id = 'html-composer-heading';
    wrap.setAttribute('aria-labelledby', heading.id);

    const nodes: HTMLElement[] = [heading];
    fields = [];
    const add = (name: ComposerField, glyph: IconName, label: string, placeholder: string, value: string): void => {
      const node = fieldRow(name, glyph, label, placeholder, value, `html-composer-error-${name}`);
      fields.push(node);
      nodes.push(node.row, node.error);
    };
    add('name', copy.nameGlyph, copy.nameLabel, copy.namePlaceholder, msg.name);
    if (hasAddress(msg.mode)) add('url', 'link', ADDRESS_LABEL, ADDRESS_PLACEHOLDER, msg.url ?? '');

    const separator = el('div', 'html-menu-separator');
    separator.setAttribute('role', 'separator');
    nodes.push(separator);

    const row = el('button', 'html-menu-row html-composer-confirm');
    row.type = 'button';
    const lead = el('span', 'html-menu-lead');
    lead.setAttribute('aria-hidden', 'true');
    lead.append(icon(copy.confirmGlyph));
    const shortcut = el('span', 'html-menu-shortcut', 'Enter');
    shortcut.setAttribute('aria-hidden', 'true');
    row.setAttribute('aria-keyshortcuts', 'Enter');
    row.append(lead, el('span', 'html-menu-label', copy.confirm), shortcut);
    row.addEventListener('click', submit);
    confirm = row;
    nodes.push(row);

    wrap.replaceChildren(...nodes);
    root.replaceChildren(wrap);
    focusStop(fields[0]?.input);
    measure(true);
  };

  wrap.addEventListener('keydown', (event) => {
    // Keys stay on this page; the menu view listening on #app never sees them.
    event.stopPropagation();
    if (closed) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      exit();
      return;
    }
    if (event.key === 'Tab') {
      // The page is modal: Tab and Shift+Tab cycle fields and the confirm row.
      event.preventDefault();
      const list = stops();
      const at = list.indexOf(document.activeElement as HTMLElement);
      focusStop(list[trapStep(at, list.length, event.shiftKey)]);
      return;
    }
    if (event.key === 'Enter' && event.target instanceof HTMLInputElement && !event.isComposing && event.keyCode !== 229) {
      event.preventDefault();
      submit();
    }
  });
  for (const type of ['mousedown', 'mouseup'] as const) {
    wrap.addEventListener(type, (event) => event.stopPropagation());
  }
  wrap.addEventListener('contextmenu', (event) => {
    // The page's own text editing is the only context a field needs; keep the browser menu off it.
    event.preventDefault();
    event.stopPropagation();
  });

  return {
    show(raw: unknown): boolean {
      if (!validComposerPayload(raw)) return false;
      const refresh = message !== null && !closed && message.tok === raw.tok && message.mode === raw.mode;
      message = raw;
      if (!refresh) {
        closed = false;
        build(raw);
        return true;
      }
      // Rust's verdict on a submit: the change is made (fade out, then dismiss), or an error.
      sending = false;
      clearTimeout(sendTimer);
      wrap.removeAttribute('aria-busy');
      if (raw.done) exit();
      else if (raw.error) showError(raw.error);
      return true;
    },
  };
}
