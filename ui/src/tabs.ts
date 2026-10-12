// Tab strip view (?view=tabs): keyed reconciliation of the tab list, favicons, groups, pointer and keyboard input.
// Page-controlled text is written with textContent only; favicons are data: URIs set as img.src.
import { onInbound, send, type TabAction, type TabItem, type TabsMsg } from './protocol';
import { el, icon, place } from './dom';
import { applyTheme, DRAG_ATTR } from './view';
// --- vertical sidebar (lane S) ---
import { createSidebarRenderer, type SidebarWorkspace } from './sidebar';
// --- end vertical sidebar ---
import {
  beforeId,
  buildEntries,
  dropIndex,
  dragScrollDelta,
  hiddenForTab,
  pruneFavs,
  rememberFav,
  slotShift,
  scrollDeltaForWheel,
  type Entry,
  type GroupEntry,
} from './tabs-logic';

/** Fixed island accents, indexed by the host's colour ids (-1 means the theme accent). */
const ISLAND: readonly string[] = [
  'var(--island-0)',
  'var(--island-1)',
  'var(--island-2)',
  'var(--island-3)',
  'var(--island-4)',
  'var(--island-5)',
  'var(--island-6)',
  'var(--island-7)',
];
const ACCENT = 'var(--focus-border)';
const DRAG_START_PX = 4;

interface TabNode {
  readonly id: number;
  readonly root: HTMLDivElement;
  readonly img: HTMLImageElement;
  readonly title: HTMLSpanElement;
  readonly marker: HTMLSpanElement;
  uri: string;
  shift: number;
}

interface GroupNode {
  readonly shell: HTMLDivElement;
  readonly ball: HTMLButtonElement;
  label: string;
  color: string;
  flex: string;
}

interface Drag {
  readonly pointerId: number;
  readonly startX: number;
  readonly node: TabNode;
  /** Visible tabs in strip order when the drag started. */
  readonly order: readonly TabNode[];
  centers: readonly number[];
  readonly from: number;
  active: boolean;
  dest: number;
}

function colourOf(gcol: number | undefined): string {
  if (gcol === undefined || gcol < 0) return ACCENT;
  return ISLAND[gcol & 7] ?? ACCENT;
}

function createTab(id: number): TabNode {
  const root = el('div', 'tab');
  root.setAttribute('role', 'tab');
  root.dataset.id = String(id);
  root.dataset.icon = 'glyph';

  const close = el('button', 'close');
  close.type = 'button';
  close.tabIndex = -1;
  close.setAttribute('aria-label', 'Close tab: Tab');
  close.append(icon('close'));

  const img = el('img', 'fav');
  img.alt = '';
  img.draggable = false;
  // Placeholder for tabs without a favicon: a small rounded tile with a plus.
  const glyph = el('span', 'glyph');
  glyph.setAttribute('aria-hidden', 'true');
  glyph.append(icon('plus'));
  const spin = el('span', 'spin');
  const ico = el('span', 'ico');
  ico.append(img, glyph, spin);

  const title = el('span', 'title');
  title.dir = 'auto';
  // Favicon and title are one centred group, so the group stays centred in the tab.
  const label = el('span', 'label');
  label.append(ico, title);
  const marker = el('span', 'marker');
  marker.setAttribute('aria-hidden', 'true');

  root.append(close, label, marker);
  return { id, root, img, title, marker, uri: '', shift: 0 };
}

