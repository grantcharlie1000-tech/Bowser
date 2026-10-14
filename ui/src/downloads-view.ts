// One download row, shared by the bubble (downloads-bubble.ts) and the page (downloads-page.ts).
// Built once per entry id and then patched in place: a re-post (4/s while bytes move) only
// rewrites the parts that changed, so the progress bar's transition and any running animation
// keep going. Text from Rust reaches the DOM through textContent only. Styles: downloads.css.
//
// Markup:
//   li.dl-row[data-id][data-state][data-kind](.is-unknown)
//     button.dl-open[aria-disabled]            the row body; opens a finished file
//       span.dl-tile[data-kind] > svg           file-kind glyph on its tone
//       span.dl-text > span.dl-name > span.dl-stem + span.dl-ext   (middle elision: nameParts)
//                      span.dl-meta > span.dl-status (+ page: span.dl-host)
//                      span.dl-bar > span.dl-fill  progress (aria-hidden; the status says it)
//       (page) span.dl-time                     HH:MM, gives way to the buttons on hover
//     span.dl-acts > button.bw-icon-btn.dl-act[data-act][data-tier]
import { el, icon, type IconName } from './dom';
import {
  ACTION_LABEL, SEP, actionsFor, entryChange, fileKind, fraction, hasBar, nameParts, statusText, statusTone,
  type DlAction, type DlEntry, type FileKind,
} from './downloads-logic';

export const KIND_ICON: Readonly<Record<FileKind, IconName>> = {
  image: 'image', video: 'video', audio: 'workspace-music', archive: 'archive', document: 'file-text', code: 'workspace-code', program: 'package', other: 'file',
};
export const ACTION_ICON: Readonly<Record<DlAction, IconName>> = {
  pause: 'pause', resume: 'play', cancel: 'close', open: 'external', show: 'folder-open', remove: 'close', copy: 'link', retry: 'reload',
};

export type Where = 'bubble' | 'page';

export interface RowNode {
  readonly root: HTMLLIElement;
  readonly open: HTMLButtonElement;
  readonly tile: HTMLElement;
  readonly stem: HTMLElement;
  readonly ext: HTMLElement;
  readonly status: HTMLElement;
  readonly host: HTMLElement | null;
  readonly fill: HTMLElement;
  readonly time: HTMLElement | null;
  readonly acts: HTMLElement;
  entry: DlEntry | undefined;
  /** The search the name was last marked for (page). */
  marked: string;
}

/** What a row reports: a button press (`act`) or the row body (`open`). */
export type RowAction = (node: RowNode, action: DlAction, event: Event) => void;

export function makeRow(where: Where, onAction: RowAction): RowNode {
  const root = el('li', 'dl-row');
  const open = el('button', 'dl-open');
  open.type = 'button';
  const tile = el('span', 'dl-tile');
  tile.setAttribute('aria-hidden', 'true');
  const text = el('span', 'dl-text');
  const name = el('span', 'dl-name');
  const stem = el('span', 'dl-stem');
  const ext = el('span', 'dl-ext');
  stem.dir = 'auto';
  name.append(stem, ext);
  const meta = el('span', 'dl-meta');
  const status = el('span', 'dl-status');
  meta.append(status);
  let host: HTMLElement | null = null;
  if (where === 'page') {
    host = el('span', 'dl-host');
    host.dir = 'auto';
    meta.append(host);
  }
  const bar = el('span', 'dl-bar');
  bar.setAttribute('aria-hidden', 'true');
  const fill = el('span', 'dl-fill');
  bar.append(fill);
  text.append(name, meta, bar);
  open.append(tile, text);
  let time: HTMLElement | null = null;
  if (where === 'page') {
    time = el('span', 'dl-time');
    open.append(time);
  }
  const acts = el('span', 'dl-acts');
  root.append(open, acts);
  const node: RowNode = { root, open, tile, stem, ext, status, host, fill, time, acts, entry: undefined, marked: '\u0000' };
  open.addEventListener('click', (event) => onAction(node, 'open', event));
  acts.addEventListener('click', (event) => {
    const button = event.target instanceof Element ? event.target.closest<HTMLButtonElement>('.dl-act') : null;
    const action = button?.dataset.act as DlAction | undefined;
    if (!button || !action) return;
    event.stopPropagation();
    onAction(node, action, event);
  });
  // A status change (finished, failed) crossfades the line once; ticks only rewrite it.
  status.addEventListener('animationend', () => status.classList.remove('is-swap'));
  return node;
}

