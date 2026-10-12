import { icon, type IconName } from './dom';
import { edgeMenu, stepMenu, type MenuEntry, validMenuPayload } from './menu-logic';
import { sendPopupDismiss, sendPopupPick, sendPopupSize } from './protocol';

type MenuPayload = Extract<MenuEntry, { id: number }>;
type MenuMessage = { t: 'menu'; tok: number; kind: string; entries: MenuEntry[] };

const ICONS: Readonly<Record<string, IconName>> = {
  back: 'back', forward: 'forward', reload: 'reload', stop: 'stop', menu: 'menu', split: 'split',
  plus: 'plus', lock: 'lock', unlock: 'unlock', warning: 'warning', info: 'info', close: 'close',
  person: 'person', grid: 'grid', key: 'key', shield: 'shield', gear: 'gear', check: 'check',
  mic: 'mic', camera: 'camera', location: 'location', bell: 'bell', cookie: 'cookie', reset: 'reset',
};

export function renderMenu(root: HTMLElement, raw: unknown): boolean {
  if (!validMenuPayload(raw)) return false;
  const message = raw as MenuMessage;
  root.className = 'html-menu';
  let entries = message.entries;
  const pages: { entries: MenuEntry[]; label: string; selected: number }[] = [];
  let selected = edgeMenu(entries, false);
  root.replaceChildren();
  root.setAttribute('role', 'menu');
  root.setAttribute('aria-label', message.kind || 'Menu');

  const draw = (): void => {
    root.replaceChildren();
    if (pages.length > 0) {
      const back = document.createElement('button');
      back.type = 'button';
      back.className = 'html-menu-back';
      back.setAttribute('role', 'menuitem');
      back.textContent = `‹  ${pages[pages.length - 1]?.label ?? 'Back'}`;
      back.addEventListener('mouseup', (event) => { if (event.button === 0) goBack(); });
      root.append(back);
    }
    entries.forEach((entry, index) => {
      if ('section' in entry) {
        const heading = document.createElement('div');
        heading.className = 'html-menu-section';
        heading.textContent = entry.section;
        root.append(heading);
        return;
      }
      if ('separator' in entry) {
        const separator = document.createElement('div');
        separator.className = 'html-menu-separator';
        separator.setAttribute('role', 'separator');
        root.append(separator);
        return;
      }
      const row = document.createElement('button');
      row.type = 'button';
      row.className = 'html-menu-row';
      row.setAttribute('role', 'menuitem');
      row.disabled = !entry.enabled;
      row.setAttribute('aria-disabled', String(!entry.enabled));
      if (entry.checked) row.setAttribute('aria-checked', 'true');
      if (entry.submenu) row.setAttribute('aria-haspopup', 'menu');
      row.dataset.index = String(index);
      if (index === selected) row.classList.add('selected');
      const iconName = entry.icon ? ICONS[entry.icon] : undefined;
      if (iconName) row.append(icon(iconName));
      const label = document.createElement('span');
      label.className = 'html-menu-label';
      label.textContent = entry.label;
      row.append(label);
      if (entry.shortcut) {
        const shortcut = document.createElement('span');
        shortcut.className = 'html-menu-shortcut';
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
        arrow.textContent = '›';
        row.append(arrow);
      }
      row.addEventListener('mouseenter', () => {
        if (!entry.enabled || selected === index) return;
        selected = index;
        root.querySelectorAll('.html-menu-row').forEach((node) => node.classList.remove('selected'));
        row.classList.add('selected');
      });
      row.addEventListener('mouseup', (event) => {
        if (event.button !== 0 || !entry.enabled) return;
        selected = index;
        if (entry.submenu) openSubmenu(entry);
        else sendPopupPick(message.tok, entry.id);
      });
      root.append(row);
    });
    requestAnimationFrame(() => {
      const rect = root.getBoundingClientRect();
      sendPopupSize(message.tok, Math.ceil(rect.width), Math.ceil(rect.height));
    });
  };

  const openSubmenu = (entry: MenuPayload): void => {
    if (!entry.submenu) return;
    pages.push({ entries, label: entry.label, selected });
    entries = entry.submenu;
    selected = edgeMenu(entries, false);
    draw();
  };
  const goBack = (): void => {
    const page = pages.pop();
    if (!page) { sendPopupDismiss(message.tok); return; }
    entries = page.entries;
    selected = page.selected;
    draw();
  };

  root.onkeydown = (event: KeyboardEvent): void => {
    if (event.key === 'Escape') {
      event.preventDefault();
      sendPopupDismiss(message.tok);
      return;
    }
    if (event.key === 'ArrowLeft' && pages.length) { event.preventDefault(); goBack(); return; }
    if (event.key === 'ArrowRight' || event.key === 'Enter' || event.key === ' ') {
      if (event.key === ' ') event.preventDefault();
      const entry = entries[selected];
      if (entry && !('separator' in entry) && !('section' in entry) && entry.enabled) {
        if (entry.submenu) openSubmenu(entry);
        else if (event.key !== 'ArrowRight') sendPopupPick(message.tok, entry.id);
      }
      return;
    }
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      selected = stepMenu(entries, selected, event.key === 'ArrowDown' ? 1 : -1);
      draw();
      return;
    }
    if (event.key === 'Home' || event.key === 'End') {
      event.preventDefault();
      selected = edgeMenu(entries, event.key === 'End');
      draw();
    }
  };
  draw();
  root.focus();
  return true;
}
