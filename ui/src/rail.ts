// Left rail view (?view=rail): keyed reconciliation of the rail buttons, favicons, pointer reorder and keyboard.
// Labels reach the DOM only through textContent and attributes; favicons are validated data: URIs set as img.src.
import { onInbound, send, type RailItem, type RailMsg } from './protocol';
import {
  beforeKey,
  dropSlot,
  initialOf,
  isDraggable,
  pruneRailFavs,
  rememberRailFav,
  sectionOf,
  splitSections,
  type Section,
} from './rail-logic';
import { railIcon } from './rail-icons';
import { el, place } from './dom';
import { applyTheme } from './view';

const DRAG_START_PX = 4;
/** Gap between two rail items; the insertion line sits in the middle of it. */
const GAP_PX = 6;
const FALLBACK_LABEL = 'Rail item';

interface RailNode {
  readonly key: string;
  readonly root: HTMLButtonElement;
  readonly face: HTMLSpanElement;
  readonly img: HTMLImageElement;
  /** What the face currently shows: 'fav', 'letter:<initial>' or 'icon:<name>'. */
  faceSig: string;
  uri: string;
  accent: string;
  label: string | null;
}

interface Drag {
  readonly pointerId: number;
  readonly startY: number;
  readonly key: string;
  readonly node: HTMLButtonElement;
  /** Draggable buttons of the same section, in order, including the dragged one. */
  readonly centres: readonly number[];
  readonly from: number;
  /** The other draggables, in order, and their keys. */
  readonly others: readonly HTMLButtonElement[];
  readonly otherKeys: readonly string[];
  /** Viewport y of the insertion line before each other item; the last entry is after the final one. */
  readonly lines: readonly number[];
  active: boolean;
  dest: number;
}

function setAttr(node: Element, name: string, value: string | null): void {
  if (value === null) {
    if (node.hasAttribute(name)) node.removeAttribute(name);
  } else if (node.getAttribute(name) !== value) {
    node.setAttribute(name, value);
  }
}

function rule(): HTMLDivElement {
  const line = el('div', 'rail-rule');
  line.setAttribute('aria-hidden', 'true');
  return line;
}

function createNode(key: string): RailNode {
  const root = el('button', 'rb');
  root.type = 'button';
  root.dataset.k = key;
  if (key.startsWith('ws:')) root.classList.add('ws');
  const bar = el('span', 'rb-bar');
  bar.setAttribute('aria-hidden', 'true');
  const face = el('span', 'rb-face');
  const dot = el('span', 'rb-dot');
  dot.setAttribute('aria-hidden', 'true');
  const img = el('img', 'rb-fav');
  img.alt = '';
  img.draggable = false;
  root.append(bar, face, dot);
  return { key, root, face, img, faceSig: '', uri: '', accent: '', label: null };
}

function faceFor(n: RailNode, item: RailItem, uri: string): HTMLElement | SVGSVGElement {
  if (uri !== '') return n.img;
  if (item.icon === 'letter') {
    const tile = el('span', 'rb-tile');
    tile.textContent = initialOf(item.label);
    return tile;
  }
  return railIcon(item.icon);
}

function updateNode(n: RailNode, item: RailItem, uri: string): void {
  const btn = n.root;
  if (n.label !== item.label) {
    n.label = item.label;
    const name = item.label.trim() !== '' ? item.label : FALLBACK_LABEL;
    setAttr(btn, 'aria-label', name);
    btn.title = name;
  }
  if (n.uri !== uri) {
    n.uri = uri;
    if (uri !== '') n.img.src = uri;
    else n.img.removeAttribute('src');
  }
  const sig = uri !== '' ? 'fav' : item.icon === 'letter' ? `letter:${initialOf(item.label)}` : `icon:${item.icon}`;
  if (n.faceSig !== sig) {
    n.faceSig = sig;
    n.face.replaceChildren(faceFor(n, item, uri));
  }
  if (n.accent !== item.accent) {
    n.accent = item.accent;
    if (item.accent !== '') btn.style.setProperty('--ac', item.accent);
    else btn.style.removeProperty('--ac');
  }
  btn.classList.toggle('on', item.active);
  btn.classList.toggle('dot-on', item.dot);
  setAttr(btn, 'aria-pressed', String(item.active));
  setAttr(btn, 'aria-current', item.active ? 'true' : null);
}

