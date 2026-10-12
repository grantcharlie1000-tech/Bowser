// Vertical sidebar renderer used by the existing Tabs view/controller.
import { send, sendSide, type TabItem, type TabsMsg } from './protocol';
import { setTip } from './tooltip';
import { el, icon, place } from './dom';
import { DRAG_ATTR } from './view';
import { beforeId, dragScrollDelta } from './tabs-logic';
import { buildSidebarEntries, dropIndexVertical, scrollIntoViewDelta, verticalSlotShift } from './sidebar-logic';

const ISLAND = [
  'var(--bw-group-0)', 'var(--bw-group-1)', 'var(--bw-group-2)', 'var(--bw-group-3)',
  'var(--bw-group-4)', 'var(--bw-group-5)', 'var(--bw-group-6)', 'var(--bw-group-7)',
] as const;
const GLYPHS: Readonly<Record<string, string>> = {
  'g:briefcase': '▣', 'g:home': '⌂', 'g:code': '‹›', 'g:book': '▤',
  'g:cart': '🛒', 'g:game': '⌗', 'g:music': '♫', 'g:heart': '♡',
  'g:star': '☆', 'g:bolt': 'ϟ', 'g:globe': '◎', 'g:leaf': '❧',
};

interface SidebarTabNode {
  item: TabItem;
  readonly root: HTMLDivElement;
  readonly image: HTMLImageElement;
  readonly title: HTMLSpanElement;
  readonly marker: HTMLSpanElement;
  readonly close: HTMLButtonElement;
  shift: number;
}
interface SidebarDrag {
  readonly pointerId: number;
  readonly startY: number;
  readonly node: SidebarTabNode;
  readonly order: readonly SidebarTabNode[];
  centers: readonly number[];
  readonly from: number;
  active: boolean;
  destination: number;
}
export interface SidebarWorkspace { readonly name: string; readonly icon: string }

function colour(index: number): string { return index < 0 ? 'var(--focus-border)' : ISLAND[index & 7] ?? 'var(--focus-border)'; }

function makeTab(item: TabItem, active: boolean, favs: ReadonlyMap<number, string>): SidebarTabNode {
  const root = el('div', 'side-tab');
  root.setAttribute('role', 'tab');
  root.dataset.id = String(item.id);
  root.tabIndex = active ? 0 : -1;
  root.classList.toggle('active', active);
  root.classList.toggle('pinned', item.pinned);
  root.classList.toggle('muted', item.muted);
  root.setAttribute('aria-selected', String(active));
  const title = el('span', 'side-title');
  title.dir = 'auto';
  title.textContent = item.title || 'New tab';
  setTip(root, item.title || 'New tab');
  const iconBox = el('span', 'side-tab-icon');
  const image = el('img', 'side-fav');
  image.alt = '';
  image.draggable = false;
  const glyph = el('span', 'side-glyph');
  glyph.textContent = '＋';
  const spin = el('span', 'side-spin');
  const uri = item.fav ? favs.get(item.id) : undefined;
  if (item.loading) spin.hidden = false;
  else if (uri) image.src = uri;
  else glyph.hidden = false;
  image.hidden = !uri || item.loading;
  spin.hidden = !item.loading;
  glyph.hidden = item.loading || Boolean(uri);
  iconBox.append(image, glyph, spin);
  const marker = el('span', 'side-marker');
  marker.setAttribute('aria-hidden', 'true');
  if (item.marker >= 0) {
    marker.classList.add('on');
    marker.style.setProperty('--mc', colour(item.marker));
  }
  const close = el('button', 'side-close');
  close.type = 'button';
  close.tabIndex = -1;
  setTip(close, `Close ${item.title || 'tab'}`);
  close.setAttribute('aria-label', `Close ${item.title || 'tab'}`);
  close.append(icon('close'));
  root.append(iconBox, title, marker, close);
  return { item, root, image, title, marker, close, shift: 0 };
}

