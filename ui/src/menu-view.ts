import { icon, type IconName } from './dom';
import {
  activate, back, createNav, currentPage, menuKey, validMenuPayload,
  type MenuMessage, type MenuNav, type MenuOutcome,
} from './menu-logic';
import { sendPopupDismiss, sendPopupPick, sendPopupSize } from './protocol';

// Markup contract (styling lives in CSS; keep these names stable):
//   #app.html-menu[role=menu][data-kind][data-page]   the menu, one drill-in page at a time
//   .html-menu-header > .html-menu-avatar, .html-menu-name, .html-menu-caption   main menu card
//   .html-menu-back[role=menuitem]                     back row on a submenu page
//   .html-menu-section                                 non-interactive heading
//   .html-menu-separator[role=separator]
//   .html-menu-row[role=menuitem|menuitemcheckbox][data-index][data-id][data-icon]
//       aria-disabled, aria-checked, aria-haspopup; .selected marks the keyboard/hover row
//       children: svg icon, .html-menu-label, .html-menu-shortcut, .html-menu-check, .html-menu-arrow
//   .html-menu-footer                                  muted note under the list
// `--menu-accent` on the header carries the profile accent colour.

const ICONS: Readonly<Record<string, IconName>> = {
  plus: 'plus', lock: 'lock', unlock: 'unlock', warning: 'warning', info: 'info', close: 'close',
  person: 'person', grid: 'grid', key: 'key', shield: 'shield', gear: 'gear', check: 'check',
  mic: 'mic', camera: 'camera', location: 'location', bell: 'bell', cookie: 'cookie', reset: 'reset',
};

export interface MenuView {
  /** Shows a `menu` message. False (and nothing changes) when it is not a valid menu payload. */
  show(raw: unknown): boolean;
}

