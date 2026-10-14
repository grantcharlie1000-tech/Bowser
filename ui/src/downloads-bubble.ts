// Download bubble in the popup host (`{t:"downloads"}`, docs/design/downloads.md "Bubble").
// Styles: styles/downloads.css. Rust re-posts the page under the same `tok` on every change (4/s
// while bytes move); a re-post patches rows by id and never rebuilds them, so bars glide and
// animations run on. A new `tok` is a new opening: the page is rebuilt and enters again.
//
// Markup contract:
//   .popup-page.bw-dl[role=dialog]                    page root (child of #app.popup-pages)
//     header.db-head > h2.db-title, span.db-sub, button.bw-icon-btn.db-folder
//     ul.db-list > li.dl-row ... (downloads-view.ts)
//     p.db-empty                                     no downloads to list
//     button.db-foot > span.db-foot-label, span.db-more, kbd.db-key
//     p.db-sr[role=status]                           polite announcements
// Sends `size` (natural height), `dismiss` and `dlact` only.
import { el, icon } from './dom';
import { sendDownloadAct, sendPopupDismiss, sendPopupSize } from './protocol';
import {
  ACTION_CODE, ActionGate, LIST_ACTION, announceText, canAct, diffIds, moreText, parseBubble, removable,
  type BubbleMessage, type DlAction,
} from './downloads-logic';
import { fillRow, makeRow, rowParts, type RowNode } from './downloads-view';
import type { PageView } from './palette';

export const BUBBLE_WIDTH = 360;
/** An auto-opened bubble closes itself after this long untouched (docs/design/downloads.md). */
export const AUTO_CLOSE_MS = 6000;
/** ... and this long after the pointer leaves it. */
export const AUTO_CLOSE_AFTER_LEAVE_MS = 3000;

const reduceMotion = (): boolean => matchMedia('(prefers-reduced-motion: reduce)').matches;

