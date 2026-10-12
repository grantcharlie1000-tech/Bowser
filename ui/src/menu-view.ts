import { icon, type IconName } from './dom';
import { isIconName } from './icons';
import {
  activate, back, createNav, currentPage, footerVisible, hasLeadColumn, hasTrailColumn, menuKey, onFill, validMenuPayload,
  type MenuBadge, type MenuMessage, type MenuNav, type MenuOutcome,
} from './menu-logic';
import { sendPopupDismiss, sendPopupPick, sendPopupSize } from './protocol';

// Markup contract (styling lives in menu.css; keep these names stable):
//   #app.html-menu[role=menu][data-kind][data-page][data-input]   the menu, one drill-in page at a time
//   .html-menu-header > .html-menu-avatar, .html-menu-name, .html-menu-caption   main menu card
//   .html-menu-list.bw-scroll                          the scrolling part (header and footer stay pinned)
//   .html-menu-back[role=menuitem]                     back row on a submenu page
//   .html-menu-section                                 non-interactive heading
//   .html-menu-separator[role=separator]
//   .html-menu-row[role=menuitem|menuitemcheckbox][data-index][data-id][data-icon]
//       aria-disabled, aria-checked, aria-haspopup; .selected marks the keyboard/hover row
//       children: .html-menu-lead (icon | favicon | badge | spacer), .html-menu-label,
//                 .html-menu-shortcut, .html-menu-trail (check or chevron | spacer)
//   .html-menu-footer                                  muted note under the list
// `--menu-accent` / `--menu-on-accent` on the header and badges carry the accent and its text colour.

export interface MenuView {
  /** Shows a `menu` message. False (and nothing changes) when it is not a valid menu payload. */
  show(raw: unknown): boolean;
}

function disc(className: string, badge: MenuBadge): HTMLElement {
  const node = document.createElement('span');
  node.className = className;
  node.setAttribute('aria-hidden', 'true');
  node.style.setProperty('--menu-accent', badge.accent);
  node.style.setProperty('--menu-on-accent', onFill(badge.accent));
  node.textContent = badge.avatar;
  return node;
}

