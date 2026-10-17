// Vertical sidebar renderer used by the existing Tabs view/controller.
//
// Structure (all nodes are cached and reused, so focus and hover survive every snapshot):
//   #app.sidebar (role=none)
//     .side-top      workspace switcher row + new-tab row (never scrolls)
//     .side-list     role=tree scroller: group headers, tab rows, pinned separator, empty state
import { send, sendClosedTabs, sendSide, sendTabSelection, type TabItem, type TabOptsMsg, type TabsMsg } from './protocol';
import { setTip } from './tooltip';
import { el, icon, place, type IconName } from './dom';
import { DRAG_ATTR } from './view';
import { beforeId, dragScrollDelta, privateTabName } from './tabs-logic';
// --- tab options ---
import { clickSelection, markedName, notifyLabel, pruneSelection } from './tab-marks-logic';
// --- end tab options ---
import {
  buildSidebarEntries, dropIndexVertical, nearestTile, pinnedBeforeId, rovingTarget, scrollIntoViewDelta, tileShift,
  verticalSlotShift, workspaceIconName, type TilePoint,
} from './sidebar-logic';
// --- split view ---
import { overPageBeside, wirePoint } from './split-logic';
import { validSplitCommand, type SplitCommand } from './split-protocol';
// --- end split view ---
// --- tab ghost (docs/design/tab-drag.md) ---
import { announce, GhostSession } from './tab-drag';
import { DRAG_START_PX, dragAnnouncement, dropOutcome, grabOffset, nextPhase, sidebarBand, type DragPhase } from './tab-drag-logic';
// --- end tab ghost ---

const ISLAND = [
  'var(--bw-group-0)', 'var(--bw-group-1)', 'var(--bw-group-2)', 'var(--bw-group-3)',
  'var(--bw-group-4)', 'var(--bw-group-5)', 'var(--bw-group-6)', 'var(--bw-group-7)',
] as const;
/** Most rows that animate in at once; a bigger batch (workspace switch, group expand) just appears. */
const MAX_ENTER = 12;
/** Safety net for the leave animation when `animationend` never fires (hidden window). */
const LEAVE_MS = 200;

export interface SidebarTabNode {
  item: TabItem;
  readonly root: HTMLDivElement;
  readonly image: HTMLImageElement;
  readonly title: HTMLSpanElement;
  readonly marker: HTMLSpanElement;
  readonly mute: SVGSVGElement;
  readonly close: HTMLButtonElement;
  readonly face: HTMLSpanElement;
  shift: number;
  // --- tab options --- created only while the row needs them.
  dot?: HTMLSpanElement;
  badge?: HTMLSpanElement;
  // --- end tab options ---
}
interface SidebarGroupNode {
  readonly root: HTMLDivElement;
  readonly name: HTMLSpanElement;
  readonly count: HTMLSpanElement;
}
interface SidebarDrag {
  readonly pointerId: number;
  readonly startX: number;
  readonly startY: number;
  readonly node: SidebarTabNode;
  readonly order: readonly SidebarTabNode[];
  centers: readonly number[];
  /** Pinned tiles only: every tile's centre in the grid; the tile moves in both axes. Null for list rows. */
  readonly tiles: readonly TilePoint[] | null;
  readonly from: number;
  active: boolean;
  destination: number;
  // --- tab ghost ---
  readonly pointerType: string;
  /** Home left edge (client px) and size of the held row or tile. */
  readonly homeLeft: number;
  readonly size: readonly [number, number];
  phase: DragPhase;
  ghost: GhostSession | null;
  lastX: number;
  lastY: number;
  // --- end tab ghost ---
}
export interface SidebarWorkspace { readonly name: string; readonly icon: string }

function colour(index: number): string { return index < 0 ? 'var(--bw-accent)' : ISLAND[index & 7] ?? 'var(--bw-accent)'; }

function setFace(node: SidebarTabNode, uri: string | undefined): void {
  const state = node.item.loading ? 'spin' : uri ? 'img' : 'glyph';
  node.face.dataset.state = state;
  if (uri && state === 'img') { if (node.image.getAttribute('src') !== uri) node.image.src = uri; }
  else node.image.removeAttribute('src');
}

/** The marks a row can carry (Settings > Tabs): the unread dot and the notification count live beside the mute and marker flags. */
function sideMark(node: SidebarTabNode, key: 'dot' | 'badge', on: boolean): HTMLSpanElement | undefined {
  const have = node[key];
  if (!on) {
    if (have) { have.remove(); delete node[key]; }
    return undefined;
  }
  if (have) return have;
  const made = el('span', key === 'dot' ? 'side-unread' : 'side-notify');
  node.mute.parentElement?.insertBefore(made, node.mute);
  node[key] = made;
  return made;
}

