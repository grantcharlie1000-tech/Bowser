// --- toolbars and feeds (thread T13) ---
// The toolbar customiser, docked beside the page (src/win32/toolbar_panel.rs; ?view=dialog&p=toolbar).
// Every button the address bar and the tab bar can show is one row with a switch, grouped by where it sits.
// Buttons that are on come first in each group, in the bar's order, with a grip to drag them (Alt+Up and
// Alt+Down do the same from the keyboard); buttons that are off follow. Commands that are off are listed
// last, with a filter. Reset to default is at the bottom.
//
// Every change is the whole new list of one bar (`tbar` `set`), applied by Rust at once, so the bar beside
// the panel changes while the switch is still moving. The page shows the change straight away and the next
// snapshot settles it (a refused change springs back).
//
// Motion (transform and opacity only; none under prefers-reduced-motion): rows glide to their new place when
// a switch or a drag reorders them (167ms move ease), a glyph pops when its button comes on, a switch that
// cannot come on shakes, the reset glyph turns once, and the list settles in when the page first fills.
// Text reaches the DOM through textContent only. Styles: styles/panels.css `.tbp-*`.
import { el, icon, type IconName } from '../dom';
import { sendPanel, type Inbound } from '../protocol';
import { panelHeader, searchField, segmented } from './kit';
import {
  BAR_KEYS, ITEM_LABELS, addItem, isCommandItem, itemIcon, itemLabel, itemsOf, moveItem, removeItem, whyNotAdd, zoneOf,
  DEFAULTS, MAX_COMMAND_ITEMS, type BarId,
} from '../bars-logic';

/** `glyph`: the icon Rust names for the command (`toolbar_prefs::command_glyph`), when it does. */
interface Command { readonly id: string; readonly label: string; readonly chain: boolean; readonly glyph?: string }
interface Snapshot { readonly address: readonly string[]; readonly tabs: readonly string[]; readonly commands: readonly Command[] }

type Bar = Extract<BarId, 'address' | 'tabs'>;
interface Group { readonly key: string; readonly title: string; readonly fixed: readonly string[]; readonly accepts: (id: string) => boolean }

const GROUPS: Readonly<Record<Bar, readonly Group[]>> = {
  address: [
    { key: 'left', title: 'Before the address field', fixed: ['back', 'forward', 'reload'], accepts: (id) => zoneOf(id) === 'left' },
    { key: 'pill', title: 'In the address field', fixed: ['site-info', 'split', 'reader'], accepts: (id) => zoneOf(id) === 'pill' },
    { key: 'right', title: 'After the address field', fixed: ['bookmark', 'newtab', 'downloads', 'extensions'], accepts: (id) => zoneOf(id) === 'right' },
  ],
  tabs: [
    { key: 'tabs', title: 'At the end of the tab bar', fixed: ['closed-tabs'], accepts: () => true },
  ],
};
const BAR_NAMES: Readonly<Record<Bar, string>> = { address: 'the address bar', tabs: 'the tab bar' };

const MOVE_MS = 167;
const MOVE_EASE = 'cubic-bezier(0.55, 0.55, 0, 1)';

/** The snapshot, or null when it is not the customiser's shape. Lists are re-read with `itemsOf`. */
export function parseToolbarSnapshot(d: unknown): Snapshot | null {
  if (typeof d !== 'object' || d === null) return null;
  const raw = d as Record<string, unknown>;
  if (typeof raw.address !== 'string' || typeof raw.tabs !== 'string' || !Array.isArray(raw.commands)) return null;
  const commands: Command[] = [];
  for (const row of raw.commands.slice(0, 512)) {
    if (typeof row !== 'object' || row === null) return null;
    const { id, label, chain, glyph } = row as Record<string, unknown>;
    if (typeof id !== 'string' || typeof label !== 'string' || typeof chain !== 'boolean' || id.length > 64 || label.length > 128) return null;
    if (glyph !== undefined && (typeof glyph !== 'string' || !/^[a-z0-9-]{1,32}$/.test(glyph))) return null;
    commands.push(typeof glyph === 'string' ? { id, label, chain, glyph } : { id, label, chain });
  }
  return { address: itemsOf(raw.address), tabs: itemsOf(raw.tabs), commands };
}

/** The item id of a command row. */
const itemOf = (command: Command): string => (command.chain ? command.id : `cmd:${command.id}`);

/** The rows of one group in display order: the bar's own items there in list order, then the group's fixed items that are off. */
export function groupRows(group: Group, items: readonly string[]): { on: string[]; off: string[] } {
  const on = items.filter((id) => group.accepts(id));
  const off = group.fixed.filter((id) => !items.includes(id));
  return { on, off };
}

