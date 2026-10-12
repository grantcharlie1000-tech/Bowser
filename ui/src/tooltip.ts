// --- hover layer ---
// Tooltips for the toolbar, tab strip and rail. Elements carry their text in `data-tip` (never
// `title`, whose native tooltip is unstyled and clipped to the view); the view reports the hovered
// element's rectangle and text to Rust, which shows the tooltip in the hover layer above every view.
import { sendHover, sendTip, sendTipOff, type HoverTargetKind } from './protocol';
import { HoverReport, TooltipTiming } from './tooltip-logic';

/** Attribute holding an element's tooltip text. */
export const TIP_ATTR = 'data-tip';

/** Sets (or clears, for '') an element's tooltip text. */
export function setTip(element: HTMLElement, text: string): void {
  if (text === '') element.removeAttribute(TIP_ATTR);
  else if (element.getAttribute(TIP_ATTR) !== text) element.setAttribute(TIP_ATTR, text);
}

const clock = {
  now: (): number => performance.now(),
  set: (fn: () => void, ms: number): number => window.setTimeout(fn, ms),
  clear: (id: number): void => window.clearTimeout(id),
};

function tipTarget(node: EventTarget | null, root: HTMLElement): HTMLElement | null {
  if (!(node instanceof Element)) return null;
  const found = node.closest<HTMLElement>(`[${TIP_ATTR}]`);
  return found && root.contains(found) ? found : null;
}

/** Wires `data-tip` tooltips under `root`. Call once per view. */
export function installTooltips(root: HTMLElement): void {
  const timing = new TooltipTiming<HTMLElement>(
    clock,
    (target) => {
      const text = target.getAttribute(TIP_ATTR) ?? '';
      if (!target.isConnected || !sendTip(text, target.getBoundingClientRect())) timing.dismiss();
    },
    () => { sendTipOff(); },
  );
  root.addEventListener('pointerover', (ev) => {
    // A pressed button means a drag or a click is in progress: no tooltip.
    if (ev.buttons !== 0) return;
    const target = tipTarget(ev.target, root);
    if (target) timing.enter(target);
    else timing.leave();
  });
  root.addEventListener('pointerout', (ev) => {
    if (!tipTarget(ev.relatedTarget, root)) timing.leave();
  });
  document.documentElement.addEventListener('pointerleave', () => timing.leave());
  const dismiss = (): void => timing.dismiss();
  window.addEventListener('pointerdown', dismiss, true);
  window.addEventListener('keydown', dismiss, true);
  window.addEventListener('wheel', dismiss, { capture: true, passive: true });
  window.addEventListener('scroll', dismiss, true);
  window.addEventListener('blur', dismiss);
}

/** The tab (horizontal `.tab` or vertical `.side-tab`) or group ball under `node`, if any. */
export function tabStripHoverTarget(node: Element): { kind: HoverTargetKind; id: number; element: HTMLElement } | null {
  const ball = node.closest<HTMLElement>('.ball[data-key], .side-group-ball[data-key]');
  if (ball) return { kind: 'ball', id: Number(ball.dataset.key), element: ball };
  // The close button belongs to its tab, so the card still opens over it.
  const tab = node.closest<HTMLElement>('.tab[data-id], .side-tab[data-id]');
  if (tab) return { kind: 'tab', id: Number(tab.dataset.id), element: tab };
  return null;
}

/**
 * Reports which tab or group ball the pointer is over (for the hover card and island menu), once
 * per change. `resolve` maps an event target to a target kind, id and element, or null for none.
 */
export function installHoverTargets(
  root: HTMLElement,
  resolve: (node: Element) => { kind: HoverTargetKind; id: number; element: HTMLElement } | null,
): void {
  const report = new HoverReport();
  const none = (): void => {
    if (report.changed('')) sendHover('none');
  };
  root.addEventListener('pointerover', (ev) => {
    if (ev.buttons !== 0 || !(ev.target instanceof Element)) {
      none();
      return;
    }
    const target = resolve(ev.target);
    if (!target) {
      none();
      return;
    }
    if (report.changed(`${target.kind}:${target.id}`)) {
      if (!sendHover(target.kind, target.id, target.element.getBoundingClientRect())) report.reset();
    }
  });
  document.documentElement.addEventListener('pointerleave', none);
  window.addEventListener('pointerdown', none, true);
  window.addEventListener('blur', none);
}
// --- end hover layer ---
