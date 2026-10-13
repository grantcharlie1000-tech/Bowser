// Site information page in the popup host (`{t:"siteinfo"}`). Styles: styles/siteinfo.css.
//
// Markup contract:
//   .popup-page.bw-siteinfo                         page root (child of #app.popup-pages)
//     .bw-siteinfo-header[data-security] > svg, .bw-siteinfo-title > .bw-siteinfo-host + .bw-siteinfo-summary
//     .bw-siteinfo-notice                            "a request was blocked" (only when one was)
//     .bw-section-header
//     .bw-siteinfo-row[data-k] > svg, .bw-siteinfo-text > .bw-siteinfo-label (+ .bw-siteinfo-meta),
//         control (.bw-segmented[role=radiogroup] > button[role=radio][aria-checked] | .bw-toggle | .bw-button)
//     .bw-separator
//     .bw-siteinfo-footer > .bw-button (reset), .bw-siteinfo-policy
// Controls send `act` (the popup stays open) and Rust answers with a refreshed page.
import { icon, type IconName } from './icons';
import { sendPopupAct, sendPopupDismiss, sendPopupSize, type Security } from './protocol';
import {
  ACT_BLOCKER, ACT_CLEAR_COOKIES, ACT_RESET, ACT_ZOOM_RESET, CHOICES, CHOICE_LABEL, blockerText, cookieText,
  validSiteInfoPayload, type PermissionKey, type SiteInfoMessage,
} from './siteinfo-logic';
import type { PageView } from './palette';
import { isZoomed, zoomText } from './zoom-logic';

export const SITEINFO_WIDTH = 320;

const SECURITY_ICON: Readonly<Record<Security, IconName>> = {
  secure: 'lock', notsecure: 'unlock', certproblem: 'warning', internal: 'info', unknown: 'info', none: 'info',
};
const PERMISSION_ICON: Readonly<Record<PermissionKey, IconName>> = {
  camera: 'camera', mic: 'mic', location: 'location', notifications: 'bell', popups: 'external',
};

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className: string, text?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function button(className: string, label: string, focusKey: string): HTMLButtonElement {
  const node = el('button', className, label);
  node.type = 'button';
  node.dataset.focus = focusKey;
  return node;
}

