// The bookmarks bar (?view=bar): a strip of the chosen folder's bookmarks and folders, then tab-group
// chips. Rust sends `{t:"state"}` (theme) and `{t:"snap",p:"bookmarkbar"}`; the page sends
// `{t:"addr",a:"bar..."}` commands. Menus (a folder's items, the overflow list, context menus) are
// Bowser's own popups, opened by Rust under the element, so nothing here overflows its 28px row.
// Text from Rust reaches the DOM through textContent only.
import { onInbound, rectWire, send, sendAddr, type Inbound } from './protocol';
import { applyTheme } from './view';
import { el, icon } from './dom';
import { faviconTile } from './panels/kit';
import {
  contextFields, faceOf, fitCount, folderFields, groupContextFields, groupFields, openFields, overflowFields, parseBarSnapshot,
  type BarGroup, type BarItem, type BarSnapshot,
} from './bookmark-bar-logic';

export function startBookmarkBar(root: HTMLElement): void {
  root.classList.add('bm-root');
  const bar = el('div', 'bm-bar');
  bar.setAttribute('role', 'toolbar');
  bar.setAttribute('aria-label', 'Bookmarks bar');
  const list = el('div', 'bm-list');
  list.setAttribute('role', 'presentation');
  const overflow = el('button', 'bm-item bm-overflow');
  overflow.type = 'button';
  overflow.setAttribute('aria-label', 'More bookmarks');
  overflow.setAttribute('aria-haspopup', 'menu');
  overflow.append(icon('chevron-right', { size: 12, fine: true }));
  overflow.hidden = true;
  bar.append(list, overflow);
  root.replaceChildren(bar);

  let snapshot: BarSnapshot | null = null;
  let nodes: HTMLElement[] = [];
  let firstHidden = -1;

  const send_ = (fields: Parameters<typeof sendAddr>[0]): void => { sendAddr(fields); };

  const itemNode = (item: BarItem, display: BarSnapshot['display']): HTMLElement => {
    const face = faceOf(item, display);
    const button = el('button', `bm-item${item.kind === 'folder' ? ' is-folder' : ''}`);
    button.type = 'button';
    button.dataset.id = String(item.id);
    button.setAttribute('aria-label', item.title !== '' ? item.title : item.url);
    if (item.kind === 'folder') button.setAttribute('aria-haspopup', 'menu');
    if (face.icon === 'folder') button.append(icon('folder', { size: 14, fine: true, className: 'bm-glyph' }));
    else if (face.icon === 'bookmark') button.append(icon('bookmark', { size: 14, fine: true, className: 'bm-glyph' }));
    else if (face.icon === 'site') {
      const tile = faviconTile(item.title !== '' ? item.title : item.url, item.fav !== '' ? item.fav : undefined);
      tile.classList.add('bm-fav');
      button.append(tile);
    }
    if (face.text) {
      const label = el('span', 'bm-label');
      label.textContent = item.title !== '' ? item.title : item.url;
      label.dir = 'auto';
      button.append(label);
    }
    button.addEventListener('click', (event) => {
      if (item.kind === 'folder') {
        const rect = rectWire(button.getBoundingClientRect());
        if (rect !== null) send_(folderFields(item.id, rect));
      } else {
        send_(openFields(item.id, event.ctrlKey || event.shiftKey));
      }
    });
    button.addEventListener('auxclick', (event) => {
      if (event.button === 1 && item.kind === 'link') { event.preventDefault(); send_(openFields(item.id, true)); }
    });
    button.addEventListener('mousedown', (event) => { if (event.button === 1) event.preventDefault(); });
    button.addEventListener('contextmenu', (event) => { event.preventDefault(); send_(contextFields(item.id)); });
    return button;
  };

  const groupNode = (group: BarGroup): HTMLElement => {
    const button = el('button', `bm-item is-group${group.live ? ' is-live' : ''}${group.pinned ? ' is-pinned' : ''}`);
    button.type = 'button';
    button.dataset.group = group.key;
    button.setAttribute('aria-label', `Tab group ${group.name}${group.live ? '' : ' (no open tabs)'}`);
    const dot = el('span', 'bm-dot');
    dot.setAttribute('aria-hidden', 'true');
    if (group.colour >= 0) dot.dataset.colour = String(group.colour);
    const label = el('span', 'bm-label');
    label.textContent = group.name;
    label.dir = 'auto';
    button.append(dot, label);
    button.addEventListener('click', () => send_(groupFields(group.key)));
    button.addEventListener('contextmenu', (event) => { event.preventDefault(); send_(groupContextFields(group.key)); });
    return button;
  };

  /** Hides what does not fit and shows the overflow button for the rest of the bookmarks. */
  const fit = (): void => {
    if (!snapshot) return;
    for (const node of nodes) node.hidden = false;
    overflow.hidden = true;
    firstHidden = -1;
    const bookmarks = snapshot.items.length;
    const widths = nodes.map((node) => node.getBoundingClientRect().width);
    const gap = 2;
    const reserve = 28;
    const available = bar.clientWidth - 16;
    const count = fitCount(widths, available, gap, reserve);
    if (count >= nodes.length) return;
    nodes.forEach((node, index) => { node.hidden = index >= count; });
    overflow.hidden = false;
    // Only bookmarks list under the button; tab groups that did not fit simply go.
    firstHidden = Math.min(count, bookmarks);
    overflow.disabled = firstHidden >= bookmarks;
  };

  const render = (): void => {
    if (!snapshot) return;
    const empty = snapshot.items.length === 0 && snapshot.groups.length === 0;
    nodes = [...snapshot.items.map((item) => itemNode(item, snapshot!.display))];
    const separator = snapshot.items.length > 0 && snapshot.groups.length > 0 ? el('span', 'bm-sep') : null;
    separator?.setAttribute('aria-hidden', 'true');
    const groupNodes = snapshot.groups.map(groupNode);
    const children: HTMLElement[] = [...nodes];
    if (separator) children.push(separator);
    children.push(...groupNodes);
    nodes = [...nodes, ...groupNodes];
    if (empty) {
      const note = el('span', 'bm-empty');
      note.textContent = 'Bookmarks you add to this folder appear here.';
      children.push(note);
    }
    list.replaceChildren(...children);
    fit();
  };

  overflow.addEventListener('click', () => {
    const rect = rectWire(overflow.getBoundingClientRect());
    if (rect !== null && firstHidden >= 0) send_(overflowFields(firstHidden, rect));
  });
  new ResizeObserver(() => fit()).observe(bar);

  onInbound((message: Inbound) => {
    if (message.t === 'state') { applyTheme(message.theme, message.solid, message.dark, message.fontScale, message.uiFont, message.density, message.compactMenus); return; }
    if (message.t !== 'snap' || message.p !== 'bookmarkbar') return;
    const parsed = parseBarSnapshot(message.d);
    if (!parsed) return;
    snapshot = parsed;
    render();
  });
  send({ v: 1, t: 'ready' });
}
