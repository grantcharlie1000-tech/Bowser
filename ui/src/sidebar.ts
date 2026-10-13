// Vertical sidebar renderer used by the existing Tabs view/controller.
//
// Structure (all nodes are cached and reused, so focus and hover survive every snapshot):
//   #app.sidebar (role=none)
//     .side-top      workspace switcher row + new-tab row (never scrolls)
//     .side-list     role=tree scroller: group headers, tab rows, pinned separator, empty state
import { send, sendSide, type TabItem, type TabsMsg } from './protocol';
import { setTip } from './tooltip';
import { el, icon, place, type IconName } from './dom';
import { DRAG_ATTR } from './view';
import { beforeId, dragScrollDelta } from './tabs-logic';
import {
  buildSidebarEntries, dropIndexVertical, rovingTarget, scrollIntoViewDelta, verticalSlotShift,
  workspaceIconName,
} from './sidebar-logic';

const ISLAND = [
  'var(--bw-group-0)', 'var(--bw-group-1)', 'var(--bw-group-2)', 'var(--bw-group-3)',
  'var(--bw-group-4)', 'var(--bw-group-5)', 'var(--bw-group-6)', 'var(--bw-group-7)',
] as const;
/** Most rows that animate in at once; a bigger batch (workspace switch, group expand) just appears. */
const MAX_ENTER = 12;
/** Safety net for the leave animation when `animationend` never fires (hidden window). */
const LEAVE_MS = 200;