export function createSiteInfoView(root: HTMLElement): PageView {
  let message: SiteInfoMessage | null = null;
  let closed = true;
  let lastSize = '';
  const page = el('div', 'popup-page bw-siteinfo');
  page.setAttribute('role', 'dialog');
  page.tabIndex = -1;

  const act = (id: number, n: number, control: HTMLElement): void => {
    if (!message || closed) return;
    // Rust re-sends the page; until then the controls show the request and take no more input.
    page.setAttribute('aria-busy', 'true');
    control.classList.add('is-pending');
    sendPopupAct(message.tok, id, n);
    // A dropped action (stale page) must not leave the controls locked.
    const tok = message.tok;
    setTimeout(() => {
      if (message?.tok === tok && page.getAttribute('aria-busy') === 'true') {
        page.removeAttribute('aria-busy');
        control.classList.remove('is-pending');
      }
    }, 4000);
  };

  const measure = (): void => {
    if (!message) return;
    const tok = message.tok;
    requestAnimationFrame(() => {
      if (!message || message.tok !== tok || closed) return;
      const height = Math.max(1, page.offsetHeight);
      const key = `${tok}:${height}`;
      if (key === lastSize) return;
      lastSize = key;
      sendPopupSize(tok, SITEINFO_WIDTH, height);
    });
  };

  const row = (glyph: IconName, label: string, meta: string | null, control: HTMLElement | null, key: string): HTMLElement => {
    const node = el('div', 'bw-siteinfo-row');
    node.dataset.k = key;
    const text = el('div', 'bw-siteinfo-text');
    text.append(el('span', 'bw-siteinfo-label', label));
    if (meta) text.append(el('span', 'bw-siteinfo-meta', meta));
    node.append(icon(glyph), text);
    if (control) node.append(control);
    return node;
  };

  const draw = (msg: SiteInfoMessage): void => {
    const busy = (): boolean => page.getAttribute('aria-busy') === 'true';
    const focused = document.activeElement instanceof HTMLElement ? document.activeElement.dataset.focus : undefined;
    page.removeAttribute('aria-busy');
    page.setAttribute('aria-label', `Site information for ${msg.host}`);
    const nodes: HTMLElement[] = [];

    const header = el('div', 'bw-siteinfo-header');
    header.dataset.security = msg.security;
    const title = el('div', 'bw-siteinfo-title');
    const host = el('span', 'bw-siteinfo-host', msg.host);
    host.dir = 'auto';
    title.append(host, el('span', 'bw-siteinfo-summary', msg.summary));
    header.append(icon(SECURITY_ICON[msg.security]), title);
    nodes.push(header);

    if (msg.blockedHere) {
      const notice = el('div', 'bw-siteinfo-notice');
      notice.setAttribute('role', 'status');
      notice.append(icon('shield'), el('span', '', 'This page asked for a permission and was blocked. Allow it below to use it here.'));
      nodes.push(notice);
    }

    nodes.push(el('h2', 'bw-section-header', 'Permissions'));
    if (msg.perms.length === 0) {
      nodes.push(el('p', 'bw-siteinfo-empty', 'Permissions apply to http and https pages.'));
    }
    for (const perm of msg.perms) {
      const group = el('div', 'bw-segmented');
      group.setAttribute('role', 'radiogroup');
      group.setAttribute('aria-label', perm.label);
      CHOICES.forEach((choice, n) => {
        const option = button('', CHOICE_LABEL[choice], `perm-${perm.k}-${choice}`);
        option.setAttribute('role', 'radio');
        option.setAttribute('aria-checked', String(perm.state === choice));
        if (choice === 'default') option.setAttribute('aria-description', 'Blocked: Bowser never asks');
        option.addEventListener('click', () => {
          if (busy() || perm.state === choice) return;
          for (const other of Array.from(group.querySelectorAll('button'))) other.setAttribute('aria-checked', String(other === option));
          act(perm.id, n, option);
        });
        group.append(option);
      });
      group.addEventListener('keydown', (event) => {
        if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
        event.preventDefault();
        const options = Array.from(group.querySelectorAll('button'));
        const at = options.indexOf(document.activeElement as HTMLButtonElement);
        const next = options[(at + (event.key === 'ArrowRight' ? 1 : options.length - 1)) % options.length];
        next?.focus();
      });
      nodes.push(row(PERMISSION_ICON[perm.k], perm.label, null, group, perm.k));
    }

    nodes.push(el('div', 'bw-separator'));
    let clear: HTMLButtonElement | null = null;
    if (msg.cookies.clear) {
      clear = button('bw-button is-subtle', 'Clear', 'cookies');
      clear.setAttribute('aria-label', 'Clear cookies for this site');
      const target = clear;
      clear.addEventListener('click', () => { if (!busy()) act(ACT_CLEAR_COOKIES, 0, target); });
    }
    nodes.push(row('cookie', 'Cookies', cookieText(msg.cookies), clear, 'cookies'));

    const toggle = button('bw-toggle', '', 'blocker');
    toggle.setAttribute('role', 'switch');
    toggle.setAttribute('aria-label', 'Tracker blocker on this site');
    toggle.setAttribute('aria-checked', String(msg.blocker.state === 'on'));
    toggle.disabled = msg.blocker.state === 'unavailable';
    toggle.addEventListener('click', () => {
      if (busy()) return;
      const on = toggle.getAttribute('aria-checked') !== 'true';
      toggle.setAttribute('aria-checked', String(on));
      act(ACT_BLOCKER, on ? 1 : 0, toggle);
    });
    nodes.push(row('shield', 'Tracker blocker', blockerText(msg.blocker), toggle, 'blocker'));
    nodes.push(row('eye', 'Tracking prevention', msg.tracking === '' ? 'Unavailable' : msg.tracking.charAt(0).toUpperCase() + msg.tracking.slice(1), null, 'tracking'));
    if (isZoomed(msg.zoom)) {
      const zoomReset = button('bw-button is-subtle', 'Reset', 'zoom');
      zoomReset.setAttribute('aria-label', 'Reset zoom to 100%');
      zoomReset.addEventListener('click', () => { if (!busy()) act(ACT_ZOOM_RESET, 0, zoomReset); });
      nodes.push(row(msg.zoom > 100 ? 'zoom-in' : 'zoom-out', 'Zoom', zoomText(msg.zoom), zoomReset, 'zoom'));
    }

    nodes.push(el('div', 'bw-separator'));
    const footer = el('div', 'bw-siteinfo-footer');
    const reset = button('bw-button', 'Reset permissions', 'reset');
    reset.prepend(icon('reset'));
    reset.disabled = !msg.reset;
    reset.addEventListener('click', () => { if (!busy()) act(ACT_RESET, 0, reset); });
    footer.append(reset, el('p', 'bw-siteinfo-policy', 'Sites never get a prompt. Anything not allowed here is blocked. Links you click still open in new tabs.'));
    nodes.push(footer);

    page.replaceChildren(...nodes);
    const restore = focused ? page.querySelector<HTMLElement>(`[data-focus="${CSS.escape(focused)}"]`) : null;
    if (restore && !(restore as HTMLButtonElement).disabled) restore.focus({ preventScroll: true });
    else if (!page.contains(document.activeElement)) page.focus({ preventScroll: true });
    measure();
  };

  page.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') {
      event.preventDefault();
      if (message && !closed) {
        closed = true;
        sendPopupDismiss(message.tok);
      }
    }
    // Keys stay on this page; the menu view listening on #app never sees them.
    event.stopPropagation();
  });
  for (const type of ['mousedown', 'mouseup', 'contextmenu'] as const) {
    page.addEventListener(type, (event) => {
      if (type === 'contextmenu') event.preventDefault();
      event.stopPropagation();
    });
  }

  return {
    show(raw: unknown): boolean {
      if (!validSiteInfoPayload(raw)) return false;
      const refresh = message !== null && message.tok === raw.tok && !closed;
      message = raw;
      if (!refresh) {
        closed = false;
        lastSize = '';
        root.className = 'popup-pages';
        root.removeAttribute('role');
        root.removeAttribute('aria-activedescendant');
        root.removeAttribute('aria-label');
        page.classList.remove('bw-enter', 'is-down');
        root.replaceChildren(page);
        void page.offsetWidth;
        page.classList.add('bw-enter', 'is-down');
      }
      draw(raw);
      if (!refresh) page.focus({ preventScroll: true });
      return true;
    },
  };
}
