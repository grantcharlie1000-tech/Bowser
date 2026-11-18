// --- toolbars and feeds (thread T13) ---
// The toolbar editor (Settings > Appearance > Toolbars). One bar at a time (Address bar | Tab bar), shown three
// ways that stay in step:
//   - a live preview of the bar, drawn with the real glyphs in their real places;
//   - the list of its buttons, each with move up, move down and remove (revealed on hover or focus);
//   - "Add" chips for the bar's own buttons that are off it, and a picker for any command or chain.
// Buttons that appear only while they have something to show (Reader, Downloads, Extensions, Closed tabs)
// say so under their name and are drawn dashed in the preview, so a button just added never looks lost.
// Bowser has no status bar and no longer has the left rail, so neither is offered.
// Every change is the whole new list for one bar: the settings page saves it (the address bar's through its
// Save, the tab bar's through `baropt`) and Rust puts it to work at once. Names arrive as text, never HTML.
// The pure rules are in bars-logic.ts.
import { el, icon, type IconName } from './dom';
import { selectField, type SelectField, type SelectOption } from './panels/select';
import { iconButton, segmented, textButton } from './panels/kit';
import {
  EDITABLE_BARS, ITEM_LABELS, SHOWS_WHEN, addItem, addable, addressZones, canMove, isCommandItem, isDefault, itemIcon, itemLabel, moveItem,
  removeItem, whyNotAdd, DEFAULTS, type BarId,
} from './bars-logic';

export interface EditorCommand { readonly id: string; readonly label: string; readonly chain: boolean }
export interface EditorState {
  readonly address: readonly string[];
  readonly tabs: readonly string[];
  /** Still sent by the page; not edited (the panel bar belonged to the removed rail). */
  readonly panel: readonly string[];
  /** Every command and chain Rust knows (`commands` snapshot), for the command picker and the names of command buttons. */
  readonly commands: readonly EditorCommand[];
}
export interface ToolbarEditor {
  readonly root: HTMLElement;
  set(state: EditorState): void;
  disable(off: boolean): void;
}

const BAR_SHORT: Readonly<Record<BarId, string>> = { address: 'Address bar', tabs: 'Tab bar', panel: 'Panel bar' };
const BAR_NAMES: Readonly<Record<BarId, string>> = { address: 'the address bar', tabs: 'the tab bar', panel: 'the panel bar' };
const ZONE_TITLES = { left: 'Before the address field', pill: 'Inside the address field', right: 'After the address field' } as const;

/** Command ids that only exist so shortcuts can reach them: never offered as a button. */
const HIDDEN_COMMAND = /^select-/;

/** The one effect the next paint plays: the item that was added (it pops in) or moved (it glides from where it was). */
type Fx = { readonly kind: 'add'; readonly id: string; readonly list: string } | { readonly kind: 'move'; readonly id: string; readonly dir: -1 | 1; readonly list: string };

const glyph = (id: string, size?: number): SVGSVGElement => icon(itemIcon(id) as IconName, size === undefined ? undefined : { size });