function updateTab(n: TabNode, item: TabItem, selected: boolean, uri: string): void {
  if (n.title.textContent !== item.title) n.title.textContent = item.title;
  const label = item.title !== '' ? item.title : 'Tab';
  if (n.root.getAttribute('aria-label') !== label) {
    n.root.setAttribute('aria-label', label);
    n.root.title = label;
  }
  n.root.setAttribute('aria-selected', String(selected));
  const close = n.root.querySelector<HTMLButtonElement>('.close');
  if (close) close.setAttribute('aria-label', `Close tab: ${label}`);
  n.root.classList.toggle('active', selected);
  n.root.classList.toggle('pinned', item.pinned);
  n.root.classList.toggle('muted', item.muted);

  const mode = item.loading ? 'spin' : uri !== '' ? 'fav' : 'glyph';
  if (n.root.dataset.icon !== mode) n.root.dataset.icon = mode;
  if (n.uri !== uri) {
    n.uri = uri;
    if (uri !== '') n.img.src = uri;
    else n.img.removeAttribute('src');
  }

  const marked = item.marker >= 0;
  n.marker.classList.toggle('on', marked);
  if (marked) {
    const colour = ISLAND[item.marker & 7] ?? ACCENT;
    if (n.marker.style.getPropertyValue('--mc') !== colour) n.marker.style.setProperty('--mc', colour);
  }
}

function createGroup(key: number): GroupNode {
  const shell = el('div', 'group');
  shell.setAttribute('role', 'none');
  shell.dataset.key = String(key);
  const ball = el('button', 'ball');
  ball.type = 'button';
  ball.dataset.key = String(key);
  return { shell, ball, label: '', color: '', flex: '' };
}

function syncGroup(g: GroupNode, e: GroupEntry): void {
  const name = e.leader.gname;
  const label = name !== undefined && name !== '' ? name : 'Tab group';
  if (g.label !== label) {
    g.label = label;
    g.ball.title = label;
    g.ball.setAttribute('aria-label', label);
  }
  const expanded = String(!e.collapsed);
  if (g.ball.getAttribute('aria-expanded') !== expanded) g.ball.setAttribute('aria-expanded', expanded);

  const color = colourOf(e.leader.gcol);
  if (g.color !== color) {
    g.color = color;
    g.shell.style.setProperty('--gc', color);
  }
  const flex = e.collapsed ? '0 1 auto' : `${Math.max(1, e.visible)} 1 0px`;
  if (g.flex !== flex) {
    g.flex = flex;
    g.shell.style.flex = flex;
  }
  g.shell.classList.toggle('collapsed', e.collapsed);
}

function setShift(n: TabNode, px: number): void {
  if (n.shift === px) return;
  n.shift = px;
  n.root.style.transform = px === 0 ? '' : `translateX(${px}px)`;
}