/** One menu view per popup page. Listeners attach once; each `menu` message replaces the menu. */
export function createMenuView(root: HTMLElement, now: () => number = () => performance.now()): MenuView {
  let message: MenuMessage | null = null;
  let nav: MenuNav | null = null;
  // Set once a pick or dismissal is sent, so a double click or key repeat sends nothing more.
  let closed = true;

  const rows = (): HTMLElement[] => Array.from(root.querySelectorAll<HTMLElement>('.html-menu-row'));

  // Focus stays on the menu root; the selected row is exposed with aria-activedescendant, so no
  // row takes a focus ring and every key reaches one listener.
  const markSelected = (): void => {
    if (!nav) return;
    const selected = currentPage(nav).selected;
    root.removeAttribute('aria-activedescendant');
    for (const row of rows()) {
      const on = Number(row.dataset.index) === selected;
      row.classList.toggle('selected', on);
      if (on) {
        root.setAttribute('aria-activedescendant', row.id);
        row.scrollIntoView({ block: 'nearest' });
      }
    }
    if (document.activeElement !== root) root.focus({ preventScroll: true });
  };

  const finish = (outcome: MenuOutcome): void => {
    if (!message || !nav || closed) return;
    switch (outcome.kind) {
      case 'select': markSelected(); break;
      case 'page': draw(); break;
      case 'pick': closed = true; sendPopupPick(message.tok, outcome.id); break;
      case 'dismiss': closed = true; sendPopupDismiss(message.tok); break;
      case 'none': break;
    }
  };

  const measure = (): void => {
    if (!message) return;
    const tok = message.tok;
    requestAnimationFrame(() => {
      if (!message || message.tok !== tok) return;
      // Measure the natural size; Rust caps it to the monitor work area and the page scrolls inside.
      root.style.maxHeight = 'none';
      const rect = root.getBoundingClientRect();
      root.style.maxHeight = '100vh';
      sendPopupSize(tok, Math.max(1, Math.ceil(rect.width)), Math.max(1, Math.ceil(rect.height)));
    });
  };

  const draw = (): void => {
    if (!message || !nav) return;
    const page = currentPage(nav);
    const top = nav.pages.length === 1;
    const nodes: HTMLElement[] = [];
    root.dataset.page = String(nav.pages.length - 1);
    root.setAttribute('aria-label', top ? (message.footer || message.kind) : page.label);

    if (top && message.header) {
      const header = document.createElement('div');
      header.className = 'html-menu-header';
      header.setAttribute('aria-hidden', 'true');
      header.style.setProperty('--menu-accent', message.header.accent);
      const avatar = document.createElement('span');
      avatar.className = 'html-menu-avatar';
      avatar.textContent = message.header.avatar;
      const name = document.createElement('span');
      name.className = 'html-menu-name';
      name.textContent = message.header.name;
      const caption = document.createElement('span');
      caption.className = 'html-menu-caption';
      caption.textContent = message.header.caption;
      header.append(avatar, name, caption);
      nodes.push(header);
    }
    if (!top) {
      const backRow = document.createElement('button');
      backRow.type = 'button';
      backRow.tabIndex = -1;
      backRow.className = 'html-menu-back';
      backRow.setAttribute('role', 'menuitem');
      backRow.setAttribute('aria-label', `Back from ${page.label}`);
      backRow.textContent = `‹  ${page.label}`;
      backRow.addEventListener('mouseup', (event) => {
        if (event.button === 0 && nav && back(nav)) finish({ kind: 'page' });
      });
      nodes.push(backRow);
    }
    page.entries.forEach((entry, index) => {
      if ('section' in entry) {
        const heading = document.createElement('div');
        heading.className = 'html-menu-section';
        heading.setAttribute('role', 'presentation');
        heading.textContent = entry.section;
        nodes.push(heading);
        return;
      }
      if ('separator' in entry) {
        const separator = document.createElement('div');
        separator.className = 'html-menu-separator';
        separator.setAttribute('role', 'separator');
        nodes.push(separator);
        return;
      }
      const row = document.createElement('button');
      row.type = 'button';
      row.tabIndex = -1;
      row.className = 'html-menu-row';
      row.setAttribute('role', entry.checked ? 'menuitemcheckbox' : 'menuitem');
      if (entry.checked) row.setAttribute('aria-checked', 'true');
      row.disabled = !entry.enabled;
      row.setAttribute('aria-disabled', String(!entry.enabled));
      if (entry.submenu) row.setAttribute('aria-haspopup', 'menu');
      if (entry.shortcut) row.setAttribute('aria-keyshortcuts', entry.shortcut);
      row.id = `html-menu-row-${index}`;
      row.dataset.index = String(index);
      row.dataset.id = String(entry.id);
      if (entry.icon) row.dataset.icon = entry.icon;
      const iconName = entry.icon ? ICONS[entry.icon] : undefined;
      if (iconName) {
        const glyph = icon(iconName);
        glyph.setAttribute('aria-hidden', 'true');
        row.append(glyph);
      }
      const label = document.createElement('span');
      label.className = 'html-menu-label';
      label.textContent = entry.label;
      row.append(label);
      if (entry.shortcut) {
        const shortcut = document.createElement('span');
        shortcut.className = 'html-menu-shortcut';
        shortcut.setAttribute('aria-hidden', 'true');
        shortcut.textContent = entry.shortcut;
        row.append(shortcut);
      }
      if (entry.checked) {
        const check = icon('check');
        check.classList.add('html-menu-check');
        check.setAttribute('aria-hidden', 'true');
        row.append(check);
      }
      if (entry.submenu) {
        const arrow = document.createElement('span');
        arrow.className = 'html-menu-arrow';
        arrow.setAttribute('aria-hidden', 'true');
        arrow.textContent = '›';
        row.append(arrow);
      }
      row.addEventListener('mousemove', () => {
        if (!nav || !entry.enabled || currentPage(nav).selected === index) return;
        currentPage(nav).selected = index;
        markSelected();
      });
      // Mouse up, not click: a press that started outside the menu (the right click that opened it)
      // must not pick, and keyboard Space/Enter are handled on keydown.
      row.addEventListener('mouseup', (event) => {
        if (event.button !== 0 || !nav) return;
        finish(activate(nav, index));
      });
      nodes.push(row);
    });
    if (message.footer) {
      const footer = document.createElement('div');
      footer.className = 'html-menu-footer';
      footer.setAttribute('aria-hidden', 'true');
      footer.textContent = message.footer;
      nodes.push(footer);
    }
    root.replaceChildren(...nodes);
    markSelected();
    measure();
  };

  root.addEventListener('keydown', (event: KeyboardEvent) => {
    if (!nav || closed) return;
    const outcome = menuKey(nav, event.key, { ctrl: event.ctrlKey, alt: event.altKey, meta: event.metaKey, shift: event.shiftKey }, now());
    // Keep every key inside the menu: no page scrolling, no focus leaving, no button activation.
    if (event.key.length === 1 || outcome.kind !== 'none' || ['Tab', 'Enter', ' ', 'Home', 'End', 'PageUp', 'PageDown', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Backspace', 'Escape'].includes(event.key)) {
      event.preventDefault();
    }
    finish(outcome);
  });
  root.addEventListener('contextmenu', (event) => event.preventDefault());
  // Clicks must not move focus off the root (a focused button would draw a focus ring).
  root.addEventListener('mousedown', (event) => event.preventDefault());

  return {
    show(raw: unknown): boolean {
      if (!validMenuPayload(raw)) return false;
      message = raw;
      nav = createNav(raw.entries);
      closed = false;
      root.className = 'html-menu';
      root.tabIndex = -1;
      root.setAttribute('role', 'menu');
      root.dataset.kind = raw.kind;
      draw();
      return true;
    },
  };
}
