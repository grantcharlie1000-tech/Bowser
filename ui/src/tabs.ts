// Tab strip view (?view=tabs): keyed reconciliation of the tab list, favicons, groups, pointer and keyboard input.
// Page-controlled text is written with textContent only; favicons are data: URIs set as img.src.
import { onInbound, send, type TabAction, type TabItem, type TabsMsg } from './protocol';
import { el, icon, place } from './dom';
import { applyTheme, DRAG_ATTR } from './view';
import { setTip } from './tooltip';
// --- vertical sidebar (lane S) ---
import { createSidebarRenderer, type SidebarWorkspace } from './sidebar';
// --- end vertical sidebar ---
import {
  beforeId,
  buildEntries,
  dropIndex,
  dragScrollDelta,
  groupWeight,
  hiddenForTab,
  layoutSignature,
  privateTabName,
  pruneFavs,
  rememberFav,
  slotShift,
  stepMoveBefore,
  scrollDeltaForWheel,
  type Entry,
  type GroupEntry,
} from './tabs-logic';

/** Fixed island accents, indexed by the host's colour ids (-1 means the theme accent). */
const ISLAND: readonly string[] = [
  'var(--bw-group-0)',
  'var(--bw-group-1)',
  'var(--bw-group-2)',
  'var(--bw-group-3)',
  'var(--bw-group-4)',
  'var(--bw-group-5)',
  'var(--bw-group-6)',
  'var(--bw-group-7)',
];
const ACCENT = 'var(--bw-accent)';
const DRAG_START_PX = 4;
/** Above this many tabs, position animations are skipped (each one costs a measured layout). */
const FLIP_MAX_TABS = 80;

export interface TabNode {
  readonly id: number;
  readonly root: HTMLDivElement;
  readonly img: HTMLImageElement;
  readonly title: HTMLSpanElement;
  readonly marker: HTMLSpanElement;
  readonly audio: HTMLButtonElement;
  readonly close: HTMLButtonElement;
  uri: string;
  shift: number;
}

interface GroupNode {
  readonly shell: HTMLDivElement;
  readonly ball: HTMLButtonElement;
  readonly name: HTMLSpanElement;
  label: string;
  color: string;
  weight: number;
  visible: number;
  /** Observes the header chip so the island's flex-basis can reserve its (content-sized) width. */
  readonly watch: ResizeObserver;
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
  /** Pointer travel since the drag started (px). */
  dx: number;
}

function colourOf(gcol: number | undefined): string {
  if (gcol === undefined || gcol < 0) return ACCENT;
  return ISLAND[gcol & 7] ?? ACCENT;
}

/** Exported for test/private.mjs (fake DOM); the strip itself is built by startTabs. */
export function createTab(id: number): TabNode {
  const root = el('div', 'tab bw-focus-inset');
  root.setAttribute('role', 'tab');
  root.dataset.id = String(id);
  root.dataset.icon = 'glyph';

  const img = el('img', 'fav');
  img.alt = '';
  img.draggable = false;
  // Placeholder for tabs without a favicon: the family's globe glyph, no tile.
  const glyph = el('span', 'glyph');
  glyph.setAttribute('aria-hidden', 'true');
  glyph.append(icon('workspace-globe'));
  const spin = el('span', 'spin bw-spinner');
  // Private tabs (tabs.css): with no favicon the private glyph replaces the globe; with a favicon (or
  // the spinner) a small glasses badge sits inside the icon's corner, out of flow, so neither ever
  // moves or sizes anything in the row.
  const privGlyph = el('span', 'priv-glyph');
  privGlyph.setAttribute('aria-hidden', 'true');
  privGlyph.append(icon('private'));
  const priv = el('span', 'priv');
  priv.setAttribute('aria-hidden', 'true');
  priv.append(icon('glasses', { size: 8, fine: true }));
  const ico = el('span', 'ico');
  ico.append(img, glyph, privGlyph, spin, priv);

  const title = el('span', 'title');
  title.dir = 'auto';
  const marker = el('span', 'marker');
  marker.setAttribute('aria-hidden', 'true');

  const audio = el('button', 'audio bw-icon-btn');
  audio.type = 'button';
  audio.tabIndex = -1;
  audio.hidden = true;
  audio.setAttribute('aria-label', 'Unmute tab');
  audio.append(icon('mute'));

  const close = el('button', 'close bw-icon-btn');
  close.type = 'button';
  close.tabIndex = -1;
  close.setAttribute('aria-label', 'Close tab: Tab');
  close.append(icon('close'));

  root.append(ico, title, marker, audio, close);
  return { id, root, img, title, marker, audio, close, uri: '', shift: 0 };
}