export function mountToolbarEditor(options: { onChange(bar: BarId, items: string[]): void }): ToolbarEditor {
  let state: EditorState = { address: [], tabs: [], panel: [], commands: [] };
  let disabled = false;
  let bar: BarId = 'address';
  /** What to focus after the next paint: `bar:item:action`. */
  let refocus: string | null = null;
  let fx: Fx | null = null;

  const root = el('div', 'sp-tbe');

  // ---- head: which bar, and its Reset ----
  const head = el('div', 'sp-tbe-head');
  const picker = segmented<BarId>(EDITABLE_BARS.map((id) => [id, BAR_SHORT[id]] as const), bar, (next) => { bar = next; note.textContent = ''; paint(); }, 'Bar to edit');
  picker.classList.add('sp-tbe-switch');
  const reset = textButton('Reset', () => change(bar, [...DEFAULTS[bar]], null), { subtle: true, icon: 'reset' });
  head.append(picker, reset);

  // ---- preview ----
  const preview = el('div', 'sp-tbe-preview');
  preview.setAttribute('aria-hidden', 'true');

  // ---- the buttons on the bar ----
  const listTitle = el('h3', 'sp-tbe-sub');
  listTitle.id = 'sp-tbe-list-h';
  listTitle.textContent = 'On the bar';
  const list = el('div', 'sp-card sp-tbe-card');
  list.setAttribute('role', 'list');
  list.setAttribute('aria-labelledby', listTitle.id);

  // ---- adding ----
  const addTitle = el('h3', 'sp-tbe-sub');
  addTitle.id = 'sp-tbe-add-h';
  addTitle.textContent = 'Add';
  const chips = el('div', 'sp-tbe-chips');
  chips.setAttribute('role', 'group');
  chips.setAttribute('aria-labelledby', addTitle.id);
  const commandSlot = el('div', 'sp-tbe-command');
  const note = el('p', 'sp-tbe-note');
  note.setAttribute('role', 'status');
  let commandField: SelectField<string> | null = null;
  let commandKey = '';

  root.append(head, preview, listTitle, list, addTitle, chips, commandSlot, note);

  const names = (): Map<string, string> => {
    const map = new Map<string, string>();
    for (const command of state.commands) map.set(command.chain ? command.id : `cmd:${command.id}`, command.label);
    return map;
  };
  const listOf = (which: BarId): readonly string[] => state[which];
  const change = (which: BarId, items: string[] | null, focus: string | null, effect: Fx | null = null): void => {
    if (items === null || disabled) return;
    refocus = focus;
    fx = effect;
    options.onChange(which, items);
  };
  /** Adds `id` to the current bar, or says why it cannot (and shakes what was pressed). */
  const add = (id: string, from: HTMLElement | null): void => {
    const items = listOf(bar);
    const next = addItem(bar, items, id);
    if (next === null) {
      note.textContent = whyNotAdd(bar, items, id) ?? 'That button cannot be added.';
      if (from) replay(from, 'is-refused');
      return;
    }
    note.textContent = '';
    change(bar, next, `${bar}:${id}:remove`, { kind: 'add', id, list: next.join(',') });
  };

  // ---- micro motion helpers (classes only; the CSS keeps them inside prefers-reduced-motion) ----
  const replay = (node: HTMLElement, cls: string): void => {
    node.classList.remove(cls);
    void node.offsetWidth;
    node.classList.add(cls);
    node.addEventListener('animationend', () => node.classList.remove(cls), { once: true });
  };
  /** Lights the preview's copy of `id` while its row is pointed at or focused. */
  const hot = (id: string, on: boolean): void => {
    for (const node of Array.from(preview.querySelectorAll<HTMLElement>('[data-item]'))) {
      if (node.dataset.item === id) node.classList.toggle('is-hot', on);
    }
  };

  // ---- preview ----
  const previewButton = (id: string, _label: string): HTMLElement => {
    const node = el('span', 'sp-tbp-btn');
    node.dataset.item = id;
    if (SHOWS_WHEN[id] !== undefined) node.classList.add('is-conditional');
    node.append(glyph(id));
    return node;
  };
  const paintPreview = (items: readonly string[], nameOf: (id: string) => string): void => {
    const nodes: HTMLElement[] = [];
    if (bar === 'address') {
      const zones = addressZones(items);
      const left = el('span', 'sp-tbp-zone');
      for (let at = 0; at < zones.left.length; at += 1) {
        const id = zones.left[at] as string;
        const next = zones.left[at + 1];
        // Back and forward share one capsule with a line between them, as on the real bar.
        if ((id === 'back' && next === 'forward') || (id === 'forward' && next === 'back')) {
          const pair = el('span', 'sp-tbp-pair');
          pair.append(previewButton(id, nameOf(id)), previewButton(next, nameOf(next)));
          left.append(pair);
          at += 1;
        } else left.append(previewButton(id, nameOf(id)));
      }
      const pill = el('span', 'sp-tbp-pill');
      const site = zones.pill.includes('site-info') ? [previewButton('site-info', nameOf('site-info'))] : [];
      const text = el('span', 'sp-tbp-text');
      text.textContent = 'Search or enter address';
      pill.append(...site, text, ...zones.pill.filter((id) => id !== 'site-info').map((id) => previewButton(id, nameOf(id))));
      const right = el('span', 'sp-tbp-zone');
      right.append(...zones.right.map((id) => previewButton(id, nameOf(id))));
      const menu = el('span', 'sp-tbp-btn is-fixed');
      menu.append(icon('menu-dots'));
      right.append(menu);
      if (zones.left.length > 0) nodes.push(left);
      nodes.push(pill, right);
    } else {
      const strip = el('span', 'sp-tbp-tabs');
      for (const [title, active] of [['Inbox', false], ['Design notes', true], ['Docs', false]] as const) {
        const tab = el('span', active ? 'sp-tbp-tab is-active' : 'sp-tbp-tab');
        tab.textContent = title;
        strip.append(tab);
      }
      const plus = el('span', 'sp-tbp-btn is-fixed');
      plus.append(icon('plus'));
      const tools = el('span', 'sp-tbp-zone');
      tools.append(...items.map((id) => previewButton(id, nameOf(id))));
      nodes.push(strip, plus, tools);
    }
    preview.classList.toggle('is-tabs', bar === 'tabs');
    preview.replaceChildren(...nodes);
  };

  // ---- the list ----
  const itemRow = (id: string, label: string, items: readonly string[]): HTMLElement => {
    const row = el('div', 'sp-tb-row sp-tb-item');
    row.setAttribute('role', 'listitem');
    row.dataset.item = id;
    const lead = el('span', 'sp-tb-glyph');
    lead.setAttribute('aria-hidden', 'true');
    lead.append(glyph(id));
    const text = el('div', 'sp-text');
    const name = el('span', 'sp-label');
    name.textContent = label;
    name.dir = 'auto';
    text.append(name);
    const when = SHOWS_WHEN[id];
    if (when !== undefined) {
      const hint = el('span', 'sp-tb-hint');
      hint.textContent = when;
      text.append(hint);
    }
    const tools = el('div', 'sp-ctl sp-tb-tools');
    const up = iconButton('chevron-up', `Move ${label} up`, () => change(bar, moveItem(bar, items, id, -1), `${bar}:${id}:up`, { kind: 'move', id, dir: -1, list: (moveItem(bar, items, id, -1) ?? []).join(',') }));
    const down = iconButton('chevron-down', `Move ${label} down`, () => change(bar, moveItem(bar, items, id, 1), `${bar}:${id}:down`, { kind: 'move', id, dir: 1, list: (moveItem(bar, items, id, 1) ?? []).join(',') }));
    const remove = iconButton('close', `Remove ${label} from ${BAR_NAMES[bar]}`, () => change(bar, removeItem(items, id), `${bar}::next`), 'sp-tb-remove');
    // An arrow that cannot move the button is left out, not greyed.
    up.hidden = !canMove(bar, items, id, -1);
    down.hidden = !canMove(bar, items, id, 1);
    for (const button of [up, down, remove]) button.disabled = disabled;
    up.dataset.focus = `${bar}:${id}:up`;
    down.dataset.focus = `${bar}:${id}:down`;
    remove.dataset.focus = `${bar}:${id}:remove`;
    tools.append(up, down, remove);
    row.append(lead, text, tools);
    row.addEventListener('pointerenter', () => hot(id, true));
    row.addEventListener('pointerleave', () => hot(id, false));
    row.addEventListener('focusin', () => hot(id, true));
    row.addEventListener('focusout', () => hot(id, false));
    return row;
  };
  const zoneHeading = (text: string): HTMLElement => {
    const heading = el('p', 'sp-tb-zone');
    heading.textContent = text;
    return heading;
  };
  const paintList = (items: readonly string[], nameOf: (id: string) => string): void => {
    const rows: HTMLElement[] = [];
    if (items.length === 0) {
      const none = el('p', 'sp-tb-none');
      none.textContent = bar === 'address' ? 'Only the address field and the menu. Add buttons below.' : 'No buttons yet. Add one below.';
      rows.push(none);
    } else if (bar === 'address') {
      const zones = addressZones(items);
      for (const zone of ['left', 'pill', 'right'] as const) {
        if (zones[zone].length === 0) continue;
        rows.push(zoneHeading(ZONE_TITLES[zone]));
        for (const id of zones[zone]) rows.push(itemRow(id, nameOf(id), items));
      }
    } else {
      for (const id of items) rows.push(itemRow(id, nameOf(id), items));
    }
    list.replaceChildren(...rows);
  };

  // ---- adding ----
  const paintChips = (items: readonly string[]): void => {
    const free = addable(bar, items);
    const nodes: HTMLElement[] = free.map((id) => {
      const label = ITEM_LABELS[id]?.label ?? id;
      const chip = el('button', 'sp-tbe-chip');
      chip.type = 'button';
      chip.setAttribute('aria-label', `Add ${label} to ${BAR_NAMES[bar]}`);
      chip.dataset.focus = `${bar}:${id}:chip`;
      chip.disabled = disabled;
      const plus = el('span', 'sp-tbe-chip-plus');
      plus.append(icon('plus', { size: 12, fine: true }));
      const name = el('span');
      name.textContent = label;
      chip.append(glyph(id, 16), name, plus);
      chip.addEventListener('click', () => add(id, chip));
      return chip;
    });
    if (nodes.length === 0) {
      const all = el('p', 'sp-tb-none');
      all.textContent = 'Every built-in button is on this bar.';
      nodes.push(all);
    }
    chips.replaceChildren(...nodes);
  };
  const commandOptions = (items: readonly string[]): SelectOption<string>[] => {
    const wanted = new Set(items);
    const out: SelectOption<string>[] = [];
    for (const command of state.commands) {
      if (HIDDEN_COMMAND.test(command.id)) continue;
      const item = command.chain ? command.id : `cmd:${command.id}`;
      if (wanted.has(item)) continue;
      out.push({ value: item, label: command.chain ? `Chain: ${command.label}` : command.label, lead: () => glyph(item) });
    }
    return out;
  };
  const paintCommands = (items: readonly string[]): void => {
    const offered = commandOptions(items);
    const key = `${bar}|${offered.map((option) => option.value).join(',')}`;
    if (commandField === null || commandKey !== key) {
      commandField?.close();
      commandKey = key;
      commandField = selectField<string>({
        options: offered,
        value: null,
        label: `Add a command or chain to ${BAR_NAMES[bar]}`,
        placeholder: 'Add a command…',
        momentary: true,
        minListWidth: 280,
        onChange: (value) => add(value, commandField?.button ?? null),
      });
      commandField.button.dataset.focus = `${bar}::command`;
      const lead = el('span', 'sp-tbe-command-lead');
      lead.setAttribute('aria-hidden', 'true');
      lead.append(icon('command', { size: 16 }));
      commandSlot.replaceChildren(lead, commandField.root);
    }
    const used = items.filter(isCommandItem).length;
    commandField.button.toggleAttribute('disabled', disabled || offered.length === 0);
    commandSlot.classList.toggle('is-full', used >= 4);
  };

  // ---- effects after a paint ----
  const playFx = (items: readonly string[]): void => {
    if (fx === null || fx.list !== items.join(',')) return;
    const { id } = fx;
    const row = Array.from(list.querySelectorAll<HTMLElement>('[data-item]')).find((node) => node.dataset.item === id);
    const icons = Array.from(preview.querySelectorAll<HTMLElement>('[data-item]')).filter((node) => node.dataset.item === id);
    if (fx.kind === 'add') {
      if (row) replay(row, 'is-new');
      for (const node of icons) replay(node, 'is-new');
    } else {
      if (row) replay(row, fx.dir < 0 ? 'is-moved-up' : 'is-moved-down');
      for (const node of icons) replay(node, 'is-new');
    }
    fx = null;
  };

  const paint = (): void => {
    const items = listOf(bar);
    const labels = names();
    const nameOf = (id: string): string => itemLabel(id, labels);
    reset.disabled = disabled || isDefault(bar, items);
    reset.setAttribute('aria-label', `Reset ${BAR_NAMES[bar]} to its default buttons`);
    paintPreview(items, nameOf);
    paintList(items, nameOf);
    paintChips(items);
    paintCommands(items);
    playFx(items);
    if (refocus !== null) {
      const wanted = refocus;
      const [barPart, itemPart] = wanted.split(':');
      const all = Array.from(root.querySelectorAll<HTMLElement>('[data-focus]')).filter((node) => !node.hasAttribute('disabled') && !node.hidden);
      // The button that was used; failing that another control of the same row; failing that (the row is gone)
      // the first control of the list, then the command picker.
      const target = all.find((node) => node.dataset.focus === wanted)
        ?? all.find((node) => node.dataset.focus?.startsWith(`${barPart}:${itemPart}:`) && itemPart !== '')
        ?? all.find((node) => node.dataset.focus?.endsWith(':remove'))
        ?? all.find((node) => node.dataset.focus === `${barPart}::command`);
      target?.focus({ preventScroll: true });
      refocus = null;
    }
  };
  paint();

  return {
    root,
    set(next): void {
      state = next;
      paint();
    },
    disable(off): void {
      disabled = off;
      paint();
    },
  };
}
// --- end toolbars and feeds ---