const reducedMotion = (): boolean => typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;

export function startToolbarPanel(root: HTMLElement): (message: Inbound) => void {
  let snapshot: Snapshot | null = null;
  /** What the page shows: the snapshot, or the change it just sent (until the next snapshot). */
  const lists: Record<Bar, string[]> = { address: [], tabs: [] };
  let bar: Bar = 'address';
  let filter = '';
  let refocus: string | null = null;
  let entered = false;
  /** The icons Rust named, by item id (from the snapshot's commands). */
  let glyphs = new Map<string, string>();

  const shell = el('section', 'pn tbp');
  shell.setAttribute('aria-label', 'Customise toolbar');
  const header = panelHeader('Customise toolbar', 'Close');
  const switcher = segmented<Bar>([['address', 'Address bar'], ['tabs', 'Tab bar']], bar, (next) => {
    bar = next;
    note.textContent = '';
    paint({ swap: true });
  }, 'Bar to customise');
  switcher.classList.add('tbp-switch');
  const body = el('div', 'tbp-body bw-scroll');
  const card = el('div', 'tbp-card');
  const commandsCard = el('div', 'tbp-card');
  const commandsHead = el('div', 'tbp-group-head');
  const commandsTitle = el('h2', 'tbp-group');
  commandsTitle.id = 'tbp-commands-h';
  const commandsCount = el('span', 'tbp-count');
  commandsHead.append(commandsTitle, commandsCount);
  const search = searchField('Filter commands');
  search.root.classList.add('tbp-filter');
  search.input.addEventListener('input', () => { filter = search.input.value.trim().toLocaleLowerCase(); paint({}); });
  const commandList = el('div', 'tbp-list');
  commandList.setAttribute('role', 'list');
  commandList.setAttribute('aria-labelledby', commandsTitle.id);
  commandsCard.append(commandsHead, search.root, commandList);
  const note = el('p', 'tbp-note');
  note.setAttribute('role', 'status');
  body.append(switcher, card, commandsCard);
  const foot = el('footer', 'tbp-foot');
  const reset = el('button', 'bw-button tbp-reset');
  reset.type = 'button';
  const resetGlyph = el('span', 'tbp-reset-glyph');
  resetGlyph.append(icon('reset'));
  reset.append(resetGlyph, document.createTextNode('Reset to default'));
  reset.addEventListener('click', () => {
    replay(resetGlyph, 'is-turning');
    send(bar, [...DEFAULTS[bar]], null);
  });
  foot.append(note, reset);
  shell.append(header.root, body, foot);
  root.replaceChildren(shell);

  // ---- sending ----
  const send = (which: Bar, next: string[] | null, focus: string | null): void => {
    if (next === null) return;
    refocus = focus;
    lists[which] = next;
    sendPanel('tbar', { a: 'set', text: `${BAR_KEYS[which]}=${next.join(',')}` });
    paint({});
  };
  const turnOn = (id: string, from: HTMLElement): void => {
    const items = lists[bar];
    const next = addItem(bar, items, id);
    if (next === null) {
      note.textContent = whyNotAdd(bar, items, id) ?? 'That button cannot be added.';
      replay(from, 'is-refused');
      paint({});
      return;
    }
    note.textContent = '';
    popped = id;
    send(bar, next, `switch:${id}`);
  };
  const turnOff = (id: string): void => {
    note.textContent = '';
    send(bar, removeItem(lists[bar], id), `switch:${id}`);
  };
  /** Moves `id` to sit just before `before` (or last when null) within its group. */
  const moveTo = (id: string, before: string | null): void => {
    const items = lists[bar].filter((other) => other !== id);
    const at = before === null ? -1 : items.indexOf(before);
    if (at < 0) {
      // Last of its group: after the group's last item.
      const group = GROUPS[bar].find((g) => g.accepts(id));
      let end = -1;
      items.forEach((other, index) => { if (group?.accepts(other)) end = index; });
      items.splice(end + 1, 0, id);
    } else items.splice(at, 0, id);
    send(bar, items, `switch:${id}`);
  };

  // ---- motion helpers ----
  let popped: string | null = null;
  function replay(node: HTMLElement, cls: string): void {
    node.classList.remove(cls);
    void node.offsetWidth;
    node.classList.add(cls);
    node.addEventListener('animationend', () => node.classList.remove(cls), { once: true });
  }
  /** Where each row is before a repaint, so rows that moved can glide from there (FLIP). */
  const positions = (): Map<string, number> => {
    const out = new Map<string, number>();
    for (const row of Array.from(shell.querySelectorAll<HTMLElement>('.tbp-row[data-item]'))) out.set(row.dataset.item ?? '', row.getBoundingClientRect().top);
    return out;
  };
  const glide = (before: Map<string, number>): void => {
    if (reducedMotion()) return;
    for (const row of Array.from(shell.querySelectorAll<HTMLElement>('.tbp-row[data-item]'))) {
      const was = before.get(row.dataset.item ?? '');
      if (was === undefined) continue;
      const dy = was - row.getBoundingClientRect().top;
      if (Math.abs(dy) < 1 || typeof row.animate !== 'function') continue;
      row.animate([{ transform: `translateY(${dy}px)` }, { transform: 'none' }], { duration: MOVE_MS, easing: MOVE_EASE });
    }
  };

  // ---- rows ----
  const row = (id: string, label: string, on: boolean, draggable: boolean): HTMLElement => {
    const node = el('div', on ? 'tbp-row is-on' : 'tbp-row');
    node.setAttribute('role', 'listitem');
    node.dataset.item = id;
    const grip = el('span', 'tbp-grip');
    grip.setAttribute('aria-hidden', 'true');
    if (draggable) {
      grip.append(icon('grip', { size: 12, fine: true }));
      grip.addEventListener('pointerdown', (event) => startDrag(event, node));
    }
    const glyph = el('span', 'tbp-glyph');
    glyph.setAttribute('aria-hidden', 'true');
    glyph.append(icon(itemIcon(id, glyphs.get(id)) as IconName));
    const text = el('span', 'tbp-text');
    const name = el('span', 'tbp-label');
    name.textContent = label;
    name.dir = 'auto';
    text.append(name);
    const toggle = el('button', 'bw-toggle tbp-toggle');
    toggle.type = 'button';
    toggle.setAttribute('role', 'switch');
    toggle.setAttribute('aria-checked', String(on));
    toggle.setAttribute('aria-label', `${label} on ${BAR_NAMES[bar]}`);
    if (draggable) toggle.setAttribute('aria-keyshortcuts', 'Alt+ArrowUp Alt+ArrowDown');
    toggle.dataset.focus = `switch:${id}`;
    toggle.addEventListener('click', () => (toggle.getAttribute('aria-checked') === 'true' ? turnOff(id) : turnOn(id, toggle)));
    toggle.addEventListener('keydown', (event) => {
      if (!draggable || !event.altKey || (event.key !== 'ArrowUp' && event.key !== 'ArrowDown')) return;
      event.preventDefault();
      send(bar, moveItem(bar, lists[bar], id, event.key === 'ArrowUp' ? -1 : 1), `switch:${id}`);
    });
    // The whole row is the switch's target, except the grip.
    node.addEventListener('click', (event) => { if (event.target === node || event.target === text || event.target === glyph || name.contains(event.target as Node)) toggle.click(); });
    node.append(grip, glyph, text, toggle);
    if (popped === id) replay(glyph, 'is-popped');
    return node;
  };

  // ---- dragging (pointer) ----
  let dragging: { node: HTMLElement; id: string; startY: number; peers: HTMLElement[]; tops: number[]; height: number } | null = null;
  function startDrag(event: PointerEvent, node: HTMLElement): void {
    if (event.button !== 0) return;
    event.preventDefault();
    const list = node.parentElement;
    if (!list) return;
    const peers = Array.from(list.querySelectorAll<HTMLElement>('.tbp-row.is-on[data-item]'));
    dragging = { node, id: node.dataset.item ?? '', startY: event.clientY, peers, tops: peers.map((peer) => peer.getBoundingClientRect().top), height: node.getBoundingClientRect().height };
    node.classList.add('is-dragging');
    node.setPointerCapture?.(event.pointerId);
    node.addEventListener('pointermove', onDrag);
    node.addEventListener('pointerup', endDrag, { once: true });
    node.addEventListener('pointercancel', endDrag, { once: true });
  }
  /** The place among its peers the dragged row would drop at. */
  const dropIndex = (dy: number): number => {
    if (!dragging) return 0;
    const from = dragging.peers.indexOf(dragging.node);
    const y = (dragging.tops[from] ?? 0) + dy + dragging.height / 2;
    let index = 0;
    dragging.tops.forEach((top, at) => { if (at !== from && y > top + dragging!.height / 2) index += 1; });
    return index;
  };
  function onDrag(event: PointerEvent): void {
    if (!dragging) return;
    const dy = event.clientY - dragging.startY;
    const from = dragging.peers.indexOf(dragging.node);
    const to = dropIndex(dy);
    dragging.node.style.setProperty('--tbp-drag', `${dy}px`);
    dragging.peers.forEach((peer, at) => {
      if (peer === dragging!.node) return;
      const shift = from < to && at > from && at <= to ? -1 : from > to && at < from && at >= to ? 1 : 0;
      peer.style.setProperty('--tbp-shift', `${shift * dragging!.height}px`);
      peer.classList.toggle('is-shifted', shift !== 0);
    });
  }
  function endDrag(event: PointerEvent): void {
    if (!dragging) return;
    const { node, id, peers, startY } = dragging;
    node.removeEventListener('pointermove', onDrag);
    const to = dropIndex(event.clientY - startY);
    const others = peers.filter((peer) => peer !== node).map((peer) => peer.dataset.item ?? '');
    node.classList.remove('is-dragging');
    node.style.removeProperty('--tbp-drag');
    for (const peer of peers) { peer.classList.remove('is-shifted'); peer.style.removeProperty('--tbp-shift'); }
    dragging = null;
    // Dropped where it was: nothing to send.
    if (peers.indexOf(node) === to) return;
    moveTo(id, others[to] ?? null);
  }

  // ---- painting ----
  const paint = (options: { swap?: boolean }): void => {
    if (!snapshot) return;
    const before = options.swap ? new Map<string, number>() : positions();
    const items = lists[bar];
    const names = new Map(snapshot.commands.map((command) => [itemOf(command), command.label] as const));
    const nameOf = (id: string): string => itemLabel(id, names);

    const sections: HTMLElement[] = [];
    for (const group of GROUPS[bar]) {
      const { on, off } = groupRows(group, items);
      const section = el('div', 'tbp-section');
      const title = el('h2', 'tbp-group');
      title.id = `tbp-g-${group.key}`;
      title.textContent = group.title;
      const list = el('div', 'tbp-list');
      list.setAttribute('role', 'list');
      list.setAttribute('aria-labelledby', title.id);
      list.append(
        ...on.map((id) => row(id, nameOf(id), true, on.length > 1)),
        ...off.map((id) => row(id, ITEM_LABELS[id]?.label ?? id, false, false)),
      );
      section.append(title, list);
      sections.push(section);
    }
    card.replaceChildren(...sections);

    // Commands that are off; the ones that are on sit in their group above.
    const used = items.filter(isCommandItem).length;
    commandsTitle.textContent = 'Commands';
    commandsCount.textContent = `${used} of ${MAX_COMMAND_ITEMS} on ${bar === 'address' ? 'the bar' : 'the tab bar'}`;
    commandsCount.classList.toggle('is-full', used >= MAX_COMMAND_ITEMS);
    const offCommands = snapshot.commands.filter((command) => !items.includes(itemOf(command)));
    const shown = filter === '' ? offCommands : offCommands.filter((command) => command.label.toLocaleLowerCase().includes(filter));
    const rows = shown.map((command) => row(itemOf(command), command.chain ? `Chain: ${command.label}` : command.label, false, false));
    if (rows.length === 0) {
      const none = el('p', 'tbp-none');
      none.textContent = filter === '' ? 'Every command is on this bar.' : 'No command matches.';
      rows.push(none);
    }
    commandList.replaceChildren(...rows);
    reset.disabled = items.length === DEFAULTS[bar].length && items.every((id, at) => id === DEFAULTS[bar][at]);

    if (options.swap && !reducedMotion()) replay(card, 'is-swapping');
    glide(before);
    popped = null;
    if (refocus !== null) {
      const wanted = refocus;
      refocus = null;
      shell.querySelector<HTMLElement>(`[data-focus="${CSS.escape(wanted)}"]`)?.focus({ preventScroll: true });
    }
  };

  return (message: Inbound): void => {
    if (message.t !== 'snap' || message.p !== 'toolbar') return;
    const next = parseToolbarSnapshot(message.d);
    if (!next) return;
    snapshot = next;
    glyphs = new Map(next.commands.filter((command) => command.glyph !== undefined).map((command) => [itemOf(command), command.glyph as string] as const));
    lists.address = [...next.address];
    lists.tabs = [...next.tabs];
    paint({});
    if (!entered) {
      entered = true;
      if (!reducedMotion()) replay(body, 'is-entering');
    }
  };
}
// --- end toolbars and feeds ---