/** Builds the tab strip into `strip` (the #app element) and wires it to the host. */
export function startTabs(strip: HTMLElement): void {
  // --- vertical sidebar (lane S) ---
  const sidebar = createSidebarRenderer(strip);
  let layoutMode: 'horizontal' | 'vertical' = 'horizontal';
  let workspace: SidebarWorkspace = { name: 'Workspace', icon: 'g:briefcase' };
  // --- end vertical sidebar ---
  strip.classList.remove('sidebar');
  strip.removeAttribute('aria-orientation');
  strip.classList.add('strip');
  strip.setAttribute('role', 'tablist');
  strip.setAttribute('aria-label', 'Tabs');
  strip.setAttribute(DRAG_ATTR, '');
  strip.replaceChildren();

  const tabNodes = new Map<number, TabNode>();
  const groups = new Map<number, GroupNode>();
  const favs = new Map<number, string>();
  let activeId = 0;
  let latest: TabsMsg | null = null;
  let dirty = false;
  let frame = 0;
  let drag: Drag | null = null;
  let ignoreClick = false;

  const schedule = (): void => {
    if (frame === 0) frame = requestAnimationFrame(flush);
  };

  // One render per animation frame, however many host messages arrived. Deferred while a drag is live.
  const flush = (): void => {
    frame = 0;
    const msg = latest;
    if (drag?.active || !dirty || msg === null) return;
    dirty = false;
    render(msg);
  };

  const tabFor = (item: TabItem): TabNode => {
    let node = tabNodes.get(item.id);
    if (!node) {
      node = createTab(item.id);
      tabNodes.set(item.id, node);
    }
    return node;
  };

  const syncTab = (item: TabItem): TabNode => {
    const n = tabFor(item);
    updateTab(n, item, item.id === activeId, item.fav ? (favs.get(item.id) ?? '') : '');
    return n;
  };

  const visibleTabs = (): TabNode[] => {
    const list: TabNode[] = [];
    for (const node of Array.from(strip.querySelectorAll<HTMLElement>('.tab:not([hidden])'))) {
      const n = tabNodes.get(Number(node.dataset.id));
      if (n) list.push(n);
    }
    return list;
  };

  // Roving tabindex: one tab in the tab order, the active one unless the keyboard moved focus.
  const applyRoving = (focus?: TabNode): void => {
    const list = visibleTabs();
    const target = focus ?? list.find((n) => n.id === activeId) ?? list[0];
    for (const n of tabNodes.values()) {
      const t = n === target ? 0 : -1;
      if (n.root.tabIndex !== t) n.root.tabIndex = t;
    }
  };

  const render = (msg: TabsMsg): void => {
    // --- vertical sidebar (lane S) ---
    if (layoutMode === 'vertical') {
      activeId = msg.active;
      sidebar.render(msg, favs, workspace);
      return;
    }
    strip.classList.remove('sidebar');
    strip.classList.add('strip');
    strip.removeAttribute('aria-orientation');
    // --- end vertical sidebar ---
    activeId = msg.active;
    const entries: Entry[] = buildEntries(msg.items, msg.active);
    const live = new Set<number>(msg.items.map((it) => it.id));
    const keys = new Set<number>();
    for (const e of entries) if (e.kind === 'group') keys.add(e.key);

    for (const [id, n] of tabNodes) {
      if (!live.has(id)) {
        n.root.remove();
        tabNodes.delete(id);
      }
    }
    for (const [key, g] of groups) {
      if (!keys.has(key)) {
        g.shell.remove();
        groups.delete(key);
      }
    }
    pruneFavs(favs, live);

    const top: HTMLElement[] = [];
    for (const e of entries) {
      if (e.kind === 'tab') {
        const n = syncTab(e.slot.item);
        n.root.hidden = false;
        top.push(n.root);
        continue;
      }
      let g = groups.get(e.key);
      if (!g) {
        g = createGroup(e.key);
        groups.set(e.key, g);
      }
      syncGroup(g, e);
      const nodes: HTMLElement[] = [g.ball];
      for (const slot of e.members) {
        const n = syncTab(slot.item);
        n.root.hidden = hiddenForTab(true, e.collapsed, slot.item.id === activeId);
        nodes.push(n.root);
      }
      place(g.shell, nodes);
      top.push(g.shell);
    }
    place(strip, top);
    let pinnedLeft = 0;
    for (const node of tabNodes.values()) {
      if (!node.root.classList.contains('pinned')) continue;
      node.root.style.left = `${pinnedLeft}px`;
      pinnedLeft += node.root.getBoundingClientRect().width + 3;
    }
    applyRoving();
    tabNodes.get(activeId)?.root.scrollIntoView({
      block: 'nearest', inline: 'nearest',
      behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'instant' : 'smooth',
    });
    updateEdges();
  };

  // ----- drag reorder -----

  const onDragKey = (ev: KeyboardEvent): void => {
    if (ev.key !== 'Escape') return;
    ev.preventDefault();
    ev.stopPropagation();
    endDrag(false);
  };

  const endDrag = (commit: boolean): void => {
    const d = drag;
    if (!d) return;
    drag = null;
    window.removeEventListener('keydown', onDragKey, true);
    try { if (d.node.root.hasPointerCapture(d.pointerId)) d.node.root.releasePointerCapture(d.pointerId); } catch { /* capture may already be lost */ }
    if (!d.active) return;
    ignoreClick = true;
    d.node.root.classList.remove('dragging');
    d.node.root.style.transform = '';
    d.node.shift = 0;
    for (const n of d.order) setShift(n, 0);
    if (commit && d.dest !== d.from) {
      const others = d.order.filter((n) => n !== d.node).map((n) => n.id);
      send({ v: 1, t: 'tabmove', id: d.node.id, before: beforeId(others, d.dest) });
    }
    schedule();
  };

  const updateEdges = (): void => {
    const left = strip.scrollLeft > 1;
    const right = strip.scrollLeft + strip.clientWidth < strip.scrollWidth - 1;
    strip.style.setProperty('--fade-left', left ? '12px' : '0px');
    strip.style.setProperty('--fade-right', right ? '12px' : '0px');
  };

  const onPointerDown = (ev: PointerEvent): void => {
    ignoreClick = false;
    if (ev.button !== 0 || drag !== null || !(ev.target instanceof Element)) return;
    if (ev.target.closest('.close, .ball')) return;
    const tab = ev.target.closest<HTMLElement>('.tab');
    if (!tab) {
      if (ev.target instanceof HTMLElement && ev.target.hasAttribute(DRAG_ATTR)) send({ v: 1, t: 'drag' });
      return;
    }
    const node = tabNodes.get(Number(tab.dataset.id));
    if (!node) return;
    const order = visibleTabs();
    const from = order.indexOf(node);
    if (from < 0) return;
    const centers = order.map((n) => {
      const r = n.root.getBoundingClientRect();
      return r.left + r.width / 2;
    });
    drag = { pointerId: ev.pointerId, startX: ev.clientX, node, order, centers, from, active: false, dest: from };
    try { node.root.setPointerCapture(ev.pointerId); } catch { /* window listeners still end the drag */ }
  };

  const onPointerMove = (ev: PointerEvent): void => {
    const d = drag;
    if (!d || ev.pointerId !== d.pointerId) return;
    const dx = ev.clientX - d.startX;
    if (!d.active) {
      if (Math.abs(dx) < DRAG_START_PX) return;
      d.active = true;
      d.node.root.classList.add('dragging');
      window.addEventListener('keydown', onDragKey, true);
    }
    d.node.root.style.transform = `translateX(${dx}px)`;
    const edgeDelta = dragScrollDelta(ev.clientX, strip.getBoundingClientRect().left, strip.getBoundingClientRect().right);
    if (edgeDelta !== 0) {
      strip.scrollLeft += edgeDelta;
      updateEdges();
      d.centers = d.order.map((n) => { const r = n.root.getBoundingClientRect(); return r.left + r.width / 2; });
    }
    d.dest = dropIndex(d.centers, d.from, (d.centers[d.from] ?? 0) + dx);
    d.order.forEach((n, i) => {
      if (i !== d.from) setShift(n, slotShift(d.centers, i, d.from, d.dest));
    });
  };

  const onPointerEnd = (ev: PointerEvent, commit: boolean): void => {
    const d = drag;
    if (d && ev.pointerId === d.pointerId) endDrag(commit);
  };

  // ----- events -----

  strip.addEventListener('pointerdown', onPointerDown);
  strip.addEventListener('pointermove', onPointerMove);
  strip.addEventListener('pointerup', (ev) => onPointerEnd(ev, true));
  strip.addEventListener('pointercancel', (ev) => onPointerEnd(ev, false));
  strip.addEventListener('lostpointercapture', (ev) => onPointerEnd(ev, false));
  window.addEventListener('pointerup', (ev) => onPointerEnd(ev, true));
  window.addEventListener('pointercancel', (ev) => onPointerEnd(ev, false));
  window.addEventListener('blur', () => endDrag(false));
  strip.addEventListener('scroll', updateEdges, { passive: true });
  strip.addEventListener('wheel', (ev) => {
    const delta = scrollDeltaForWheel(ev.deltaX, ev.deltaY, ev.shiftKey);
    if (delta !== 0) { ev.preventDefault(); strip.scrollLeft += delta; }
  }, { passive: false });

  strip.addEventListener('click', (ev) => {
    const t = ev.target;
    if (!(t instanceof Element)) return;
    const ball = t.closest<HTMLElement>('.ball');
    if (ball) {
      send({ v: 1, t: 'group', a: 'toggle', key: Number(ball.dataset.key) });
      return;
    }
    const tab = t.closest<HTMLElement>('.tab');
    if (!tab) return;
    if (ignoreClick) {
      ignoreClick = false;
      return;
    }
    const id = Number(tab.dataset.id);
    const a: TabAction = t.closest('.close') ? 'close' : 'select';
    send({ v: 1, t: 'tab', a, id });
  });

  strip.addEventListener('auxclick', (ev) => {
    if (ev.button !== 1 || !(ev.target instanceof Element)) return;
    const tab = ev.target.closest<HTMLElement>('.tab');
    if (!tab) return;
    ev.preventDefault();
    send({ v: 1, t: 'tab', a: 'close', id: Number(tab.dataset.id) });
  });

  // Middle button on a tab closes it; stop the browser's autoscroll cursor from starting.
  strip.addEventListener('mousedown', (ev) => {
    if (ev.button === 1 && ev.target instanceof Element && ev.target.closest('.tab')) ev.preventDefault();
  });

  strip.addEventListener('contextmenu', (ev) => {
    const t = ev.target;
    if (!(t instanceof Element)) return;
    const ball = t.closest<HTMLElement>('.ball');
    if (ball) {
      ev.preventDefault();
      send({ v: 1, t: 'group', a: 'menu', key: Number(ball.dataset.key) });
      return;
    }
    const tab = t.closest<HTMLElement>('.tab');
    if (tab) {
      ev.preventDefault();
      send({ v: 1, t: 'tab', a: 'menu', id: Number(tab.dataset.id) });
      return;
    }
    if (t instanceof HTMLElement && t.hasAttribute(DRAG_ATTR)) {
      ev.preventDefault();
      send({ v: 1, t: 'caption', a: 'systemMenu' });
    }
  });

  strip.addEventListener('dblclick', (ev) => {
    if (ev.target instanceof HTMLElement && ev.target.hasAttribute(DRAG_ATTR)) {
      send({ v: 1, t: 'caption', a: 'toggleMaximize' });
    }
  });

  strip.addEventListener('keydown', (ev) => {
    const t = ev.target;
    if (!(t instanceof Element)) return;
    const tab = t.closest<HTMLElement>('.tab');
    if (!tab) return;
    const id = Number(tab.dataset.id);
    const list = visibleTabs();
    const at = list.findIndex((n) => n.id === id);
    if (at < 0) return;
    let next: number;
    switch (ev.key) {
      case 'ArrowLeft':
        next = Math.max(0, at - 1);
        break;
      case 'ArrowRight':
        next = Math.min(list.length - 1, at + 1);
        break;
      case 'Home':
        next = 0;
        break;
      case 'End':
        next = list.length - 1;
        break;
      case 'Enter':
      case ' ':
        ev.preventDefault();
        send({ v: 1, t: 'tab', a: 'select', id });
        return;
      case 'Delete':
        ev.preventDefault();
        send({ v: 1, t: 'tab', a: 'close', id });
        return;
      default:
        return;
    }
    ev.preventDefault();
    const target = list[next];
    if (!target) return;
    applyRoving(target);
    target.root.focus();
    target.root.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  });

  onInbound((msg) => {
    switch (msg.t) {
      // --- vertical sidebar (lane S) ---
      case 'layout':
        layoutMode = msg.mode;
        if (latest) {
          dirty = true;
          schedule();
        } else if (layoutMode === 'vertical') {
          strip.classList.remove('strip');
          strip.classList.add('sidebar');
          strip.setAttribute('aria-orientation', 'vertical');
        } else {
          strip.classList.remove('sidebar');
          strip.classList.add('strip');
          strip.removeAttribute('aria-orientation');
        }
        break;
      case 'workspace':
        workspace = { name: msg.name, icon: msg.icon };
        if (layoutMode === 'vertical') sidebar.workspace(workspace);
        break;
      // --- end vertical sidebar ---
      case 'state':
        // Only the theme and mode reach the tab strip; scale and page state are toolbar concerns.
        applyTheme(msg.theme, msg.solid, msg.dark);
        break;
      case 'tabs':
        latest = msg;
        dirty = true;
        schedule();
        break;
      case 'fav':
        rememberFav(favs, msg.id, msg.uri, new Set(latest?.items.map((item) => item.id) ?? []));
        dirty = true;
        schedule();
        break;
    }
  });

  // Registered before ready so no tabs message can be missed.
  send({ v: 1, t: 'ready' });
}