interface SidebarTabNode {
  item: TabItem;
  readonly root: HTMLDivElement;
  readonly image: HTMLImageElement;
  readonly title: HTMLSpanElement;
  readonly marker: HTMLSpanElement;
  readonly mute: SVGSVGElement;
  readonly close: HTMLButtonElement;
  readonly face: HTMLSpanElement;
  shift: number;
}
interface SidebarGroupNode {
  readonly root: HTMLDivElement;
  readonly name: HTMLSpanElement;
  readonly count: HTMLSpanElement;
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

function colour(index: number): string { return index < 0 ? 'var(--bw-accent)' : ISLAND[index & 7] ?? 'var(--bw-accent)'; }

function setFace(node: SidebarTabNode, uri: string | undefined): void {
  const state = node.item.loading ? 'spin' : uri ? 'img' : 'glyph';
  node.face.dataset.state = state;
  if (uri && state === 'img') { if (node.image.getAttribute('src') !== uri) node.image.src = uri; }
  else node.image.removeAttribute('src');
}

function syncTab(node: SidebarTabNode, item: TabItem, active: boolean, favs: ReadonlyMap<number, string>): void {
  node.item = item;
  const label = item.title || 'New tab';
  node.title.textContent = label;
  node.root.setAttribute('aria-label', label); // no data-tip: the hover card shows the title
  node.root.setAttribute('aria-selected', String(active));
  node.root.setAttribute('aria-level', item.g > 0 ? '2' : '1');
  node.root.dataset.g = String(item.g);
  node.root.classList.toggle('is-active', active);
  node.root.classList.toggle('pinned', item.pinned);
  node.root.classList.toggle('muted', item.muted);
  node.root.classList.toggle('in-group', item.g > 0 && !item.pinned);
  node.mute.classList.toggle('on', item.muted);
  const closeLabel = `Close ${item.title || 'tab'}`;
  setTip(node.close, closeLabel);
  node.close.setAttribute('aria-label', closeLabel);
  setFace(node, item.fav ? favs.get(item.id) : undefined);
  node.marker.classList.toggle('on', item.marker >= 0);
  if (item.marker >= 0) node.marker.style.setProperty('--mc', colour(item.marker));
}

function makeTab(item: TabItem, active: boolean, favs: ReadonlyMap<number, string>): SidebarTabNode {
  const root = el('div', 'side-tab bw-island bw-focus-inset');
  root.setAttribute('role', 'treeitem');
  root.dataset.id = String(item.id);
  root.tabIndex = -1;
  const face = el('span', 'side-face');
  const image = el('img', 'side-fav');
  image.alt = '';
  image.draggable = false;
  const glyph = el('span', 'side-glyph');
  glyph.append(icon('workspace-globe', { size: 12, fine: true }));
  const spin = el('span', 'side-spin');
  face.append(image, glyph, spin);
  const title = el('span', 'side-title');
  title.dir = 'auto';
  const flags = el('span', 'side-flags');
  flags.setAttribute('aria-hidden', 'true');
  const mute = icon('mute', { size: 12, fine: true, className: 'side-mute' });
  const marker = el('span', 'side-marker');
  flags.append(mute, marker);
  const pin = icon('pin', { size: 12, fine: true, className: 'side-pin' });
  const close = el('button', 'side-close bw-focus-inset');
  close.type = 'button';
  close.tabIndex = -1;
  close.append(icon('close', { size: 12, fine: true }));
  root.append(face, title, flags, pin, close);
  const node: SidebarTabNode = { item, root, image, title, marker, mute, close, face, shift: 0 };
  syncTab(node, item, active, favs);
  return node;
}

function makeGroup(key: number): SidebarGroupNode {
  const root = el('div', 'side-group side-group-ball bw-focus-inset');
  root.setAttribute('role', 'treeitem');
  root.setAttribute('aria-level', '1');
  root.dataset.key = String(key);
  root.tabIndex = -1;
  const dot = el('span', 'side-group-dot');
  dot.setAttribute('aria-hidden', 'true');
  const name = el('span', 'side-group-name');
  name.dir = 'auto';
  const count = el('span', 'side-group-count');
  const chevron = icon('chevron-down', { size: 12, fine: true, className: 'side-group-chevron' });
  root.append(dot, name, count, chevron);
  return { root, name, count };
}

/** Binds vertical row interaction and renders snapshots from the same Tabs controller data. */
export function createSidebarRenderer(root: HTMLElement): {
  render(msg: TabsMsg, favs: ReadonlyMap<number, string>, workspace: SidebarWorkspace): void;
  workspace(value: SidebarWorkspace): void;
} {
  const tabs = new Map<number, SidebarTabNode>();
  const groups = new Map<number, SidebarGroupNode>();
  let latest: TabsMsg | null = null;
  let latestFavs: ReadonlyMap<number, string> = new Map();
  let currentWorkspace: SidebarWorkspace = { name: 'Workspace', icon: 'g:briefcase' };
  let drag: SidebarDrag | null = null;
  let pendingRender: { readonly msg: TabsMsg; readonly favs: ReadonlyMap<number, string>; readonly workspace: SidebarWorkspace } | null = null;
  let ignoreClick = false;
  let rendered = false;

  // Fixed top region: workspace switcher and new-tab row.
  const header = el('button', 'side-workspace bw-focus-inset');
  header.type = 'button';
  const workspaceIcon = el('span', 'side-workspace-icon');
  workspaceIcon.setAttribute('aria-hidden', 'true');
  const workspaceName = el('span', 'side-workspace-name');
  header.append(workspaceIcon, workspaceName, icon('chevron-down', { size: 12, fine: true, className: 'side-workspace-chevron' }));
  const add = el('button', 'side-newtab bw-focus-inset');
  add.type = 'button';
  setTip(add, 'New tab');
  const addLabel = el('span', 'side-newtab-label');
  addLabel.textContent = 'New tab';
  add.append(icon('plus', { size: 16 }), addLabel);
  // Collapse toggle: the host switches the sidebar between its persisted width and 48px (icons only).
  const collapse = el('button', 'side-collapse bw-focus-inset');
  collapse.type = 'button';
  collapse.append(icon('sidebar', { size: 16 }));
  const head = el('div', 'side-head');
  head.append(header, collapse);
  const top = el('div', 'side-top');
  top.setAttribute(DRAG_ATTR, '');
  top.append(head, add);
  const pinGrid = el('div', 'side-pinned');
  pinGrid.setAttribute('role', 'none');
  const COLLAPSED_MAX_PX = 72;
  const syncCollapsed = (): void => {
    const narrow = root.clientWidth > 0 && root.clientWidth <= COLLAPSED_MAX_PX;
    if (root.classList.contains('is-collapsed') !== narrow) root.classList.toggle('is-collapsed', narrow);
    const text = narrow ? 'Expand sidebar' : 'Collapse sidebar';
    if (collapse.getAttribute('aria-label') !== text) {
      collapse.setAttribute('aria-label', text);
      collapse.setAttribute('aria-expanded', String(!narrow));
      setTip(collapse, text);
    }
  };
  new ResizeObserver(syncCollapsed).observe(root);
  syncCollapsed();

  const list = el('div', 'side-list');
  list.setAttribute('role', 'tree');
  list.setAttribute('aria-label', 'Tabs');
  list.setAttribute('aria-orientation', 'vertical');
  list.setAttribute(DRAG_ATTR, '');
  const separator = el('div', 'side-sep');
  separator.setAttribute('role', 'separator');
  const empty = el('div', 'side-empty');
  empty.setAttribute('role', 'none');
  const emptyLabel = el('span', 'side-empty-label');
  emptyLabel.textContent = 'No tabs open';
  empty.append(icon('app-tile', { size: 20 }), emptyLabel);

  const applyRoot = (): void => {
    root.classList.remove('strip');
    root.classList.add('sidebar');
    root.setAttribute('role', 'none');
    root.removeAttribute('aria-label');
    root.removeAttribute('aria-orientation');
    root.setAttribute(DRAG_ATTR, '');
  };
  applyRoot();
  // tabs.ts owns the root's horizontal semantics; hand them back when the layout flips.
  new MutationObserver(() => {
    if (root.classList.contains('strip') && root.getAttribute('role') === 'none') {
      root.setAttribute('role', 'tablist');
      root.setAttribute('aria-label', 'Tabs');
      root.setAttribute(DRAG_ATTR, '');
    } else if (root.classList.contains('sidebar') && root.getAttribute('role') !== 'none') {
      applyRoot();
    }
  }).observe(root, { attributes: true, attributeFilter: ['class'] });

  const rowsOf = (): HTMLElement[] => Array.from(list.querySelectorAll<HTMLElement>('.side-tab:not(.leaving), .side-group'));
  const tabRowsOf = (): HTMLElement[] => Array.from(list.querySelectorAll<HTMLElement>('.side-tab:not(.leaving):not(.pinned)'));

  const updateFades = (): void => {
    list.classList.toggle('can-up', list.scrollTop > 1);
    list.classList.toggle('can-down', list.scrollTop + list.clientHeight < list.scrollHeight - 1);
  };
  list.addEventListener('scroll', updateFades, { passive: true });

  const reveal = (row: HTMLElement | undefined): void => {
    if (!row) return;
    const delta = scrollIntoViewDelta(list.scrollTop, list.clientHeight, row.offsetTop, row.offsetHeight);
    if (delta !== 0) list.scrollTop += delta;
  };

  const settle = (node: HTMLElement): void => {
    node.classList.remove('bw-enter');
  };
  list.addEventListener('animationend', (event) => {
    if (!(event.target instanceof HTMLElement)) return;
    if (event.target.classList.contains('leaving')) event.target.remove();
    else settle(event.target);
  });

  const render = (msg: TabsMsg, favs: ReadonlyMap<number, string>, workspace: SidebarWorkspace): void => {
    latest = msg;
    latestFavs = favs;
    currentWorkspace = workspace;
    if (drag?.active) {
      pendingRender = { msg, favs, workspace };
      return;
    }
    applyRoot();
    workspaceName.textContent = workspace.name;
    workspaceIcon.replaceChildren(icon(workspaceIconName(workspace.icon) as IconName, { size: 14 }));
    setTip(header, `${workspace.name} - workspace menu`);
    header.setAttribute('aria-label', `${workspace.name} workspace menu`);

    const rows: HTMLElement[] = [];
    const live = new Set(msg.items.map((item) => item.id));
    for (const id of tabs.keys()) if (!live.has(id)) tabs.delete(id);
    const entries = buildSidebarEntries(msg.items, msg.active);
    const keys = new Set<number>();
    const groupColour = new Map<number, string>();
    for (const entry of entries) if (entry.kind === 'group') groupColour.set(entry.key, colour(entry.colour));
    // Pinned tabs are a grid of 32px tiles above the list rows, not rows of their own.
    const pinned: HTMLElement[] = [];
    entries.forEach((entry) => {
      if (entry.kind === 'group') {
        keys.add(entry.key);
        let node = groups.get(entry.key);
        if (!node) {
          node = makeGroup(entry.key);
          groups.set(entry.key, node);
        }
        node.root.style.setProperty('--gc', colour(entry.colour));
        node.root.setAttribute('aria-expanded', String(!entry.collapsed));
        node.root.setAttribute('aria-label', `${entry.name}, ${entry.count} ${entry.count === 1 ? 'tab' : 'tabs'}`);
        setTip(node.root, entry.name);
        node.name.textContent = entry.name;
        node.count.textContent = String(entry.count);
        rows.push(node.root);
      } else {
        const item = entry.item;
        const active = item.id === msg.active;
        let node = tabs.get(item.id);
        if (!node) {
          node = makeTab(item, active, favs);
          tabs.set(item.id, node);
        } else {
          syncTab(node, item, active, favs);
        }
        const gc = item.g > 0 && !item.pinned ? groupColour.get(item.g) : undefined;
        if (gc) node.root.style.setProperty('--gc', gc);
        else node.root.style.removeProperty('--gc');
        if (item.pinned) pinned.push(node.root);
        else rows.push(node.root);
      }
    });
    if (pinned.length > 0) {
      place(pinGrid, pinned);
      rows.unshift(pinGrid);
      if (rows.length > 1) rows.splice(1, 0, separator);
    }
    for (const key of groups.keys()) if (!keys.has(key)) groups.delete(key);
    if (rows.length === 0) rows.push(empty);

    // Rows that are about to leave the list fade out in place for 83ms instead of vanishing.
    const kept = new Set<Node>(rows);
    const leaving: HTMLElement[] = [];
    for (const child of Array.from(list.children)) {
      if (!(child instanceof HTMLElement) || kept.has(child)) continue;
      if (!rendered || !child.classList.contains('side-tab') && !child.classList.contains('side-group')) continue;
      if (child.classList.contains('leaving') || !child.isConnected) continue;
      leaving.push(child);
    }
    if (leaving.length > 0 && leaving.length <= MAX_ENTER * 2 && !matchMedia('(prefers-reduced-motion: reduce)').matches) {
      for (const node of leaving) {
        const prev = node.previousElementSibling;
        const at = prev ? rows.indexOf(prev as HTMLElement) : -1;
        let insertAt = at + 1;
        while (leaving.includes(rows[insertAt] as HTMLElement)) insertAt += 1;
        rows.splice(insertAt, 0, node);
        node.classList.remove('bw-enter');
        node.classList.add('leaving');
        window.setTimeout(() => { if (node.classList.contains('leaving')) node.remove(); }, LEAVE_MS);
      }
    }
    const entering = rows.filter((row) => row.parentNode !== list && row !== separator && row !== empty && row !== pinGrid && !row.classList.contains('leaving'));
    for (const row of rows) if (!row.classList.contains('leaving')) row.classList.remove('leaving');
    // Roving tabindex: keep it where the user is, else on the active tab.
    const focused = list.contains(document.activeElement) ? (document.activeElement as HTMLElement).closest<HTMLElement>('.side-tab, .side-group') : null;
    place(list, rows);
    place(root, [top, list]);
    const activeRow = tabs.get(msg.active)?.root;
    const all = rowsOf();
    const target = focused ?? activeRow ?? all[0];
    for (const row of all) if (row.tabIndex !== (row === target ? 0 : -1)) row.tabIndex = row === target ? 0 : -1;
    if (rendered && entering.length > 0 && entering.length <= MAX_ENTER) {
      for (const row of entering) row.classList.add('bw-enter');
    }
    rendered = true;
    reveal(activeRow);
    updateFades();
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
    list.classList.remove('is-dragging');
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

  const toggleGroup = (row: HTMLElement): void => { send({ v: 1, t: 'group', a: 'toggle', key: Number(row.dataset.key) }); };

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
    if (event.target.closest('.side-collapse')) {
      sendSide('collapse');
      return;
    }
    const group = event.target.closest<HTMLElement>('.side-group');
    if (group) {
      toggleGroup(group);
      return;
    }
    const row = event.target.closest<HTMLElement>('.side-tab');
    if (!row || row.classList.contains('leaving')) return;
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
    if (event.button !== 0 || !(event.target instanceof Element) || event.target.closest('.side-close, .side-group, .side-workspace, .side-newtab')) return;
    const row = event.target.closest<HTMLElement>('.side-tab');
    if (!row || row.classList.contains('pinned')) return; // pinned tiles are a grid: no vertical drag
    const node = tabs.get(Number(row.dataset.id));
    const order = tabRowsOf().map((element) => tabs.get(Number(element.dataset.id))).filter((value): value is SidebarTabNode => value !== undefined);
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
      list.classList.add('is-dragging');
    }
    current.node.root.style.transform = `translateY(${dy}px)`;
    const rect = list.getBoundingClientRect();
    const edge = dragScrollDelta(event.clientY, rect.top, rect.bottom);
    if (edge !== 0) {
      list.scrollTop += edge;
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
    const row = event.target.closest<HTMLElement>('.side-tab, .side-group');
    if (!row || event.target.closest('.side-close')) return;
    const order = rowsOf();
    const at = order.indexOf(row);
    if (at < 0) return;
    const isGroup = row.classList.contains('side-group');
    const expanded = row.getAttribute('aria-expanded') === 'true';
    const owner = !isGroup ? list.querySelector<HTMLElement>(`.side-group[data-key="${row.dataset.g ?? ''}"]`) : null;
    let next = at;
    switch (event.key) {
      case 'ArrowUp': case 'ArrowDown': case 'Home': case 'End':
        next = rovingTarget(event.key, at, order.length);
        break;
      case 'ArrowLeft':
        if (isGroup && expanded) { event.preventDefault(); toggleGroup(row); return; }
        if (owner) next = order.indexOf(owner);
        else return;
        break;
      case 'ArrowRight':
        if (isGroup && !expanded) { event.preventDefault(); toggleGroup(row); return; }
        if (isGroup) next = Math.min(order.length - 1, at + 1);
        else return;
        break;
      case 'Enter':
      case ' ':
        event.preventDefault();
        if (isGroup) toggleGroup(row);
        else send({ v: 1, t: 'tab', a: 'select', id: Number(row.dataset.id) });
        return;
      case 'Delete':
        if (isGroup) return;
        event.preventDefault();
        send({ v: 1, t: 'tab', a: 'close', id: Number(row.dataset.id) });
        return;
      default: return;
    }
    event.preventDefault();
    const target = order[next];
    if (!target) return;
    order.forEach((tab) => { tab.tabIndex = tab === target ? 0 : -1; });
    target.focus();
    reveal(target);
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