/** Binds vertical row interaction and renders snapshots from the same Tabs controller data. */
export function createSidebarRenderer(root: HTMLElement): {
  render(msg: TabsMsg, favs: ReadonlyMap<number, string>, workspace: SidebarWorkspace): void;
  workspace(value: SidebarWorkspace): void;
} {
  const tabs = new Map<number, SidebarTabNode>();
  let latest: TabsMsg | null = null;
  let latestFavs: ReadonlyMap<number, string> = new Map();
  let currentWorkspace: SidebarWorkspace = { name: 'Workspace', icon: 'g:briefcase' };
  let drag: SidebarDrag | null = null;
  let pendingRender: { readonly msg: TabsMsg; readonly favs: ReadonlyMap<number, string>; readonly workspace: SidebarWorkspace } | null = null;
  let ignoreClick = false;
  root.classList.remove('strip');
  root.classList.add('sidebar');
  root.setAttribute('role', 'tablist');
  root.setAttribute('aria-label', 'Tabs');
  root.setAttribute('aria-orientation', 'vertical');
  root.setAttribute(DRAG_ATTR, '');

  const scrollActiveIntoView = (): void => {
    const active = tabs.get(latest?.active ?? 0)?.root;
    if (!active) return;
    const delta = scrollIntoViewDelta(root.scrollTop, root.clientHeight, active.offsetTop, active.offsetHeight);
    if (delta !== 0) root.scrollTop += delta;
  };

  const render = (msg: TabsMsg, favs: ReadonlyMap<number, string>, workspace: SidebarWorkspace): void => {
    latest = msg;
    latestFavs = favs;
    currentWorkspace = workspace;
    if (drag?.active) {
      pendingRender = { msg, favs, workspace };
      return;
    }
    const header = el('button', 'side-workspace');
    header.type = 'button';
    setTip(header, `${workspace.name} — workspace menu`);
    header.setAttribute('aria-label', `${workspace.name} workspace menu`);
    const workspaceIcon = el('span', 'side-workspace-icon');
    workspaceIcon.textContent = GLYPHS[workspace.icon] ?? '▣';
    workspaceIcon.setAttribute('aria-hidden', 'true');
    const workspaceName = el('span', 'side-workspace-name');
    workspaceName.textContent = workspace.name;
    header.append(workspaceIcon, workspaceName);

    const add = el('button', 'side-newtab');
    add.type = 'button';
    add.append(icon('plus'), document.createTextNode('New tab'));
    setTip(add, 'New tab');

    const rows: HTMLElement[] = [header, add];
    const live = new Set(msg.items.map((item) => item.id));
    for (const id of tabs.keys()) if (!live.has(id)) tabs.delete(id);
    const entries = buildSidebarEntries(msg.items, msg.active);
    for (const entry of entries) {
      if (entry.kind === 'group') {
        const group = el('div', 'side-group');
        group.dataset.key = String(entry.key);
        group.style.setProperty('--gc', colour(entry.colour));
        const ball = el('button', 'side-group-ball');
        ball.type = 'button';
        ball.dataset.key = String(entry.key);
        setTip(ball, entry.name);
        ball.setAttribute('aria-label', `${entry.name}, ${entry.count} tabs`);
        ball.setAttribute('aria-expanded', String(!entry.collapsed));
        const label = el('span', 'side-group-name');
        label.textContent = entry.name;
        const count = el('span', 'side-group-count');
        count.textContent = String(entry.count);
        const chevron = el('span', 'side-group-chevron');
        chevron.textContent = entry.collapsed ? '›' : '⌄';
        group.append(ball, label, count, chevron);
        rows.push(group);
      } else {
        const item = entry.item;
        let node = tabs.get(item.id);
        if (!node) {
          node = makeTab(item, item.id === msg.active, favs);
          tabs.set(item.id, node);
        } else {
          node.item = item;
          node.title.textContent = item.title || 'New tab';
          setTip(node.root, item.title || 'New tab');
          node.root.setAttribute('aria-selected', String(item.id === msg.active));
          node.root.classList.toggle('active', item.id === msg.active);
          node.root.classList.toggle('pinned', item.pinned);
          node.root.classList.toggle('muted', item.muted);
          node.root.tabIndex = item.id === msg.active ? 0 : -1;
          setTip(node.close, `Close ${item.title || 'tab'}`);
          node.close.setAttribute('aria-label', `Close ${item.title || 'tab'}`);
          const uri = item.fav ? favs.get(item.id) : undefined;
          node.image.hidden = !uri || item.loading;
          if (uri) node.image.src = uri;
          else node.image.removeAttribute('src');
          const glyph = node.root.querySelector<HTMLElement>('.side-glyph');
          const spin = node.root.querySelector<HTMLElement>('.side-spin');
          if (glyph) glyph.hidden = item.loading || Boolean(uri);
          if (spin) spin.hidden = !item.loading;
          node.marker.classList.toggle('on', item.marker >= 0);
          if (item.marker >= 0) node.marker.style.setProperty('--mc', colour(item.marker));
        }
        rows.push(node.root);
      }
    }
    place(root, rows);
    if (!drag?.active) scrollActiveIntoView();
  };

  const endDrag = (commit: boolean): void => {
    const current = drag;
    if (!current) return;
    drag = null;
    try { if (current.node.root.hasPointerCapture(current.pointerId)) current.node.root.releasePointerCapture(current.pointerId); } catch { /* capture may have ended */ }
    if (!current.active) return;
    ignoreClick = true;
    current.node.root.classList.remove('dragging');
    current.node.root.style.transform = '';
    for (const node of current.order) {
      node.shift = 0;
      node.root.style.transform = '';
    }
    if (commit && current.destination !== current.from) {
      const others = current.order.filter((node) => node !== current.node).map((node) => node.item.id);
      send({ v: 1, t: 'tabmove', id: current.node.item.id, before: beforeId(others, current.destination) });
    }
    if (pendingRender) {
      const pending = pendingRender;
      pendingRender = null;
      render(pending.msg, pending.favs, pending.workspace);
    }
  };

  root.addEventListener('click', (event) => {
    if (!(event.target instanceof Element)) return;
    if (event.target.closest('.side-workspace')) {
      sendSide('workspace');
      return;
    }
    if (event.target.closest('.side-newtab')) {
      sendSide('newtab');
      return;
    }
    const ball = event.target.closest<HTMLElement>('.side-group-ball');
    if (ball) {
      send({ v: 1, t: 'group', a: 'toggle', key: Number(ball.dataset.key) });
      return;
    }
    const row = event.target.closest<HTMLElement>('.side-tab');
    if (!row) return;
    if (ignoreClick) { ignoreClick = false; return; }
    const a = event.target.closest('.side-close') ? 'close' : 'select';
    send({ v: 1, t: 'tab', a, id: Number(row.dataset.id) });
  });
  root.addEventListener('contextmenu', (event) => {
    if (!(event.target instanceof Element)) return;
    const group = event.target.closest<HTMLElement>('.side-group');
    if (group) {
      event.preventDefault();
      send({ v: 1, t: 'group', a: 'menu', key: Number(group.dataset.key) });
      return;
    }
    const row = event.target.closest<HTMLElement>('.side-tab');
    if (row) {
      event.preventDefault();
      send({ v: 1, t: 'tab', a: 'menu', id: Number(row.dataset.id) });
    }
  });
  root.addEventListener('pointerdown', (event) => {
    ignoreClick = false;
    if (event.button !== 0 || !(event.target instanceof Element) || event.target.closest('.side-close, .side-group-ball, .side-workspace, .side-newtab')) return;
    const row = event.target.closest<HTMLElement>('.side-tab');
    if (!row) return;
    const node = tabs.get(Number(row.dataset.id));
    const order = Array.from(root.querySelectorAll<HTMLElement>('.side-tab')).map((element) => tabs.get(Number(element.dataset.id))).filter((value): value is SidebarTabNode => value !== undefined);
    if (!node) return;
    const from = order.indexOf(node);
    if (from < 0) return;
    const centers = order.map((tab) => { const rect = tab.root.getBoundingClientRect(); return rect.top + rect.height / 2; });
    drag = { pointerId: event.pointerId, startY: event.clientY, node, order, centers, from, active: false, destination: from };
    try { node.root.setPointerCapture(event.pointerId); } catch { /* window listeners still end the drag */ }
  });
  root.addEventListener('pointermove', (event) => {
    const current = drag;
    if (!current || current.pointerId !== event.pointerId) return;
    const dy = event.clientY - current.startY;
    if (!current.active) {
      if (Math.abs(dy) < 4) return;
      current.active = true;
      current.node.root.classList.add('dragging');
    }
    current.node.root.style.transform = `translateY(${dy}px)`;
    const rect = root.getBoundingClientRect();
    const edge = dragScrollDelta(event.clientY, rect.top, rect.bottom);
    if (edge !== 0) {
      root.scrollTop += edge;
      current.centers = current.order.map((tab) => { const bounds = tab.root.getBoundingClientRect(); return bounds.top + bounds.height / 2; });
    }
    current.destination = dropIndexVertical(current.centers, current.from, (current.centers[current.from] ?? 0) + dy);
    current.order.forEach((tab, index) => {
      if (index === current.from) return;
      const shift = verticalSlotShift(current.centers, index, current.from, current.destination);
      tab.shift = shift;
      tab.root.style.transform = shift === 0 ? '' : `translateY(${shift}px)`;
    });
  });
  root.addEventListener('pointerup', (event) => { if (drag?.pointerId === event.pointerId) endDrag(true); });
  root.addEventListener('pointercancel', (event) => { if (drag?.pointerId === event.pointerId) endDrag(false); });
  root.addEventListener('lostpointercapture', (event) => { if (drag?.pointerId === event.pointerId) endDrag(false); });
  root.addEventListener('keydown', (event) => {
    if (!(event.target instanceof Element)) return;
    const row = event.target.closest<HTMLElement>('.side-tab');
    if (!row) return;
    const order = Array.from(root.querySelectorAll<HTMLElement>('.side-tab'));
    const at = order.indexOf(row);
    if (at < 0) return;
    let next = at;
    switch (event.key) {
      case 'ArrowUp': next = Math.max(0, at - 1); break;
      case 'ArrowDown': next = Math.min(order.length - 1, at + 1); break;
      case 'Home': next = 0; break;
      case 'End': next = order.length - 1; break;
      case 'Enter':
      case ' ': event.preventDefault(); send({ v: 1, t: 'tab', a: 'select', id: Number(row.dataset.id) }); return;
      case 'Delete': event.preventDefault(); send({ v: 1, t: 'tab', a: 'close', id: Number(row.dataset.id) }); return;
      default: return;
    }
    event.preventDefault();
    const target = order[next];
    if (!target) return;
    order.forEach((tab) => { tab.tabIndex = tab === target ? 0 : -1; });
    target.focus();
    const delta = scrollIntoViewDelta(root.scrollTop, root.clientHeight, target.offsetTop, target.offsetHeight);
    if (delta !== 0) root.scrollTop += delta;
  });
  window.addEventListener('pointerup', (event) => { if (drag?.pointerId === event.pointerId) endDrag(true); });
  window.addEventListener('pointercancel', (event) => { if (drag?.pointerId === event.pointerId) endDrag(false); });
  window.addEventListener('blur', () => endDrag(false));

  return {
    render,
    workspace(value) {
      currentWorkspace = value;
      if (latest) render(latest, latestFavs, currentWorkspace);
    },
  };
}
