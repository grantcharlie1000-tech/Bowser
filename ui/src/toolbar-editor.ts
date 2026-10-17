// --- toolbars and feeds (thread T13) ---
// The toolbar editor (Settings > Appearance > Edit toolbars): for the address bar, the tab bar and the
// panel bar, the buttons each one shows, in order. Move a button up or down, take it off, add one that is
// off (or any command or command chain), reset a bar. Bowser has no status bar of its own, so there is
// none to edit. Every change is the whole new list for one bar: the settings page saves it (the address
// bar's through its Save, the others through `baropt`) and Rust puts it to work at once.
// Names arrive as text, never HTML. The pure rules are in bars-logic.ts.
import { el, icon, type IconName } from './dom';
import { selectField, type SelectField, type SelectOption } from './panels/select';
import { iconButton, textButton } from './panels/kit';
import {
  BAR_IDS, BAR_TITLES, ITEM_ICONS, ITEM_LABELS, addItem, addable, addressZones, canMove, isCommandItem, isDefault, itemLabel, moveItem, removeItem, whyNotAdd,
  DEFAULTS, type BarId,
} from './bars-logic';

export interface EditorCommand { readonly id: string; readonly label: string; readonly chain: boolean }
export interface EditorState {
  readonly address: readonly string[];
  readonly tabs: readonly string[];
  readonly panel: readonly string[];
  /** Every command and chain Rust knows (`commands` snapshot), for the "Add a button" list and the names of command buttons. */
  readonly commands: readonly EditorCommand[];
}
export interface ToolbarEditor {
  readonly root: HTMLElement;
  set(state: EditorState): void;
  disable(off: boolean): void;
}

const BAR_LEADS: Readonly<Record<BarId, string>> = {
  address: 'The buttons around the address field. The menu button always stays last.',
  tabs: 'The buttons at the end of the tab strip, or under the tabs in the sidebar. The button that opens a new tab stays where it is.',
  panel: 'Buttons that sit after Bookmarks and History on the panel bar. Home, workspaces, web apps, Bookmarks and History are shown or hidden in Customize.',
};
const ZONE_TITLES = { left: 'Before the address field', pill: 'Inside the address field', right: 'After the address field' } as const;
const ZONE_EMPTY = { left: 'No buttons before the address field.', pill: 'No buttons inside the address field.', right: 'No buttons after the address field.' } as const;
const BAR_NAMES: Readonly<Record<BarId, string>> = { address: 'the address bar', tabs: 'the tab bar', panel: 'the panel bar' };

/** Command ids that only exist so shortcuts can reach them: never offered as a button. */
const HIDDEN_COMMAND = /^select-/;