export function updateTab(n: TabNode, item: TabItem, selected: boolean, uri: string): void {
  if (n.title.textContent !== item.title) n.title.textContent = item.title;
  const label = item.title !== '' ? item.title : 'Tab';
  const name = item.private ? privateTabName(label) : label;
  if (n.root.getAttribute('aria-label') !== name) {
    n.root.setAttribute('aria-label', name);
    setTip(n.root, name);
  }
  if (item.private) {
    if (n.root.dataset.private !== 'true') n.root.dataset.private = 'true';
  } else if (n.root.dataset.private !== undefined) {
    delete n.root.dataset.private;
  }
  n.root.setAttribute('aria-selected', String(selected));
  n.close.setAttribute('aria-label', `Close tab: ${label}`);
  setTip(n.close, 'Close tab');
  setTip(n.audio, 'Unmute tab');
  if (n.audio.hidden !== !item.muted) n.audio.hidden = !item.muted;
  n.audio.setAttribute('aria-label', `Unmute tab: ${label}`);
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
  const ball = el('button', 'ball bw-focus-inset');
  ball.type = 'button';
  ball.dataset.key = String(key);
  const dot = el('span', 'dot');
  dot.setAttribute('aria-hidden', 'true');
  const name = el('span', 'gname');
  name.dir = 'auto';
  ball.append(dot, name);
  // The group is one flex item: `--go` (header chip width) lets CSS give its tabs the same share of the
  // row as ungrouped tabs, `--gw` / `--gn` the summed weight and number of visible tabs.
  const watch = new ResizeObserver(() => {
    const w = `${ball.offsetWidth}px`;
    if (shell.style.getPropertyValue('--go') !== w) shell.style.setProperty('--go', w);
  });
  watch.observe(ball);
  return { shell, ball, name, label: '', color: '', weight: -1, visible: -1, watch };
}

