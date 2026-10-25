// The sliding thumb behind a `.bw-segmented` radio group (Settings, and the shared `segmented()`
// helper). The highlight is one absolutely positioned pill: it translates and its width morphs when
// the choice changes, the way a macOS/iOS segmented control does. Each segment reserves its bold
// width, so the glide is a single move and the row does not reflow. The first placement, a resize
// and reduced motion snap; only a later choice animates. Click and arrow keys stay on the buttons.
import { el } from './dom';

/** A segment's border box in the same coordinate space as the pointer, plus whether it can be chosen. */
export interface SegmentBox {
  readonly left: number;
  readonly right: number;
  readonly top: number;
  readonly bottom: number;
  readonly disabled: boolean;
}

/**
 * Which segment a drag is over, or -1 when the pointer has left the control.
 * A segment under the pointer wins; in a gap, the nearest centre, so a drag crosses at the midpoint.
 */
export function segmentIndex(segments: readonly SegmentBox[], group: SegmentBox, x: number, y: number): number {
  const slackY = 24;
  const slackX = 8;
  if (segments.length === 0) return -1;
  if (y < group.top - slackY || y > group.bottom + slackY) return -1;
  if (x < group.left - slackX || x > group.right + slackX) return -1;
  let inside = -1;
  let nearest = -1;
  let nearestDist = Infinity;
  for (let i = 0; i < segments.length; i++) {
    const box = segments[i];
    if (!box || box.right <= box.left) continue;
    if (x >= box.left && x <= box.right) {
      if (box.disabled) return -1;
      inside = i;
    }
    if (box.disabled) continue;
    const dist = Math.abs(x - (box.left + box.right) / 2);
    if (dist < nearestDist) {
      nearestDist = dist;
      nearest = i;
    }
  }
  return inside >= 0 ? inside : nearest;
}

const checked = (button: HTMLButtonElement): boolean =>
  button.getAttribute('aria-checked') === 'true' || button.getAttribute('aria-pressed') === 'true' || button.getAttribute('aria-selected') === 'true';

/** Motion is on when the OS allows it and Settings has not turned animations off (`data-motion`). */
function motionOn(): boolean {
  const reduce = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
  const root = globalThis.document?.documentElement;
  const off = root?.getAttribute('data-motion') === 'off';
  return !reduce && !off;
}

interface Box { x: number; y: number; w: number; h: number }

function measure(button: HTMLButtonElement): Box | null {
  const x = button.offsetLeft;
  const y = button.offsetTop;
  const w = button.offsetWidth;
  const h = button.offsetHeight;
  if (typeof x !== 'number' || typeof y !== 'number' || typeof w !== 'number' || typeof h !== 'number') return null;
  if (w <= 0 || h <= 0) return null;
  return { x, y, w, h };
}

const near = (a: Box, b: Box): boolean =>
  Math.abs(a.x - b.x) < 0.5 && Math.abs(a.y - b.y) < 0.5 && Math.abs(a.w - b.w) < 0.5 && Math.abs(a.h - b.h) < 0.5;

/**
 * Keep the button as wide as its bold label. The visible words sit out of flow and may crossfade
 * weight; the in-flow copy is always the strong weight, so choosing a segment does not reflow the row.
 */
function holdBold(button: HTMLButtonElement): void {
  if (button.querySelector('.bw-seg-label')) return;
  const text = button.textContent ?? '';
  if (!text) return;
  button.setAttribute('data-label', text);
  const label = el('span', 'bw-seg-label');
  const kids = button.childNodes;
  if (kids && typeof kids.length === 'number') {
    while (kids.length > 0) {
      const node = kids[0];
      if (!node) break;
      label.append(node);
    }
  }
  button.append(label);
}

/**
 * Adds the sliding thumb to `group` and keeps it on the checked button.
 * Safe to call once the option buttons are already children. A second call does nothing.
 */