export function mountToolbarEditor(options: { onChange(bar: BarId, items: string[]): void }): ToolbarEditor {
  let state: EditorState = { address: [], tabs: [], panel: [], commands: [] };
  let disabled = false;
  /** What to focus after the next paint: `bar:item:action`. */
  let refocus: string | null = null;
  const root = el('div', 'sp-tbe');
  const bars = new Map<BarId, { readonly body: HTMLElement; readonly reset: HTMLButtonElement; readonly note: HTMLElement; readonly add: SelectField<string> | null; addKey: string }>();

  const names = (): Map<string, string> => {
    const map = new Map<string, string>();
    for (const command of state.commands) map.set(command.chain ? command.id : `cmd:${command.id}`, command.label);
    return map;
  };
  const listOf = (bar: BarId): readonly string[] => state[bar];
  const change = (bar: BarId, items: string[] | null, focus: string | null): void => {
    if (items === null || disabled) return;
    refocus = focus;
    options.onChange(bar, items);
  };

  const glyph = (id: string): IconName => (isCommandItem(id) ? 'command' : (ITEM_ICONS[id] as IconName | undefined) ?? 'command');

  const itemRow = (bar: BarId, id: string, label: string): HTMLElement => {
    const items = listOf(bar);
    const row = el('div', 'sp-tb-row sp-tb-item');
    row.setAttribute('role', 'listitem');
    row.dataset.item = id;
    const lead = el('span', 'sp-tb-glyph');
    lead.setAttribute('aria-hidden', 'true');
    lead.append(icon(glyph(id)));
    const text = el('div', 'sp-text');
    const name = el('span', 'sp-label');
    name.textContent = label;
    name.dir = 'auto';
    text.append(name);
    const hint = ITEM_LABELS[id]?.hint ?? (isCommandItem(id) ? (id.startsWith('chain:') ? 'A command chain.' : 'A command.') : '');
    if (hint !== '') {
      const desc = el('span', 'sp-desc');
      desc.textContent = hint;
      text.append(desc);
    }
    const tools = el('div', 'sp-ctl sp-tb-tools');
    const up = iconButton('chevron-up', `Move ${label} up`, () => change(bar, moveItem(bar, items, id, -1), `${bar}:${id}:up`));
    const down = iconButton('chevron-down', `Move ${label} down`, () => change(bar, moveItem(bar, items, id, 1), `${bar}:${id}:down`));
    const remove = iconButton('close', `Remove ${label} from ${BAR_NAMES[bar]}`, () => change(bar, removeItem(items, id), `${bar}::add`));
    up.disabled = disabled || !canMove(bar, items, id, -1);
    down.disabled = disabled || !canMove(bar, items, id, 1);
    remove.disabled = disabled;
    up.dataset.focus = `${bar}:${id}:up`;
    down.dataset.focus = `${bar}:${id}:down`;
    tools.append(up, down, remove);
    row.append(lead, text, tools);
    return row;
  };

  const fixedRow = (name: string, hint: string, iconName: IconName): HTMLElement => {
    const row = el('div', 'sp-tb-row sp-tb-item is-fixed');
    row.setAttribute('role', 'listitem');
    const lead = el('span', 'sp-tb-glyph');
    lead.setAttribute('aria-hidden', 'true');
    lead.append(icon(iconName));
    const text = el('div', 'sp-text');
    const label = el('span', 'sp-label');
    label.textContent = name;
    const desc = el('span', 'sp-desc');
    desc.textContent = hint;
    text.append(label, desc);
    row.append(lead, text);
    return row;
  };

  const zoneHeading = (text: string): HTMLElement => {
    const heading = el('p', 'sp-tb-zone');
    heading.textContent = text;
    return heading;
  };

  const addOptions = (bar: BarId): SelectOption<string>[] => {
    const items = listOf(bar);
    const out: SelectOption<string>[] = [];
    for (const id of addable(bar, items)) {
      out.push({ value: id, label: ITEM_LABELS[id]?.label ?? id, lead: () => icon(glyph(id)) });
    }
    const wanted = new Set(items);
    for (const command of state.commands) {
      if (HIDDEN_COMMAND.test(command.id)) continue;
      const item = command.chain ? command.id : `cmd:${command.id}`;
      if (wanted.has(item)) continue;
      out.push({ value: item, label: command.chain ? `Chain: ${command.label}` : command.label, lead: () => icon('command') });
    }
    return out;
  };

  const buildBar = (bar: BarId): void => {
    const group = el('section', 'sp-tbe-bar');
    group.dataset.bar = bar;
    const headingId = `sp-tbe-h-${bar}`;
    group.setAttribute('aria-labelledby', headingId);
    const top = el('div', 'sp-tbe-head');
    const heading = el('h3', 'sp-group-h sp-tbe-title');
    heading.id = headingId;
    heading.textContent = BAR_TITLES[bar];
    const reset = textButton('Reset', () => change(bar, [...DEFAULTS[bar]], null), { subtle: true, icon: 'reset', aria: `Reset ${BAR_NAMES[bar]} to its default buttons` });
    top.append(heading, reset);
    const lead = el('p', 'sp-tbe-lead');
    lead.textContent = BAR_LEADS[bar];
    const body = el('div', 'sp-card sp-tbe-card');
    body.setAttribute('role', 'list');
    body.setAttribute('aria-label', `Buttons on ${BAR_NAMES[bar]}`);
    const foot = el('div', 'sp-tbe-foot');
    const note = el('p', 'sp-tbe-note');
    note.setAttribute('role', 'status');
    foot.append(note);
    group.append(top, lead, body, foot);
    root.append(group);
    bars.set(bar, { body, reset, note, add: null, addKey: '' });
  };
  for (const bar of BAR_IDS) buildBar(bar);

  const paintBar = (bar: BarId): void => {
    const entry = bars.get(bar);
    if (!entry) return;
    const items = listOf(bar);
    const labels = names();
    const nameOf = (id: string): string => itemLabel(id, labels);
    const rows: HTMLElement[] = [];
    if (bar === 'address') {
      const zones = addressZones(items);
      for (const zone of ['left', 'pill', 'right'] as const) {
        rows.push(zoneHeading(ZONE_TITLES[zone]));
        if (zones[zone].length === 0) {
          const none = el('p', 'sp-tb-none');
          none.textContent = ZONE_EMPTY[zone];
          rows.push(none);
        }
        for (const id of zones[zone]) rows.push(itemRow(bar, id, nameOf(id)));
        if (zone === 'pill') rows.push(fixedRow('Address field', 'Always there.', 'address-bar'));
      }
      rows.push(fixedRow('Menu', 'Always the last button.', 'menu-dots'));
    } else if (items.length === 0) {
      const none = el('p', 'sp-tb-none');
      none.textContent = 'No buttons on this bar.';
      rows.push(none);
    } else {
      for (const id of items) rows.push(itemRow(bar, id, nameOf(id)));
    }
    entry.body.replaceChildren(...rows);
    entry.reset.disabled = disabled || isDefault(bar, items);
    // The "Add a button" list: rebuilt when what it offers changed.
    const options2 = addOptions(bar);
    const key = options2.map((option) => option.value).join(',');
    if (entry.add === null || entry.addKey !== key) {
      entry.add?.close();
      const field = selectField<string>({
        options: options2,
        value: null,
        label: `Add a button to ${BAR_NAMES[bar]}`,
        placeholder: 'Add a button…',
        momentary: true,
        minListWidth: 260,
        onChange: (value) => {
          const next = addItem(bar, listOf(bar), value);
          if (next === null) {
            entry.note.textContent = whyNotAdd(bar, listOf(bar), value) ?? 'That button cannot be added.';
            return;
          }
          entry.note.textContent = '';
          change(bar, next, `${bar}:${value}:up`);
        },
      });
      field.button.dataset.focus = `${bar}::add`;
      const foot = entry.note.parentElement;
      entry.add?.root.remove();
      foot?.prepend(field.root);
      bars.set(bar, { ...entry, add: field, addKey: key });
    }
    const added = bars.get(bar)?.add;
    added?.button.toggleAttribute('disabled', disabled || options2.length === 0);
  };

  const paint = (): void => {
    for (const bar of BAR_IDS) paintBar(bar);
    if (refocus !== null) {
      const wanted = refocus;
      const [barPart, itemPart] = wanted.split(':');
      const all = Array.from(root.querySelectorAll<HTMLElement>('[data-focus]')).filter((node) => !node.hasAttribute('disabled'));
      // The button that was used; failing that (it is gone, or at its edge now) another control of the same row.
      const target = all.find((node) => node.dataset.focus === wanted) ?? all.find((node) => node.dataset.focus?.startsWith(`${barPart}:${itemPart}:`));
      target?.focus({ preventScroll: true });
      refocus = null;
    }
  };

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