function syncGroup(g: GroupNode, e: GroupEntry, active: number): void {
  const name = e.leader.gname;
  const label = name !== undefined && name !== '' ? name : 'Tab group';
  if (g.label !== label) {
    g.label = label;
    setTip(g.ball, label);
    g.ball.setAttribute('aria-label', label);
    g.name.textContent = name !== undefined && name !== '' ? name : '';
  }
  const expanded = String(!e.collapsed);
  if (g.ball.getAttribute('aria-expanded') !== expanded) g.ball.setAttribute('aria-expanded', expanded);

  const color = colourOf(e.leader.gcol);
  if (g.color !== color) {
    g.color = color;
    g.shell.style.setProperty('--gc', color);
  }
  g.shell.classList.toggle('collapsed', e.collapsed);

  const weight = groupWeight(e.members, active);
  if (g.weight !== weight) {
    g.weight = weight;
    g.shell.style.setProperty('--gw', String(weight));
  }
  if (g.visible !== e.visible) {
    g.visible = e.visible;
    g.shell.style.setProperty('--gn', String(e.visible));
  }
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
  /** True after the first horizontal render: later new tabs animate in, the first batch does not. */
  let rendered = false;
  let signature = '';
  /** Tab id to refocus after the next render (keyboard reorder moves the focused node). */
  let refocus = 0;
  /** A committed drag leaves its transforms in place until the host's reorder arrives. */
  let settling = false;
  let settleTimer = 0;
  const reduced = (): boolean => matchMedia('(prefers-reduced-motion: reduce)').matches;

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
      if (rendered) {
        // A new tab grows in (opacity + .96 scale, 167ms); the class is dropped afterwards so it
        // never fights the position animation or the drag transform.
        const root = node.root;
        root.classList.add('bw-enter', 'is-scale');
        root.addEventListener('animationend', (ev) => {
          if (ev.target === root) root.classList.remove('bw-enter', 'is-scale');
        });
      }
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

    // FLIP "first": where every visible tab is now (drag transforms included), measured only when
    // the arrangement is about to change.
    const nextSignature = layoutSignature(entries);
    const animate = rendered && !reduced() && tabNodes.size <= FLIP_MAX_TABS && (nextSignature !== signature || settling);
    const before = new Map<TabNode, number>();
    const ghosts: HTMLElement[] = [];
    if (animate) {
      for (const n of tabNodes.values()) {
        if (n.root.isConnected && !n.root.hidden) before.set(n, n.root.getBoundingClientRect().left);
      }
      strip.classList.add('settle');
      for (const n of tabNodes.values()) setShift(n, 0);
    } else if (settling) {
      strip.classList.add('settle');
      for (const n of tabNodes.values()) setShift(n, 0);
      void strip.offsetWidth;
      strip.classList.remove('settle');
    }
    clearTimeout(settleTimer);
    settling = false;

    for (const [id, n] of tabNodes) {
      if (!live.has(id)) {
        if (animate && before.has(n)) {
          // The closed tab fades where it was while its neighbours slide into the gap.
          const r = n.root.getBoundingClientRect();
          const stripBox = strip.getBoundingClientRect();
          n.root.removeAttribute('role');
          n.root.setAttribute('aria-hidden', 'true');
          n.root.classList.add('ghost', 'bw-exit');
          n.root.style.left = `${r.left - stripBox.left + strip.scrollLeft}px`;
          n.root.style.top = `${r.top - stripBox.top}px`;
          n.root.style.width = `${r.width}px`;
          const drop = (): void => n.root.remove();
          n.root.addEventListener('animationend', (ev) => {
            if (ev.target === n.root) drop();
          });
          setTimeout(drop, 400);
          ghosts.push(n.root);
        } else {
          n.root.remove();
        }
        tabNodes.delete(id);
      }
    }
    for (const [key, g] of groups) {
      if (!keys.has(key)) {
        g.watch.disconnect();
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
      syncGroup(g, e, activeId);
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
    strip.append(...ghosts);

    if (animate) {
      // FLIP "last, invert, play": jump each moved tab back to where it was, then release.
      let moved = false;
      for (const [n, left] of before) {
        if (!n.root.isConnected || n.root.hidden || n.root.classList.contains('ghost')) continue;
        const dx = left - n.root.getBoundingClientRect().left;
        if (Math.abs(dx) < 0.5) continue;
        n.root.style.transform = `translateX(${dx}px)`;
        moved = true;
      }
      if (moved) void strip.offsetWidth;
      strip.classList.remove('settle');
      for (const n of before.keys()) if (n.root.style.transform !== '') n.root.style.transform = '';
    }
    signature = nextSignature;
    rendered = true;
    applyRoving();
    if (refocus !== 0) {
      tabNodes.get(refocus)?.root.focus({ preventScroll: true });
      refocus = 0;
    }
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
    if (commit && d.dest !== d.from) {
      // Leave every tab visually where it landed; the host's reorder then arrives and render()
      // slides from these positions to the real ones, so nothing snaps back and forth.
      const others = d.order.filter((n) => n !== d.node).map((n) => n.id);
      send({ v: 1, t: 'tabmove', id: d.node.id, before: beforeId(others, d.dest) });
      settling = true;
      d.node.root.style.transform = `translateX(${d.dx}px)`;
      clearTimeout(settleTimer);
      settleTimer = window.setTimeout(() => {
        // The host never reordered (refused move): ease everything back.
        if (!settling) return;
        settling = false;
        d.node.root.style.transform = '';
        for (const n of d.order) setShift(n, 0);
      }, 400);
      schedule();
      return;
    }
    d.node.root.style.transform = '';
    d.node.shift = 0;
    for (const n of d.order) setShift(n, 0);
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
    if (ev.target.closest('.close, .audio, .ball')) return;
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
    drag = { pointerId: ev.pointerId, startX: ev.clientX, node, order, centers, from, active: false, dest: from, dx: 0 };
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
    d.dx = dx;
    d.node.root.style.transform = `translateX(${dx}px) scale(1.02)`;
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
    const a: TabAction = t.closest('.close') ? 'close' : t.closest('.audio') ? 'mute' : 'select';
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
    // Enter/Space on the close or mute button activates that button (click), not the tab.
    if ((ev.key === 'Enter' || ev.key === ' ') && t.closest('.close, .audio')) return;
    const id = Number(tab.dataset.id);
    const list = visibleTabs();
    const at = list.findIndex((n) => n.id === id);
    if (at < 0) return;
    let next: number;
    if (ev.ctrlKey && ev.shiftKey && (ev.key === 'ArrowLeft' || ev.key === 'ArrowRight')) {
      // Keyboard reorder: one slot per press; focus follows the moved tab once the host reorders.
      const before = stepMoveBefore(list.map((n) => n.id), at, ev.key === 'ArrowLeft' ? -1 : 1);
      ev.preventDefault();
      if (before !== null) {
        refocus = id;
        send({ v: 1, t: 'tabmove', id, before });
      }
      return;
    }
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