export function attachSegmentThumb(group: HTMLElement): void {
  if (group.querySelector('.bw-seg-thumb')) return;
  const thumb = el('span', 'bw-seg-thumb');
  const face = el('span', 'bw-seg-thumb-face');
  thumb.append(face);
  thumb.hidden = true;
  thumb.setAttribute('aria-hidden', 'true');
  group.classList.add('is-thumb');
  group.append(thumb);

  const buttons = (): HTMLButtonElement[] => Array.from(group.querySelectorAll('button'));
  const chosen = (): HTMLButtonElement | null => buttons().find(checked) ?? null;

  let seen = false;
  let gliding = false;
  let glide = 0;
  let placed: Box | null = null;
  let pointer: { id: number; x: number; y: number; dragged: boolean } | null = null;
  let suppressClick = false;

  const raf = (cb: () => void): void => {
    if (typeof requestAnimationFrame === 'function') requestAnimationFrame(cb);
    else cb();
  };

  const commit = (box: Box, animate: boolean): void => {
    if (!animate && placed && near(placed, box) && !thumb.hidden) return;
    const reveal = thumb.hidden;
    thumb.hidden = false;
    // A press scale belongs to the hold, not the glide. Drop it before the thumb moves, without
    // letting the 0.95 scale ease back across the slide.
    if (animate && group.classList.contains('is-pressed')) {
      group.classList.remove('is-pressed');
      face.style.transition = 'none';
      void face.offsetWidth;
      face.style.transition = '';
    }
    if (!animate || reveal) group.classList.remove('is-ready');
    // Set the transitioned properties themselves. A custom property inside translate() does not
    // interpolate unless it is registered, and registration adds a cascade layer the sheet forbids.
    thumb.style.width = `${box.w}px`;
    thumb.style.height = `${box.h}px`;
    thumb.style.transform = `translate(${box.x}px, ${box.y}px)`;
    placed = box;
    group.classList.add('is-placed');
    if (!animate || reveal) {
      // Flush the snapped box before transitions turn on, so the first paint and a resize do not slide.
      void thumb.offsetWidth;
      gliding = false;
      raf(() => {
        if (group.isConnected === false) return;
        group.classList.add('is-ready');
      });
    } else {
      group.classList.add('is-ready');
      gliding = true;
      const token = ++glide;
      // If the transition never ends (the box did not change), resize tracking must not stay off.
      setTimeout(() => {
        if (token !== glide || !gliding) return;
        gliding = false;
        place(false);
      }, 400);
    }
    seen = true;
  };

  const place = (fromUser: boolean): void => {
    const button = chosen();
    if (!button) {
      thumb.hidden = true;
      group.classList.remove('is-placed');
      gliding = false;
      return;
    }
    for (const option of buttons()) holdBold(option);
    const animate = fromUser && seen && motionOn();
    const box = measure(button);
    if (!box) return;
    commit(box, animate);
  };

  const live = (): boolean => {
    const DomNode = globalThis.Node;
    return typeof DomNode === 'function' && group instanceof DomNode;
  };

  if (live() && typeof MutationObserver === 'function') {
    const mo = new MutationObserver(() => {
      if (group.isConnected === false) {
        mo.disconnect();
        return;
      }
      place(true);
    });
    mo.observe(group, { subtree: true, attributes: true, attributeFilter: ['aria-checked', 'aria-pressed', 'aria-selected'] });
  }

  if (live() && typeof ResizeObserver === 'function') {
    const ro = new ResizeObserver(() => {
      if (group.isConnected === false) {
        ro.disconnect();
        return;
      }
      if (gliding) return;
      place(false);
    });
    ro.observe(group);
    for (const button of buttons()) ro.observe(button);
  }

  thumb.addEventListener('transitionend', (event) => {
    if (event.target !== thumb || !gliding) return;
    if (event.propertyName !== 'width' && event.propertyName !== 'transform') return;
    gliding = false;
    place(false);
  });

  const rects = (): { button: HTMLButtonElement; box: SegmentBox }[] => buttons().map((button) => {
    const r = button.getBoundingClientRect();
    return { button, box: { left: r.left, right: r.right, top: r.top, bottom: r.bottom, disabled: button.disabled } };
  });

  const hitButton = (x: number, y: number): HTMLButtonElement | null => {
    const items = rects();
    const host = group.getBoundingClientRect();
    const index = segmentIndex(items.map((item) => item.box), {
      left: host.left, right: host.right, top: host.top, bottom: host.bottom, disabled: false,
    }, x, y);
    return index < 0 ? null : items[index]?.button ?? null;
  };

  const buttonFrom = (target: EventTarget | null): HTMLButtonElement | null => {
    if (!target || typeof target !== 'object') return null;
    let node = target as Node;
    // A click on the label hits its text node, whose tagName is not the button's.
    if (node.nodeType === 3 && node.parentElement) node = node.parentElement;
    const element = node as Element;
    const found = typeof element.closest === 'function' ? element.closest('button') : element;
    if (!found || !group.contains(found)) return null;
    const tag = found.tagName;
    if (tag !== 'BUTTON' && tag !== 'button') return null;
    return found as HTMLButtonElement;
  };

  const pressAt = (x: number, y: number): void => {
    if (gliding || pointer?.dragged) {
      group.classList.remove('is-pressed');
      return;
    }
    const hit = hitButton(x, y);
    group.classList.toggle('is-pressed', hit !== null && checked(hit));
  };

  const doc = (): Document => group.ownerDocument ?? document;

  const endPointer = (event: PointerEvent): void => {
    if (!pointer || event.pointerId !== pointer.id) return;
    const dragged = pointer.dragged;
    pointer = null;
    group.classList.remove('is-pressed');
    const root = doc();
    root.removeEventListener('pointermove', onMove);
    root.removeEventListener('pointerup', endPointer);
    root.removeEventListener('pointercancel', endPointer);
    if (!dragged) return;
    // The click that follows pointerup must not select again. If the pointer ended on another
    // segment, the browser may not fire a click at all, so the flag clears on its own next turn.
    suppressClick = true;
    setTimeout(() => { suppressClick = false; }, 0);
  };

  const onMove = (event: PointerEvent): void => {
    if (!pointer || event.pointerId !== pointer.id) return;
    const dx = event.clientX - pointer.x;
    const dy = event.clientY - pointer.y;
    if (!pointer.dragged && dx * dx + dy * dy < 16) return;
    pointer.dragged = true;
    const hit = hitButton(event.clientX, event.clientY);
    if (hit && !hit.disabled && !checked(hit)) {
      hit.focus({ preventScroll: true });
      hit.click();
    }
    pressAt(event.clientX, event.clientY);
  };

  group.addEventListener('pointerdown', (event) => {
    if (event.button !== 0 || pointer) return;
    const target = buttonFrom(event.target);
    if (target?.disabled) return;
    if (!target && event.target !== group) return;
    pointer = { id: event.pointerId, x: event.clientX, y: event.clientY, dragged: false };
    pressAt(event.clientX, event.clientY);
    const root = doc();
    root.addEventListener('pointermove', onMove);
    root.addEventListener('pointerup', endPointer);
    root.addEventListener('pointercancel', endPointer);
  });

  group.addEventListener('click', (event) => {
    if (!suppressClick) return;
    suppressClick = false;
    event.preventDefault();
    event.stopPropagation();
  }, true);

  place(false);
}