export function syncTab(node: SidebarTabNode, item: TabItem, active: boolean, favs: ReadonlyMap<number, string>, picked = false): void {
  node.item = item;
  const label = item.title || 'New tab';
  node.title.textContent = label;
  // --- tab options ---
  sideMark(node, 'dot', item.unread === true);
  const badge = sideMark(node, 'badge', item.notify !== undefined);
  if (badge) badge.textContent = notifyLabel(item.notify ?? 0);
  node.root.classList.toggle('asleep', item.asleep === true);
  node.root.classList.toggle('idle', item.idle === true);
  node.root.classList.toggle('picked', picked);
  const marked = markedName(label, item.notify, item.unread, item.rl);
  // --- end tab options ---
  // No data-tip: the hover card shows the title, and a tooltip would flash before it.
  if (item.private) {
    node.root.setAttribute('aria-label', privateTabName(marked));
    node.root.dataset.private = 'true';
  } else {
    if (marked !== label) node.root.setAttribute('aria-label', marked);
    else node.root.removeAttribute('aria-label');
    delete node.root.dataset.private;
  }
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

/** Exported (with syncTab) for test/private.mjs (fake DOM); rows are built by createSidebarRenderer. */
export function makeTab(item: TabItem, active: boolean, favs: ReadonlyMap<number, string>): SidebarTabNode {
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
  // Private rows (sidebar.css): the private glyph replaces the globe tile when there is no favicon;
  // otherwise a glasses badge sits inside the favicon's corner, out of flow, so the grid never changes.
  const privGlyph = el('span', 'side-priv-glyph');
  privGlyph.setAttribute('aria-hidden', 'true');
  privGlyph.append(icon('private'));
  const priv = el('span', 'side-priv');
  priv.setAttribute('aria-hidden', 'true');
  priv.append(icon('glasses', { size: 8, fine: true }));
  face.append(image, glyph, privGlyph, spin, priv);
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
  options(value: TabOptsMsg): void; // --- tab options ---
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
  // --- tab options --- the strip's selection (Ctrl and Shift click) and the Closed tabs row.
  let picked: number[] = [];
  let anchor = 0;
  let selectionOn = true;
  let selectionActive = true; // --- wave-1 fixes ---
  const closedRow = el('button', 'side-closed bw-focus-inset');
  closedRow.type = 'button';
  closedRow.hidden = true;
  setTip(closedRow, 'Closed tabs');
  const closedLabel = el('span', 'side-closed-label');
  closedLabel.textContent = 'Closed tabs';
  closedRow.append(icon('tab-restore', { size: 16 }), closedLabel);
  const setPicked = (ids: readonly number[]): void => {
    if (ids.length === picked.length && ids.every((id, i) => id === picked[i])) return;
    picked = [...ids];
    sendTabSelection(picked);
    for (const [id, node] of tabs) node.root.classList.toggle('picked', picked.includes(id));
  };
  // --- end tab options ---
  // --- split view ---
  // A row or tile dragged out of the sidebar to the right is over the page: Rust shows which half
  // (left, right, top or bottom) it would open in, as for a tab dragged below the horizontal strip.
  let splitOut = false;
  let splitFrame = 0;
  let splitPoint: { x: number; y: number } | null = null;
  const sendSplit = (cmd: SplitCommand): void => {
    if (validSplitCommand(cmd)) send(cmd);
  };
  // --- end split view ---

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
  top.append(head, add, closedRow); // --- tab options ---
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
  const pinTilesOf = (): HTMLElement[] => Array.from(pinGrid.querySelectorAll<HTMLElement>('.side-tab:not(.leaving)'));

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
    // --- tab options --- a closed tab leaves the selection.
    const keptPicked = pruneSelection(picked, live);
    if (keptPicked.length !== picked.length) { picked = keptPicked; sendTabSelection(picked); }
    // --- end tab options ---
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
          syncTab(node, item, active, favs, picked.includes(item.id)); // --- tab options ---
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
    // --- tab ghost --- the host has laid out the released row's slot: the ghost glides there.
    if (ghostLand !== null && ghostLand.session.landing && ghostLand.node.root.isConnected && ghostLand.node.root.style.transform === '') {
      const slot = ghostLand.node.root.getBoundingClientRect();
      ghostLand.session.retarget(slot.left, slot.top);
    }
    // --- end tab ghost ---
  };

  // ----- tab ghost (docs/design/tab-drag.md) -----

  /** The released ghost still gliding to its slot, and the row it stands for. */
  let ghostLand: { readonly session: GhostSession; readonly node: SidebarTabNode } | null = null;

  const rowTitle = (node: SidebarTabNode): string => node.root.getAttribute('aria-label') ?? node.title.textContent ?? '';

  const moveNode = (node: SidebarTabNode, x: number, y: number): void => {
    node.shift = y;
    node.root.style.transform = x === 0 && y === 0 ? '' : x === 0 ? `translateY(${y}px)` : `translate(${x}px, ${y}px)`;
  };

  const ghostHooks = (node: SidebarTabNode): { shown(): void; lost(): void; reveal(): void } => ({
    shown: () => {
      const current = drag;
      if (!current || current.node !== node) return;
      node.root.classList.add('lifted');
      moveNode(node, 0, 0);
      layoutDrag(current);
    },
    lost: () => {
      node.root.classList.remove('lifted');
      const current = drag;
      if (current && current.node === node) layoutDrag(current);
    },
    reveal: () => {
      node.root.classList.remove('lifted');
      if (ghostLand?.node === node) ghostLand = null;
    },
  });

  /** Where the held row's (hidden) slot sits relative to its home: its destination's offset. */
  const slotOffset = (current: SidebarDrag): TilePoint => {
    if (current.tiles !== null) {
      const home = current.tiles[current.from] ?? [0, 0];
      const there = current.tiles[current.destination] ?? home;
      return [there[0] - home[0], there[1] - home[1]];
    }
    return [0, (current.centers[current.destination] ?? 0) - (current.centers[current.from] ?? 0)];
  };

  /**
   * Lays the sidebar out for the held row at the last pointer position: attached, the rows (or
   * tiles) open the gap under the pointer; floating, the gap closes and, right of the sidebar, Rust
   * shows the split drop zone.
   */
  const layoutDrag = (current: SidebarDrag): void => {
    const dx = current.lastX - current.startX;
    const dy = current.lastY - current.startY;
    const ghosted = current.ghost?.live === true;
    const box = root.getBoundingClientRect();
    const phase = nextPhase(current.phase, sidebarBand(box), current.lastX, current.lastY);
    if (phase !== current.phase) {
      current.phase = phase;
      current.ghost?.phase(phase === 'floating');
      announce(dragAnnouncement(phase === 'floating' ? 'float' : 'attach', rowTitle(current.node)));
    }
    // --- split view ---
    // Off the sidebar to the right the tab is over the page: the rows keep their order and Rust shows
    // where the tab would open (one report per frame); back on the sidebar the reorder resumes.
    const out = phase === 'floating' && overPageBeside(current.lastX, box.right);
    if (out !== splitOut) {
      splitOut = out;
      root.classList.toggle('split-out', out);
      if (!out) {
        cancelAnimationFrame(splitFrame);
        splitFrame = 0;
        sendSplit({ v: 1, t: 'split', a: 'end', id: current.node.item.id });
      }
    }
    if (out) {
      splitPoint = wirePoint(current.lastX, current.lastY);
      if (splitFrame === 0) {
        splitFrame = requestAnimationFrame(() => {
          splitFrame = 0;
          if (splitOut && splitPoint && drag) sendSplit({ v: 1, t: 'split', a: 'over', id: drag.node.item.id, x: splitPoint.x, y: splitPoint.y });
        });
      }
    }
    // --- end split view ---
    if (phase === 'floating') {
      current.destination = current.from;
      const last = current.order.length - 1;
      current.order.forEach((tab, index) => {
        if (index === current.from) return;
        if (!ghosted) { moveNode(tab, 0, 0); return; }
        // The ghost is off the sidebar: the rows after it close the gap (tiles reflow in grid order).
        if (current.tiles !== null) {
          const [x, y] = tileShift(current.tiles, index, current.from, last);
          moveNode(tab, x, y);
        } else {
          moveNode(tab, 0, verticalSlotShift(current.centers, index, current.from, last));
        }
      });
      if (!ghosted) current.node.root.style.transform = `translate(${dx}px, ${dy}px)`;
      return;
    }
    if (current.tiles !== null) {
      const tiles = current.tiles;
      const home = tiles[current.from] ?? [0, 0];
      current.destination = nearestTile(tiles, ghosted ? current.lastX : home[0] + dx, ghosted ? current.lastY : home[1] + dy);
      current.order.forEach((tab, index) => {
        if (index === current.from) return;
        const [x, y] = tileShift(tiles, index, current.from, current.destination);
        moveNode(tab, x, y);
      });
      if (ghosted) moveNode(current.node, ...slotOffset(current));
      else current.node.root.style.transform = `translate(${dx}px, ${dy}px)`;
      return;
    }
    if (!ghosted) current.node.root.style.transform = `translateY(${dy}px)`;
    const rect = list.getBoundingClientRect();
    const edge = dragScrollDelta(current.lastY, rect.top, rect.bottom);
    if (edge !== 0) {
      const before = list.scrollTop;
      list.scrollTop += edge;
      // Everything in the list moved by what actually scrolled (shifts and the lift are unaffected).
      const moved = list.scrollTop - before;
      current.centers = current.centers.map((centre) => centre - moved);
    }
    // With a ghost the slot follows the pointer; without one, the row's own centre (as before).
    const probe = ghosted ? current.lastY : (current.centers[current.from] ?? 0) + dy;
    current.destination = dropIndexVertical(current.centers, current.from, probe);
    current.order.forEach((tab, index) => {
      if (index === current.from) return;
      moveNode(tab, 0, verticalSlotShift(current.centers, index, current.from, current.destination));
    });
    if (ghosted) moveNode(current.node, ...slotOffset(current));
  };

  const endDrag = (commit: boolean): void => {
    const current = drag;
    if (!current) return;
    drag = null;
    window.removeEventListener('keydown', onDragKey, true);
    try { if (current.node.root.hasPointerCapture(current.pointerId)) current.node.root.releasePointerCapture(current.pointerId); } catch { /* capture may have ended */ }
    if (!current.active) return;
    ignoreClick = true;
    // --- tab ghost ---
    const ghost = current.ghost;
    const outcome = dropOutcome(commit, current.phase, splitOut, current.from, current.destination);
    const title = rowTitle(current.node);
    announce(outcome === 'reorder'
      ? dragAnnouncement('drop', title, current.destination + 1, current.order.length)
      : outcome === 'settle' ? dragAnnouncement('drop', title) : dragAnnouncement(outcome, title));
    const home = current.tiles !== null
      ? { x: (current.tiles[current.from]?.[0] ?? 0) - current.size[0] / 2, y: (current.tiles[current.from]?.[1] ?? 0) - current.size[1] / 2 }
      : { x: current.homeLeft, y: (current.centers[current.from] ?? 0) - current.size[1] / 2 };
    const [ox, oy] = outcome === 'reorder' ? slotOffset(current) : [0, 0];
    if (ghost) {
      if (ghost.live) ghostLand = { session: ghost, node: current.node };
      if (matchMedia('(prefers-reduced-motion: reduce)').matches) ghost.end('none');
      else if (outcome === 'split') ghost.end('drop', home.x, home.y);
      else ghost.end('settle', home.x + ox, home.y + oy);
    }
    // The hidden copy goes home without a transition, so its slot can be measured when the host re-lays it.
    current.node.root.style.transition = 'none';
    // --- end tab ghost ---
    current.node.root.classList.remove('dragging');
    current.node.root.style.transform = '';
    list.classList.remove('is-dragging');
    for (const node of current.order) {
      node.shift = 0;
      node.root.style.transform = '';
    }
    void current.node.root.offsetWidth; // --- tab ghost ---
    current.node.root.style.transition = ''; // --- tab ghost ---
    // --- split view --- released over the page: Rust opens the split if it showed a drop target.
    if (splitOut) {
      splitOut = false;
      cancelAnimationFrame(splitFrame);
      splitFrame = 0;
      root.classList.remove('split-out');
      sendSplit({ v: 1, t: 'split', a: commit ? 'drop' : 'end', id: current.node.item.id });
    } else // --- end split view ---
    if (commit && current.destination !== current.from) {
      const others = current.order.filter((node) => node !== current.node).map((node) => node.item.id);
      const before = current.tiles !== null
        ? pinnedBeforeId(others, current.destination, Number(tabRowsOf()[0]?.dataset.id ?? 0))
        : beforeId(others, current.destination);
      send({ v: 1, t: 'tabmove', id: current.node.item.id, before });
    }
    if (pendingRender) {
      const pending = pendingRender;
      pendingRender = null;
      render(pending.msg, pending.favs, pending.workspace);
    }
  };

  const onDragKey = (event: KeyboardEvent): void => {
    if (event.key !== 'Escape') return;
    event.preventDefault();
    event.stopPropagation();
    endDrag(false);
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
    // --- tab options ---
    if (event.target.closest('.side-closed')) {
      sendClosedTabs();
      return;
    }
    // --- end tab options ---
    const group = event.target.closest<HTMLElement>('.side-group');
    if (group) {
      toggleGroup(group);
      return;
    }
    const row = event.target.closest<HTMLElement>('.side-tab');
    if (!row || row.classList.contains('leaving')) return;
    if (ignoreClick) { ignoreClick = false; return; }
    const a = event.target.closest('.side-close') ? 'close' : 'select';
    // --- tab options --- Ctrl and Shift click select tabs; a plain click clears the selection.
    if (a === 'select') {
      const id = Number(row.dataset.id);
      const order = Array.from(root.querySelectorAll<HTMLElement>('.side-tab:not(.leaving)')).map((tab) => Number(tab.dataset.id));
      const outcome = selectionOn
        ? clickSelection(picked, order, anchor, latest?.active ?? 0, id, { ctrl: event.ctrlKey || event.metaKey, shift: event.shiftKey }, selectionActive)
        : { selected: [] as number[], anchor: id, activate: true };
      anchor = outcome.anchor;
      setPicked(outcome.selected);
      if (!outcome.activate) return;
    }
    // --- end tab options ---
    send({ v: 1, t: 'tab', a, id: Number(row.dataset.id) });
  });
  // --- tab options --- "Double click on a tab".
  root.addEventListener('dblclick', (event) => {
    if (!(event.target instanceof Element) || event.target.closest('.side-close')) return;
    const row = event.target.closest<HTMLElement>('.side-tab');
    if (row && !row.classList.contains('leaving')) send({ v: 1, t: 'tab', a: 'dblclick', id: Number(row.dataset.id) });
  });
  // --- end tab options ---
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
    if (!row) return;
    // Pinned tiles are a 3-up grid: they reorder among themselves and move in both axes.
    const pinned = row.classList.contains('pinned');
    const node = tabs.get(Number(row.dataset.id));
    const order = (pinned ? pinTilesOf() : tabRowsOf()).map((element) => tabs.get(Number(element.dataset.id))).filter((value): value is SidebarTabNode => value !== undefined);
    if (!node) return;
    const from = order.indexOf(node);
    if (from < 0) return;
    const rects = order.map((tab) => tab.root.getBoundingClientRect());
    const centers = rects.map((rect) => rect.top + rect.height / 2);
    const tiles = pinned ? rects.map((rect): TilePoint => [rect.left + rect.width / 2, rect.top + rect.height / 2]) : null;
    ghostLand?.session.abandon(); // --- tab ghost --- a new press ends the last landing at once
    const own = rects[from];
    drag = {
      pointerId: event.pointerId, startX: event.clientX, startY: event.clientY, node, order, centers, tiles, from, active: false, destination: from,
      pointerType: event.pointerType, homeLeft: own?.left ?? 0, size: [own?.width ?? 0, own?.height ?? 0], phase: 'attached', ghost: null,
      lastX: event.clientX, lastY: event.clientY,
    };
    try { node.root.setPointerCapture(event.pointerId); } catch { /* window listeners still end the drag */ }
  });
  root.addEventListener('pointermove', (event) => {
    const current = drag;
    if (!current || current.pointerId !== event.pointerId) return;
    const dx = event.clientX - current.startX;
    const dy = event.clientY - current.startY;
    if (!current.active) {
      if (Math.abs(dy) < DRAG_START_PX && Math.abs(dx) < DRAG_START_PX) return;
      current.active = true;
      current.node.root.classList.add('dragging');
      list.classList.add('is-dragging');
      window.addEventListener('keydown', onDragKey, true);
      // --- tab ghost --- Rust lifts a native copy of the row (or tile) off the sidebar.
      const rect = current.node.root.getBoundingClientRect();
      const { gx, gy } = grabOffset(current.startX, current.startY, rect);
      const tile = current.tiles !== null;
      current.ghost = GhostSession.start(current.node.item.id, tile ? 'tile' : 'row', rect.width, rect.height, gx, gy, !tile, current.pointerType, ghostHooks(current.node));
      announce(dragAnnouncement('lift', rowTitle(current.node)));
      // --- end tab ghost ---
    }
    current.lastX = event.clientX;
    current.lastY = event.clientY;
    layoutDrag(current);
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
    // --- tab options ---
    options(value) {
      selectionOn = value.selection;
      selectionActive = value.selectionActive;
      closedRow.hidden = value.closed === 0;
      if (!selectionOn && picked.length > 0) setPicked([]);
    },
    // --- end tab options ---
  };
}
