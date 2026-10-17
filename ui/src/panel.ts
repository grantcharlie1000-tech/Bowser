// --- panel host (lane P) ---
// The panel page (?view=panel) is shown as a rail flyout (docs/design/flyouts.md): an opaque
// surface with a caret toward its rail button, inside a transparent window whose margin holds the
// surface's shadow. Rust sizes and places the window; the page draws the frame, renders the panel
// (panels/*.ts) inside it and reports when it has painted so the window is shown with content.
import { onInbound, send, sendPanel, type Inbound, type PanelToken } from './protocol';
import { el } from './dom';
import { applyTheme } from './view';
import { installTooltips } from './tooltip';
import { panels } from './panels';
import { shouldDismissPanelEscape } from './panels/history-logic';

const FLYOUT_NAMES: Partial<Record<PanelToken, string>> = { history: 'History', bookmarks: 'Bookmarks', customize: 'Customize' };

/**
 * Where focus goes on open: the search field (History, Bookmarks), else the surface itself
 * (Customize), so Tab reaches the first control without a focus ring drawn on a switch the user
 * did not choose.
 */
function primaryControl(surface: HTMLElement): HTMLElement {
  return surface.querySelector<HTMLElement>('input[type="search"]') ?? surface;
}

export function startPanel(root: HTMLElement): void {
  root.classList.add('panel-root', 'fly-root');
  installTooltips(root);
  const frame = el('div', 'fly');
  const caret = el('span', 'fly-caret');
  caret.setAttribute('aria-hidden', 'true');
  caret.hidden = true;
  const surface = el('div', 'fly-surface');
  surface.setAttribute('role', 'dialog');
  surface.tabIndex = -1;
  frame.append(surface, caret);
  root.replaceChildren(frame);

  const dismiss = (): void => { sendPanel('panel', { a: 'dismiss' }); };
  root.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && shouldDismissPanelEscape(event.defaultPrevented)) {
      event.preventDefault();
      dismiss();
    }
  });
  // The window's transparent margin (the shadow room) is outside the flyout: a press there closes it.
  root.addEventListener('pointerdown', (event) => {
    if (event.target === root || event.target === frame) dismiss();
  });

  const focusPrimary = (): void => {
    if (surface.contains(document.activeElement) && document.activeElement !== surface) return;
    primaryControl(surface).focus({ preventScroll: true });
  };
  window.addEventListener('focus', focusPrimary);

  let active: PanelToken | undefined;
  let receive: ((message: Inbound) => void) | undefined;
  // Set by `flyout` (an open or a switch): the next snapshot ends with `shown` once it is drawn.
  let opening = false;
  const reportShown = (): void => {
    requestAnimationFrame(() => requestAnimationFrame(() => {
      sendPanel('panel', { a: 'shown' });
      focusPrimary();
    }));
  };

  onInbound((message) => {
    switch (message.t) {
      case 'state':
        applyTheme(message.theme, message.solid, message.dark, undefined, undefined, message.density, message.compactMenus, message.look);
        return;
      case 'flyout': {
        caret.hidden = message.caret < 0;
        if (message.caret >= 0) frame.style.setProperty('--fly-caret-y', `${message.caret}px`);
        // Restart the enter motion (opacity + slide from the rail side).
        frame.classList.remove('is-entering');
        void frame.offsetWidth;
        frame.classList.add('is-entering');
        opening = true;
        return;
      }
      case 'panelshow':
        if (active !== message.p) {
          active = message.p;
          surface.dataset.panel = message.p;
          surface.setAttribute('aria-label', FLYOUT_NAMES[message.p] ?? 'Panel');
          receive = panels[message.p]?.(surface);
        }
        return;
      case 'snap':
        if (message.p !== active) return;
        receive?.(message);
        if (opening) {
          opening = false;
          reportShown();
        }
        return;
    }
  });
  send({ v: 1, t: 'ready' });
}
// --- end panel host ---