/** One menu view per popup page. Listeners attach once; each `menu` message replaces the menu. */
export function createMenuView(root: HTMLElement, now: () => number = () => performance.now()): MenuView {
  let message: MenuMessage | null = null;
  let nav: MenuNav | null = null;
  // Set once a pick or dismissal is sent, so a double click or key repeat sends nothing more.
  let closed = true;
  // Which way the page last changed (+1 deeper, -1 back, 0 first show), for the drill-in slide.
  let drill: -1 | 0 | 1 = 0;

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
      case 'page': drill = nav.pages.length > Number(root.dataset.page ?? 0) + 1 ? 1 : -1; draw(); break;
      case 'pick': closed = true; sendPopupPick(message.tok, outcome.id); break;
      case 'dismiss': closed = true; sendPopupDismiss(message.tok); break;
      case 'none': break;
    }
  };

  // Measures the natural size and reports it; Rust caps it to the monitor work area and flips or
  // clamps the window, and `.html-menu-list` scrolls inside whatever height it is given.
  const measure = (first: boolean): void => {
    if (!message) return;
    const tok = message.tok;
    requestAnimationFrame(() => {
      if (!message || message.tok !== tok) return;
      root.style.maxHeight = 'none';
      const rect = root.getBoundingClientRect();
      root.style.maxHeight = '100vh';
      sendPopupSize(tok, Math.max(1, Math.ceil(rect.width)), Math.max(1, Math.ceil(rect.height)));
      // The enter motion starts when the size is reported, which is when the window is shown.
      if (first) root.classList.add('is-entering');
    });
  };

  const draw = (first = false): void => {
    if (!message || !nav) return;
    const page = currentPage(nav);
    const top = nav.pages.length === 1;
    const kind = message.kind;
    const nodes: HTMLElement[] = [];
    root.dataset.page = String(nav.pages.length - 1);
    root.setAttribute('aria-label', top ? (message.footer || kind) : page.label);
    const lead = hasLeadColumn(page.entries);
    const trail = hasTrailColumn(page.entries);

    if (top && message.header) {
      const header = document.createElement('div');
      header.className = 'html-menu-header';
      header.setAttribute('aria-hidden', 'true');
      const avatar = disc('html-menu-avatar', { avatar: message.header.avatar, accent: message.header.accent });
      const name = document.createElement('span');
      name.className = 'html-menu-name';
      name.textContent = message.header.name;
      const caption = document.createElement('span');
      caption.className = 'html-menu-caption';
      caption.textContent = message.header.caption;
      header.append(avatar, name, caption);
      nodes.push(header);
    }

    const list: HTMLElement[] = [];
    if (!top) {
      const backRow = document.createElement('button');
      backRow.type = 'button';
      backRow.tabIndex = -1;
      backRow.className = 'html-menu-back';
      backRow.setAttribute('role', 'menuitem');
      backRow.setAttribute('aria-label', `Back from ${page.label}`);
      const chevron = icon('chevron-left');
      chevron.setAttribute('aria-hidden', 'true');
      const text = document.createElement('span');
      text.className = 'html-menu-label';
      text.textContent = page.label;
      text.title = page.label;
      backRow.append(chevron, text);
      backRow.addEventListener('mouseup', (event) => {
        if (event.button === 0 && nav && back(nav)) finish({ kind: 'page' });
      });
      list.push(backRow);
    }
    page.entries.forEach((entry, index) => {
      if ('section' in entry) {
        const heading = document.createElement('div');
        heading.className = 'html-menu-section';
        heading.setAttribute('role', 'presentation');
        heading.textContent = entry.section;
        list.push(heading);
        return;
      }
      if ('separator' in entry) {
        const separator = document.createElement('div');
        separator.className = 'html-menu-separator';
        separator.setAttribute('role', 'separator');
        list.push(separator);
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

      if (lead) {
        // Icon, favicon or badge; rows without one keep the column so labels line up.
        const slot = document.createElement('span');
        slot.className = 'html-menu-lead';
        slot.setAttribute('aria-hidden', 'true');
        const favUri = entry.fav !== undefined ? message?.favs?.[entry.fav] : undefined;
        if (favUri) {
          const img = document.createElement('img');
          img.className = 'html-menu-favicon';
          img.alt = '';
          img.width = 16;
          img.height = 16;
          img.src = favUri;
          slot.append(img);
        } else if (entry.badge) {
          slot.append(disc('html-menu-badge', entry.badge));
        } else if (entry.icon && isIconName(entry.icon)) {
          slot.append(icon(entry.icon as IconName));
        }
        row.append(slot);
      }
      const label = document.createElement('span');
      label.className = 'html-menu-label';
      label.textContent = entry.label;
      label.title = entry.label;
      row.append(label);
      if (entry.shortcut) {
        const shortcut = document.createElement('span');
        shortcut.className = 'html-menu-shortcut';
        shortcut.setAttribute('aria-hidden', 'true');
        shortcut.textContent = entry.shortcut;
        row.append(shortcut);
      }
      if (trail) {
        const slot = document.createElement('span');
        slot.className = 'html-menu-trail';
        slot.setAttribute('aria-hidden', 'true');
        if (entry.checked) slot.append(icon('check'));
        else if (entry.submenu) slot.append(icon('chevron-right'));
        row.append(slot);
      }
      row.addEventListener('mousemove', () => {
        root.dataset.input = 'pointer';
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
      list.push(row);
    });
    const scroller = document.createElement('div');
    scroller.className = 'html-menu-list bw-scroll';
    scroller.setAttribute('role', 'presentation');
    scroller.append(...list);
    nodes.push(scroller);

    if (message.footer && footerVisible(kind)) {
      const footer = document.createElement('div');
      footer.className = 'html-menu-footer';
      footer.setAttribute('aria-hidden', 'true');
      footer.textContent = message.footer;
      nodes.push(footer);
    }
    root.replaceChildren(...nodes);
    // Drill-in slide: the new page's list moves 8px in from the side it came from.
    root.dataset.drill = String(drill);
    scroller.classList.remove('is-drill');
    if (drill !== 0) { void scroller.offsetWidth; scroller.classList.add('is-drill'); }
    markSelected();
    measure(first);
  };

  root.addEventListener('keydown', (event: KeyboardEvent) => {
    if (!nav || closed) return;
    root.dataset.input = 'key';
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
      drill = 0;
      root.className = 'html-menu bw-menu-surface';
      root.tabIndex = -1;
      root.setAttribute('role', 'menu');
      root.dataset.kind = raw.kind;
      root.dataset.input = 'pointer';
      draw(true);
      return true;
    },
  };
}