export function createDownloadsView(root: HTMLElement): PageView {
  let message: BubbleMessage | null = null;
  let closed = true;
  let lastSize = '';
  let autoTimer = 0;
  /** The user touched the bubble (key, press, focus): it stays until they close it. */
  let engaged = false;
  let hovered = false;
  const gate = new ActionGate();

  const page = el('div', 'popup-page bw-dl');
  page.setAttribute('role', 'dialog');
  page.setAttribute('aria-labelledby', 'db-title');
  page.tabIndex = -1;
  const head = el('header', 'db-head');
  const title = el('h2', 'db-title');
  title.id = 'db-title';
  title.textContent = 'Downloads';
  const sub = el('span', 'db-sub');
  const folder = el('button', 'bw-icon-btn is-lg is-round db-folder');
  folder.type = 'button';
  folder.setAttribute('aria-label', 'Open downloads folder');
  folder.append(icon('folder-download'));
  head.append(title, sub, folder);
  const list = el('ul', 'db-list');
  list.setAttribute('aria-label', 'Recent downloads');
  const empty = el('p', 'db-empty');
  empty.textContent = 'No recent downloads';
  const foot = el('button', 'db-foot');
  foot.type = 'button';
  foot.setAttribute('aria-keyshortcuts', 'Control+J');
  const footLabel = el('span', 'db-foot-label');
  footLabel.textContent = 'Full download history';
  const more = el('span', 'db-more');
  const key = el('kbd', 'db-key');
  key.textContent = 'Ctrl+J';
  foot.append(icon('clock', { size: 14 }), footLabel, more, key);
  const live = el('p', 'db-sr');
  live.setAttribute('role', 'status');
  live.setAttribute('aria-live', 'polite');
  page.append(head, list, empty, foot, live);

  const rows = new Map<number, RowNode>();

  // ---- sending ----
  const listAct = (n: number): void => {
    if (!message || closed || !gate.take(`0:${n}`)) return;
    sendDownloadAct(message.tok, 0, n);
  };
  const rowAct = (node: RowNode, action: DlAction): void => {
    const entry = node.entry;
    if (!message || closed || !entry || !canAct(entry, action)) return;
    if (!gate.take(`${entry.id}:${action}`)) return;
    engage();
    sendDownloadAct(message.tok, entry.id, ACTION_CODE[action]);
  };
  folder.addEventListener('click', () => listAct(LIST_ACTION.folder));
  foot.addEventListener('click', () => listAct(LIST_ACTION.page));

  // ---- size and the auto-close timer ----
  const measure = (): void => {
    if (!message) return;
    const tok = message.tok;
    requestAnimationFrame(() => {
      if (!message || message.tok !== tok || closed) return;
      // Layout height: entrance and glide transforms never change it.
      const height = Math.max(1, page.offsetHeight);
      const sizeKey = `${tok}:${height}`;
      if (sizeKey === lastSize) return;
      lastSize = sizeKey;
      sendPopupSize(tok, BUBBLE_WIDTH, height);
    });
  };
  const dismiss = (fade: boolean): void => {
    if (!message || closed) return;
    const tok = message.tok;
    window.clearTimeout(autoTimer);
    if (!fade || reduceMotion()) { closed = true; sendPopupDismiss(tok); return; }
    // An auto-close fades out first (83ms); a click inside meanwhile keeps it open.
    page.classList.add('bw-exit');
    window.setTimeout(() => {
      if (message?.tok !== tok || closed || !page.classList.contains('bw-exit')) return;
      closed = true;
      sendPopupDismiss(tok);
    }, 90);
  };
  const arm = (delay: number): void => {
    window.clearTimeout(autoTimer);
    if (!message?.auto || engaged || hovered || closed) return;
    autoTimer = window.setTimeout(() => dismiss(true), delay);
  };
  function engage(): void {
    engaged = true;
    window.clearTimeout(autoTimer);
    page.classList.remove('bw-exit');
  }
  page.addEventListener('pointerenter', () => { hovered = true; window.clearTimeout(autoTimer); page.classList.remove('bw-exit'); });
  page.addEventListener('pointerleave', () => { hovered = false; arm(AUTO_CLOSE_AFTER_LEAVE_MS); });
  page.addEventListener('pointerdown', engage);
  page.addEventListener('focusin', (event) => { if (event.target !== page) engage(); });

  // ---- rows ----
  const onRow = (node: RowNode, action: DlAction | 'open'): void => {
    const entry = node.entry;
    if (!entry) return;
    if (action === 'open') { if (entry.state === 'done') rowAct(node, 'open'); return; }
    rowAct(node, action);
  };

  const render = (msg: BubbleMessage, fresh: boolean): void => {
    const animate = !fresh && !reduceMotion();
    const before = new Map<HTMLElement, number>();
    if (animate) for (const node of [...[...rows.values()].map((r): HTMLElement => r.root), foot]) before.set(node, node.getBoundingClientRect().top);
    const prevIds = [...rows.keys()];
    const ids = msg.items.map((entry) => entry.id);
    const { added, removed } = diffIds(prevIds, ids);
    const announce: string[] = [];

    // Leaving rows come out of the flow at once (so the rest can glide up) and fade where they were.
    for (const id of removed) {
      const node = rows.get(id);
      rows.delete(id);
      if (!node) continue;
      if (animate) {
        const top = node.root.offsetTop;
        node.root.style.setProperty('--dl-top', `${top}px`);
        node.root.classList.add('is-leaving');
        node.root.addEventListener('animationend', () => node.root.remove(), { once: true });
        window.setTimeout(() => node.root.remove(), 400);
      } else {
        node.root.remove();
      }
    }
    const items: HTMLElement[] = [];
    for (const entry of msg.items) {
      let node = rows.get(entry.id);
      if (!node) {
        node = makeRow('bubble', (n, action) => onRow(n, action));
        rows.set(entry.id, node);
        if (animate && added.includes(entry.id)) node.root.classList.add('is-new');
      }
      if (fillRow(node, entry, 'bubble') && (entry.state === 'done' || entry.state === 'failed')) announce.push(announceText(entry));
      items.push(node.root);
    }
    // Place live rows in order; leaving rows stay as absolutely placed ghosts after them.
    const ghosts = Array.from(list.children).filter((child) => child.classList.contains('is-leaving')) as HTMLElement[];
    let ref: ChildNode | null = list.firstChild;
    for (const item of items) {
      if (item === ref) { ref = ref.nextSibling; continue; }
      list.insertBefore(item, ref);
    }
    for (const ghost of ghosts) list.append(ghost);

    const running = msg.active;
    sub.textContent = running > 0 ? (running === 1 ? '1 in progress' : `${running} in progress`) : '';
    sub.hidden = running === 0;
    empty.hidden = msg.items.length > 0;
    more.textContent = moreText(msg.more);
    more.hidden = msg.more === 0;
    if (announce.length > 0) live.textContent = announce.join('. ');

    if (animate) {
      for (const [node, top] of before) {
        if (!node.isConnected || node.classList.contains('is-leaving')) continue;
        const delta = top - node.getBoundingClientRect().top;
        if (Math.abs(delta) < 1) continue;
        node.classList.remove('is-gliding');
        node.style.setProperty('--dl-from', `${delta}px`);
        void node.offsetWidth;
        node.classList.add('is-gliding');
      }
    }
    measure();
  };
  page.addEventListener('animationend', (event) => {
    const node = event.target instanceof HTMLElement ? event.target : null;
    if (!node) return;
    if (event.animationName === 'dl-glide') node.classList.remove('is-gliding');
    if (event.animationName === 'dl-arrive') node.classList.remove('is-new');
  });

  // ---- keyboard ----
  const stops = (): HTMLButtonElement[] => [...[...rows.values()].map((node) => node.open), foot];
  const rowOf = (target: EventTarget | null): RowNode | undefined => {
    const item = target instanceof HTMLElement ? target.closest<HTMLElement>('.dl-row') : null;
    return item ? rows.get(Number(item.dataset.id)) : undefined;
  };
  page.addEventListener('keydown', (event) => {
    if (event.isComposing) return;
    engage();
    const node = rowOf(event.target);
    switch (event.key) {
      case 'Escape':
        event.preventDefault();
        dismiss(false);
        break;
      case 'ArrowDown':
      case 'ArrowUp':
      case 'Home':
      case 'End': {
        event.preventDefault();
        const all = stops();
        const at = node ? all.indexOf(node.open) : all.indexOf(event.target as HTMLButtonElement);
        const last = all.length - 1;
        const next = event.key === 'Home' ? 0 : event.key === 'End' ? last
          : at < 0 ? (event.key === 'ArrowDown' ? 0 : last)
            : Math.max(0, Math.min(last, at + (event.key === 'ArrowDown' ? 1 : -1)));
        all[next]?.focus();
        break;
      }
      case 'ArrowLeft':
      case 'ArrowRight': {
        if (!node) break;
        const parts = rowParts(node);
        const at = parts.indexOf(event.target as HTMLButtonElement);
        if (at < 0) break;
        event.preventDefault();
        parts[Math.max(0, Math.min(parts.length - 1, at + (event.key === 'ArrowRight' ? 1 : -1)))]?.focus();
        break;
      }
      case 'Delete':
        if (node?.entry && removable(node.entry.state)) { event.preventDefault(); rowAct(node, 'remove'); }
        break;
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
      const msg = parseBubble(raw);
      if (!msg) return false;
      const fresh = message === null || message.tok !== msg.tok || closed;
      message = msg;
      gate.clear();
      if (fresh) {
        closed = false;
        engaged = false;
        hovered = false;
        lastSize = '';
        rows.clear();
        list.replaceChildren();
        live.textContent = '';
        root.className = 'popup-pages';
        root.removeAttribute('role');
        root.removeAttribute('aria-activedescendant');
        root.removeAttribute('aria-label');
        page.classList.remove('bw-enter', 'is-down', 'bw-exit');
        root.replaceChildren(page);
        void page.offsetWidth;
        page.classList.add('bw-enter', 'is-down');
      }
      render(msg, fresh);
      if (fresh) {
        // Opened by a click: the keyboard comes here. Opened by a download start: the page keeps it.
        if (!msg.auto) page.focus({ preventScroll: true });
        arm(AUTO_CLOSE_MS);
      }
      return true;
    },
  };
}