function actButton(action: DlAction, name: string, tier: 'primary' | 'secondary', where: Where): HTMLButtonElement {
  const button = el('button', 'bw-icon-btn is-lg is-round dl-act');
  button.type = 'button';
  button.dataset.act = action;
  button.dataset.tier = tier;
  button.setAttribute('aria-label', `${ACTION_LABEL[action]} ${name}`);
  if (action === 'remove' && where === 'page') button.setAttribute('aria-keyshortcuts', 'Delete');
  button.append(icon(ACTION_ICON[action]));
  return button;
}

/** The keyboard parts of a row, left to right: the body, then the buttons. */
export function rowParts(node: RowNode): HTMLButtonElement[] {
  return [node.open, ...Array.from(node.acts.querySelectorAll<HTMLButtonElement>('.dl-act'))];
}

/**
 * Writes `entry` into `node`, touching only what changed since the last fill. `mark` paints the
 * search hits into the name (page); returns true when the state changed (for announcements).
 */
export function fillRow(node: RowNode, entry: DlEntry, where: Where, mark?: (target: HTMLElement, text: string) => void, query = ''): boolean {
  const prev = node.entry;
  const change = entryChange(prev, entry, where);
  node.entry = entry;
  const root = node.root;
  if (root.dataset.id !== String(entry.id)) root.dataset.id = String(entry.id);

  if (change.name || node.marked !== query) {
    node.marked = query;
    const { head, tail } = nameParts(entry.name);
    if (mark) { mark(node.stem, head); mark(node.ext, tail); }
    else { node.stem.textContent = head; node.ext.textContent = tail; }
    const kind = fileKind(entry.name);
    if (root.dataset.kind !== kind) {
      root.dataset.kind = kind;
      node.tile.dataset.kind = kind;
      node.tile.replaceChildren(icon(KIND_ICON[kind], { size: where === 'page' ? 20 : 16 }));
    }
    if (node.host) {
      if (mark) mark(node.host, entry.host);
      else node.host.textContent = entry.host;
      node.host.hidden = entry.host === '';
    }
    if (node.time && node.time.textContent !== entry.clock) node.time.textContent = entry.clock;
  }

  if (change.state) {
    root.dataset.state = entry.state;
    node.status.dataset.tone = statusTone(entry.state);
    const openable = entry.state === 'done';
    node.open.setAttribute('aria-disabled', String(!openable));
    if (prev && prev.state !== entry.state) {
      node.status.classList.remove('is-swap');
      void node.status.offsetWidth;
      node.status.classList.add('is-swap');
    }
  }
  if (change.status || change.state) {
    const text = statusText(entry, where);
    if (node.status.textContent !== text) node.status.textContent = text;
    const label = `${entry.name}, ${text}${where === 'page' && entry.host ? `${SEP}${entry.host}` : ''}`;
    node.open.setAttribute('aria-label', entry.state === 'done' ? `Open ${entry.name}, ${text}` : label);
  }
  if (change.bar) {
    const f = fraction(entry);
    root.classList.toggle('is-unknown', entry.state === 'active' && f === null);
    // Finished rows keep the bar's box (no layout change); it fills, then collapses (downloads.css).
    const shown = hasBar(entry.state) ? f ?? 0 : 1;
    node.fill.style.setProperty('--dl-f', String(shown));
  }
  if (change.actions || change.name) {
    const focused = node.acts.contains(document.activeElement) ? (document.activeElement as HTMLElement).dataset.act : undefined;
    const { primary, secondary } = actionsFor(entry);
    // Secondary buttons sit left of the primary ones, so the always-visible ones keep the edge.
    const buttons = [...secondary.map((a) => actButton(a, entry.name, 'secondary', where)), ...primary.map((a) => actButton(a, entry.name, 'primary', where))];
    node.acts.replaceChildren(...buttons);
    root.style.setProperty('--dl-acts', String(buttons.length));
    root.style.setProperty('--dl-primary', String(primary.length));
    const keep = focused ? buttons.find((b) => b.dataset.act === focused) : undefined;
    if (focused !== undefined) (keep ?? node.open).focus({ preventScroll: true });
  }
  return change.state && prev !== undefined;
}