/** Builds the left rail into `root` (the #app element) and wires it to the host. */
export function startRail(root: HTMLElement): void {
  root.classList.add('rail');
  root.setAttribute('role', 'toolbar');
  root.setAttribute('aria-orientation', 'vertical');
  root.setAttribute('aria-label', 'Left rail');

  const top = el('div', 'rail-top');
  const scroll = el('div', 'rail-scroll');
  const bottom = el('div', 'rail-bottom');
  const spacer = el('div', 'rail-spacer');
  const ruleTop = rule();
  const ruleInner = rule();
  const ruleBottom = rule();
  const slot = el('div', 'rail-slot');
  slot.hidden = true;
  slot.setAttribute('aria-hidden', 'true');
  root.replaceChildren(top, ruleTop, scroll, spacer, ruleBottom, bottom, slot);

  const nodes = new Map<string, RailNode>();
  const favs = new Map<string, string>();
  let latest: RailMsg | null = null;
  let dirty = false;
  let frame = 0;
  let drag: Drag | null = null;
  let ignoreClick = false;
  let rovingKey = '';

  const buttons = (): HTMLButtonElement[] => Array.from(root.querySelectorAll<HTMLButtonElement>('.rb'));

  // Roving tabindex: one button in the tab order, the last focused, else the active one, else the first.
  const applyRoving = (focus?: HTMLButtonElement): void => {
    const list = buttons();
    const target =
      focus ??
      list.find((b) => b.dataset.k === rovingKey) ??
      list.find((b) => b.classList.contains('on')) ??
      list[0];
    if (target) rovingKey = target.dataset.k ?? '';
    for (const b of list) {
      const index = b === target ? 0 : -1;
      if (b.tabIndex !== index) b.tabIndex = index;
    }
  };

  const nodesOf = (items: readonly RailItem[]): HTMLElement[] => {
    const out: HTMLElement[] = [];
    for (const item of items) {
      const n = nodes.get(item.k);
      if (n) out.push(n.root);
    }
    return out;
  };

  const render = (msg: RailMsg): void => {
    const live = new Set<string>(msg.items.map((item) => item.k));
    for (const [key, n] of nodes) {
      if (!live.has(key)) {
        n.root.remove();
        nodes.delete(key);
      }
    }
    pruneRailFavs(favs, live);

    for (const item of msg.items) {
      let n = nodes.get(item.k);
      if (!n) {
        n = createNode(item.k);
        nodes.set(item.k, n);
      }
      updateNode(n, item, item.fav ? (favs.get(item.k) ?? '') : '');
    }

    const sections = splitSections(msg.items);
    place(top, nodesOf(sections.top));
    place(scroll, [...nodesOf(sections.workspaces), ruleInner, ...nodesOf(sections.apps)]);
    place(bottom, nodesOf(sections.bottom));
    applyRoving();
  };

  // One render per animation frame, however many host messages arrived. Deferred while a drag is live.
  const flush = (): void => {
    frame = 0;
    const msg = latest;
    if (drag?.active || !dirty || msg === null) return;
    dirty = false;
    render(msg);
  };
  const schedule = (): void => {
    if (frame === 0) frame = requestAnimationFrame(flush);
  };

  // ----- drag reorder (workspaces and apps, within their own section) -----

  const showSlot = (d: Drag): void => {
    const line = d.dest === d.from ? undefined : d.lines[d.dest];
    if (line === undefined) {
      slot.hidden = true;
      return;
    }
    const pill = root.getBoundingClientRect();
    slot.style.transform = `translateY(${line - pill.top}px)`;
    slot.hidden = false;
  };

  const onDragKey = (ev: KeyboardEvent): void => {
    if (ev.key !== 'Escape') return;
    ev.preventDefault();
    ev.stopPropagation();
    finishDrag(false);
  };

  const finishDrag = (commit: boolean): void => {
    const d = drag;
    if (!d) return;
    drag = null;
    window.removeEventListener('keydown', onDragKey, true);
    if (!d.active) return;
    ignoreClick = true;
    try { if (d.node.hasPointerCapture(d.pointerId)) d.node.releasePointerCapture(d.pointerId); } catch { /* capture may already be lost */ }
    d.node.classList.remove('dragging');
    d.node.style.transform = '';
    slot.hidden = true;
    if (commit && d.dest !== d.from) {
      send({ v: 1, t: 'railmove', k: d.key, before: beforeKey(d.otherKeys, d.dest) });
    }
    schedule();
  };

  root.addEventListener('pointerdown', (ev) => {
    ignoreClick = false;
    if (ev.button !== 0 || drag !== null || !(ev.target instanceof Element)) return;
    const node = ev.target.closest<HTMLButtonElement>('.rb');
    const key = node?.dataset.k ?? '';
    if (!node || !isDraggable(key)) return;
    const section: Section | null = sectionOf(key);
    const members = buttons().filter((b) => {
      const k = b.dataset.k ?? '';
      return isDraggable(k) && sectionOf(k) === section;
    });
    const from = members.indexOf(node);
    if (from < 0) return;
    const rects = members.map((b) => b.getBoundingClientRect());
    const others = members.filter((b) => b !== node);
    const otherRects = rects.filter((_, i) => i !== from);
    const lines = otherRects.map((r) => r.top - GAP_PX / 2);
    const last = otherRects[otherRects.length - 1];
    if (last) lines.push(last.bottom + GAP_PX / 2);
    drag = {
      pointerId: ev.pointerId,
      startY: ev.clientY,
      key,
      node,
      centres: rects.map((r) => r.top + r.height / 2),
      from,
      others,
      otherKeys: others.map((b) => b.dataset.k ?? ''),
      lines,
      active: false,
      dest: from,
    };
    try { node.setPointerCapture(ev.pointerId); } catch { /* window listeners still end the drag */ }
  });

  root.addEventListener('pointermove', (ev) => {
    const d = drag;
    if (!d || ev.pointerId !== d.pointerId) return;
    const dy = ev.clientY - d.startY;
    if (!d.active) {
      if (Math.abs(dy) < DRAG_START_PX) return;
      d.active = true;
      d.node.classList.add('dragging');
      window.addEventListener('keydown', onDragKey, true);
    }
    d.node.style.transform = `translateY(${dy}px)`;
    d.dest = dropSlot(d.centres, d.from, (d.centres[d.from] ?? 0) + dy);
    showSlot(d);
  });

  const endPointer = (ev: PointerEvent, commit: boolean): void => {
    const d = drag;
    if (d && ev.pointerId === d.pointerId) finishDrag(commit);
  };
  root.addEventListener('pointerup', (ev) => endPointer(ev, true));
  root.addEventListener('pointercancel', (ev) => endPointer(ev, false));
  root.addEventListener('lostpointercapture', (ev) => endPointer(ev, false));
  window.addEventListener('pointerup', (ev) => endPointer(ev, true));
  window.addEventListener('pointercancel', (ev) => endPointer(ev, false));
  window.addEventListener('blur', () => finishDrag(false));

  // ----- events -----

  root.addEventListener('click', (ev) => {
    if (!(ev.target instanceof Element)) return;
    const btn = ev.target.closest<HTMLButtonElement>('.rb');
    if (!btn) return;
    if (ignoreClick) {
      ignoreClick = false;
      return;
    }
    send({ v: 1, t: 'rail', a: 'click', k: btn.dataset.k ?? '' });
  });

  // Right click on any part of the rail opens the native menu for the button under the cursor.
  root.addEventListener('contextmenu', (ev) => {
    ev.preventDefault();
    if (!(ev.target instanceof Element)) return;
    const btn = ev.target.closest<HTMLButtonElement>('.rb');
    if (btn) send({ v: 1, t: 'rail', a: 'menu', k: btn.dataset.k ?? '' });
  });

  root.addEventListener('focusin', (ev) => {
    if (ev.target instanceof HTMLButtonElement && ev.target.classList.contains('rb')) applyRoving(ev.target);
  });

  root.addEventListener('keydown', (ev) => {
    if (!(ev.target instanceof Element)) return;
    const btn = ev.target.closest<HTMLButtonElement>('.rb');
    if (!btn) return;
    if (ev.key === 'ContextMenu' || (ev.key === 'F10' && ev.shiftKey)) {
      ev.preventDefault();
      send({ v: 1, t: 'rail', a: 'menu', k: btn.dataset.k ?? '' });
      return;
    }
    const list = buttons();
    const at = list.indexOf(btn);
    if (at < 0) return;
    let next: number;
    switch (ev.key) {
      case 'ArrowUp':
        next = Math.max(0, at - 1);
        break;
      case 'ArrowDown':
        next = Math.min(list.length - 1, at + 1);
        break;
      case 'Home':
        next = 0;
        break;
      case 'End':
        next = list.length - 1;
        break;
      default:
        // Enter and Space reach the native button's click; nothing else is handled here.
        return;
    }
    ev.preventDefault();
    const target = list[next];
    if (!target) return;
    applyRoving(target);
    target.focus();
  });

  onInbound((msg) => {
    switch (msg.t) {
      case 'state':
        // Only the theme and mode reach the rail; scale and page state are toolbar concerns.
        applyTheme(msg.theme, msg.solid, msg.dark);
        break;
      case 'rail':
        latest = msg;
        dirty = true;
        schedule();
        break;
      case 'railfav':
        rememberRailFav(favs, msg.k, msg.uri, new Set(latest?.items.map((item) => item.k) ?? []));
        dirty = true;
        schedule();
        break;
    }
  });

  // Registered before ready so no rail message can be missed.
  send({ v: 1, t: 'ready' });
}
