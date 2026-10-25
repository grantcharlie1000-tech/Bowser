// The Tabs section of the settings page: one table of options, the same keys and values as
// src/tab_options.rs (a Rust test writes the snapshot fixture this file's test parses). Each option
// is one `key=value` text sent in a `tabopt` command; Rust validates it again. No DOM here.

export type OptionValue = string | number | boolean;
export type TabOptionValues = { readonly [key: string]: OptionValue };

interface Base {
  readonly key: string;
  readonly label: string;
  readonly description: string;
  /** Extra words the settings search matches (never shown). */
  readonly keywords: string;
  readonly card: TabCardKey;
}
export interface ToggleSpec extends Base {
  readonly kind: 'toggle';
  readonly default: boolean;
  /** --- two-level stacks (T10b) --- the choice option this switch only matters for, and its value. */
  readonly when?: readonly [key: string, value: string];
}
export interface ChoiceSpec extends Base {
  readonly kind: 'choice';
  readonly default: string;
  readonly choices: ReadonlyArray<readonly [value: string, label: string] | readonly [value: string, label: string, aria: string]>;
}
export interface NumberSpec extends Base {
  readonly kind: 'number';
  readonly default: number;
  readonly min: number;
  readonly max: number;
  /** What a value of 0 means, when 0 is "off". */
  readonly zero?: string;
  readonly unit: string;
}
export interface TextSpec extends Base {
  readonly kind: 'text';
  readonly default: string;
  readonly max: number;
  readonly placeholder: string;
  /** The choice option this text only matters for, and the value that turns it on. */
  readonly when: readonly [key: string, value: string];
}
export type TabOptionSpec = ToggleSpec | ChoiceSpec | NumberSpec | TextSpec;

export const TAB_CARDS = ['new', 'close', 'bar', 'select', 'audio', 'pinned', 'restore', 'stacks'] as const;
export type TabCardKey = typeof TAB_CARDS[number];
export const TAB_CARD_TITLES: Readonly<Record<TabCardKey, string>> = {
  new: 'New',
  close: 'Closing',
  bar: 'Bar',
  select: 'Selecting',
  audio: 'Audio',
  pinned: 'Pinned',
  restore: 'Restore',
  stacks: 'Groups', // --- T10 --- the part title; tiling and workspaces stay in the row text.
};

const toggle = (key: string, card: TabCardKey, def: boolean, label: string, description: string, keywords: string): ToggleSpec =>
  ({ kind: 'toggle', key, card, default: def, label, description, keywords });
const choice = (key: string, card: TabCardKey, def: string, choices: ChoiceSpec['choices'], label: string, description: string, keywords: string): ChoiceSpec =>
  ({ kind: 'choice', key, card, default: def, choices, label, description, keywords });
const number = (key: string, card: TabCardKey, def: number, min: number, max: number, unit: string, label: string, description: string, keywords: string, zero?: string): NumberSpec =>
  ({ kind: 'number', key, card, default: def, min, max, unit, label, description, keywords, ...(zero === undefined ? {} : { zero }) });

/** The key of the one option Rust keeps in two older settings (the position of a new tab). */
export const POSITION_KEY = 'tabs_new_tab_position';
export const MAX_PAGE_URL = 2048;

/** Every option, in the order the page lists them. The Rust table lists the same keys (plus the position). */
export const TAB_OPTIONS: readonly TabOptionSpec[] = [
  choice('tabs_new_tab_page', 'new', 'start', [['start', 'Start', 'Start page'], ['homepage', 'Homepage'], ['blank', 'Blank', 'Blank page'], ['url', 'Page', 'Specific page']], 'New tab page',
    '', 'new tab page start homepage blank url speed dial New tab page What a new tab opens: the start page, your homepage, a blank page, or an address.'),
  {
    kind: 'text', key: 'tabs_new_tab_url', card: 'new', default: '', max: MAX_PAGE_URL, placeholder: '', when: ['tabs_new_tab_page', 'url'],
    label: 'Page address', description: '', keywords: 'new tab address url website Address of the new tab page Opened by every new tab while the page above is a specific page.',
  },
  choice(POSITION_KEY, 'new', 'active', [['related', 'Related', 'After related'], ['active', 'Active', 'After active'], ['last', 'Last', 'As last'], ['stack', 'Stack', 'As stack with related']], 'New tab position',
    '', 'new tab position related active last order next beside stack New tab position Where a new tab goes. After related keeps tabs opened from a page next to it; as stack with related also puts them in its stack.'),
  toggle('tabs_link_background', 'new', false, 'Background', '', 'link background new tab open Open links in new tabs in the background A link opened in a new tab loads without leaving the page you are reading.'),
  choice('tabs_duplicate_position', 'new', 'beside', [['beside', 'Beside', 'Next to the original'], ['last', 'Last']], 'Duplicate',
    '', 'duplicate tab position copy Duplicated tab position Where Duplicate tab puts the copy.'),
  toggle('tabs_focus_page', 'new', false, 'Focus page', '', 'focus page content new tab address bar keyboard Focus the page on a new tab A new tab with an address puts the keyboard in the page instead of the address bar.'),

  toggle('tabs_close_related', 'close', false, 'Related tab', '', 'close tab activation related opener Return to the related tab when closing Closing a tab activates the tab that opened it, when it is still open.'),
  choice('tabs_close_activation', 'close', 'right', [['right', 'Right', 'Tab on the right'], ['left', 'Left', 'Tab on the left'], ['recent', 'Recent', 'Recently used']], 'After close',
    '', 'close tab activation recently used left right Tab to activate after closing Which tab is shown when the active tab closes and no related tab applies.'),
  toggle('tabs_align_close_button', 'close', false, 'Close in place', '', 'align close button pointer cursor Keep the next close button under the pointer After closing a tab with the mouse, the tabs keep their width until the pointer leaves the tab bar, so several tabs close in a row.'),
  toggle('tabs_keep_window', 'close', true, 'Keep window', '', 'last tab close window keep open Keep the window open when the last tab closes Off closes the window with its last tab.'),
  number('tabs_confirm_close', 'close', 0, 0, 99, 'tabs', 'Confirm close', '', 'confirm closing multiple tabs warn Confirm when closing many tabs Asks first when you close this many tabs at once, such as with Close other tabs.', 'Never ask'),
  choice('tabs_double_click', 'close', 'ignore', [['ignore', 'Nothing'], ['rename', 'Rename'], ['close', 'Close']], 'Double-click',
    '', 'double click tab rename close Double click on a tab What a double click on a tab does.'),
  toggle('tabs_close_left', 'close', false, 'Close on left', '', 'close button left side Close button on the left side Puts each tab’s close button before its title.'),
  toggle('tabs_close_always', 'close', false, 'Always close', '', 'close button permanent always visible Always show the close button The close button shows on every tab, not just the active and hovered ones.'),

  toggle('tabs_scroll', 'bar', false, 'Scroll tabs', '', 'horizontal scrolling scrollable tabs overflow Scroll the tab bar horizontally Tabs keep a readable width and the bar scrolls when they do not fit.'),
  toggle('tabs_remove_spacing_maximised', 'bar', false, 'Tight when max', '', 'remove spacing gap maximized maximised Remove tab spacing when maximised Takes away the gap above the tabs while the window is maximised.'),
  number('tabs_active_min_width', 'bar', 0, 0, 400, 'px', 'Active width', '', 'active tab minimum width size Active tab minimum width The narrowest the active tab gets.', 'Automatic'),
  choice('tabs_load_indicator', 'bar', 'spinner', [['spinner', 'Spinner', 'Favicon spinner'], ['bar', 'Bar'], ['favicon', 'Progress', 'Favicon progress'], ['none', 'None']], 'Load progress',
    '', 'page load progress bar spinner favicon indicator loading Page load progress How a tab shows that its page is loading.'),
  toggle('tabs_wheel_switch', 'bar', false, 'Wheel switch', '', 'switch tabs scroll wheel mouse Switch tabs by scrolling The mouse wheel over the tab bar switches to the next or previous tab.'),
  toggle('tabs_minimise_active', 'bar', false, 'Click to previous', '', 'minimize minimise active tab click previous cycling Minimise the active tab Clicking the active tab goes back to the tab you used before it.'),
  toggle('tabs_title_notifications', 'bar', true, 'Title counts', '', 'page title notifications count badge Detect page title notifications A title such as “(3) Inbox” puts its count on the tab.'),
  toggle('tabs_unread_indicator', 'bar', true, 'Unread dots', '', 'unread indicator dot background Show unread indicators A tab that changed while you were elsewhere carries a dot until you open it.'),
  toggle('tabs_dim_hibernated', 'bar', true, 'Dim sleeping', '', 'dim icon hibernated sleeping tabs Dim the icon of hibernated tabs A tab whose page is asleep shows a dimmed icon.'),
  number('tabs_inactive_days', 'bar', 0, 0, 365, 'days', 'Inactive days', '', 'inactive tabs days old unused threshold Inactive tabs threshold Tabs not used for this many days are shown dimmed.', 'Off'),

  toggle('tabs_selection', 'select', true, 'Multi-select', '', 'tab selection multiple select ctrl shift Select tabs with Ctrl and Shift Ctrl+click adds a tab to the selection and Shift+click selects a range. Tab actions then apply to the selection.'),
  toggle('tabs_selection_active', 'select', true, 'Include active', '', 'tab selection include active tab Include the active tab in the selection The active tab joins the selected tabs when you act on them.'),

  choice('tabs_mute_mode', 'audio', 'all', [['all', 'All', 'Play all'], ['active', 'Active', 'Only the active tab'], ['prioritise', 'Priority', 'Prioritise the active tab']], 'Tab audio',
    '', 'mute tab audio play all only active prioritise sound Tab audio Only the active tab plays, or other tabs are muted while the active tab is playing.'),

  choice('tabs_pinned_close', 'pinned', 'close', [['close', 'Close', 'Close as other tabs'], ['keep', 'Keep', 'Do not close'], ['minimise', 'Minimise', 'Minimise instead of close']], 'Close pinned',
    '', 'pinned tabs close do not close minimize minimise Closing a pinned tab What closing a pinned tab does. Minimise puts it to sleep and moves on.'),
  toggle('tabs_pinned_separator', 'pinned', false, 'Separator', '', 'pinned tabs separator divider Separator below pinned tabs A thin line marks where the pinned tabs end.'),
  toggle('tabs_pinned_restrict', 'pinned', false, 'Stay on site', '', 'restrict pinned tabs current site link Keep pinned tabs on their site A link to another site opens in a new tab and the pinned tab stays where it is.'),

  toggle('tabs_lazy_restore', 'restore', true, 'Lazy restore', '', 'lazy load restored tabs session restore Load restored tabs when first selected Off loads every tab at once when Bowser reopens them.'),
  toggle('tabs_load_pinned', 'restore', false, 'Load pinned', '', 'always load pinned tabs restore Always load pinned tabs Pinned tabs load as soon as Bowser reopens them, even when the others wait.'),

  // --- tab stacks, tiling and workspaces (T10; src/stacks.rs) --- a stack is a site island.
  choice('tabs_stack_style', 'stacks', 'islands', [['islands', 'Islands'], ['compact', 'Compact'], ['accordion', 'Accordion'], ['two-level', 'Two-level'], ['off', 'Off']], 'Groups',
    '',
    'tab stacks stacking islands groups compact accordion two level second row fold off Tab groups How grouped tabs are drawn: side by side, compact, accordion, on a second row, or off.'),
  toggle('tabs_workspaces', 'stacks', true, 'Workspaces', '',
    'workspaces enable disable rail switcher Workspaces Shows workspaces in the rail, the sidebar, the menu and Quick commands. Off hides them; their tabs wait until you turn them back on.'),
  toggle('tabs_stack_open_in_current', 'stacks', false, 'Join group', '', 'open new tab current stack join Open new tabs in the current stack A new tab opened while a stacked tab is in front joins that stack.'),
  toggle('tabs_stack_drag', 'stacks', true, 'Drag to group', '', 'stack drag drop tab onto another Stack tabs by dragging one onto another Hold a dragged tab over the middle of another tab, then let go, to stack the two.'),
  number('tabs_stack_drop_delay', 'stacks', 400, 0, 2000, 'ms', 'Group delay', '', 'stacking drop delay milliseconds hold wait Stacking delay How long a dragged tab rests over another before a drop stacks them.', 'At once'),
  toggle('tabs_stack_editor', 'stacks', false, 'Name on create', '', 'stack editor name colour rename auto open Open the stack editor when a stack is made Making a stack by dragging or with a Stack command opens its name and colour.'),
  toggle('tabs_tile_titles', 'stacks', false, 'Tile titles', '', 'tile title bar split view tiling header Show a title bar on each tile Tiled tabs in a split view each show their page’s title above the page.'),
  toggle('tabs_tile_tight', 'stacks', false, 'Tight tiles', '', 'tile gap space spacing tight split view tiling Remove the space around tiles Tiles meet with a hairline instead of the usual gap.'),
  toggle('tabs_workspace_scroll', 'stacks', false, 'Wheel switch', '', 'switch workspaces scroll wheel mouse rail Switch workspaces by scrolling The mouse wheel over the workspaces in the rail or the sidebar switches to the next or previous one.'),
  toggle('tabs_workspace_counts', 'stacks', false, 'Tab counts', '', 'workspace tab count number badge Show tab counts on workspaces Each workspace in the rail and the workspace menu shows how many tabs it holds.'),
  // --- two-level stacks (T10b) --- only two-level stacks have a second row to make smaller.
  { ...toggle('tabs_stack_compact_row', 'stacks', false, 'Compact row', '',
    'two level stacks second row compact small smaller Compact second row Two-level stacks: the row with the tabs of the stack you are in is smaller.'), when: ['tabs_stack_style', 'two-level'] },
  // --- end tab stacks, tiling and workspaces ---
];

export const TAB_OPTION_KEYS: readonly string[] = [...TAB_OPTIONS.map((spec) => spec.key)];

const specOf = (key: string): TabOptionSpec | undefined => TAB_OPTIONS.find((spec) => spec.key === key);

/** The defaults (what a fresh profile has), keyed like the snapshot. */
export const TAB_OPTION_DEFAULTS: TabOptionValues = Object.freeze(Object.fromEntries(TAB_OPTIONS.map((spec) => [spec.key, spec.default])));

/** Whether `value` is a legal value for `spec`. */
export function validValue(spec: TabOptionSpec, value: unknown): value is OptionValue {
  switch (spec.kind) {
    case 'toggle': return typeof value === 'boolean';
    case 'choice': return typeof value === 'string' && spec.choices.some(([choiceValue]) => choiceValue === value);
    case 'number': return typeof value === 'number' && Number.isInteger(value) && value >= spec.min && value <= spec.max;
    case 'text': return typeof value === 'string' && value.length <= spec.max && !/[\u0000-\u001f\u007f]/.test(value) && (value === '' || /^https?:\/\//i.test(value));
  }
}

/** Strict check of the snapshot's `tabOptions` object: every option present, none extra, every value legal. */
export function parseTabOptions(value: unknown): TabOptionValues | null {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record);
  if (keys.length !== TAB_OPTION_KEYS.length || keys.some((key) => !TAB_OPTION_KEYS.includes(key))) return null;
  const out: Record<string, OptionValue> = {};
  for (const spec of TAB_OPTIONS) {
    const raw = record[spec.key];
    if (!validValue(spec, raw)) return null;
    out[spec.key] = raw;
  }
  return out;
}

/** The `key=value` text of a change, or null for anything Bowser would refuse. */
export function assignmentText(key: string, value: unknown): string | null {
  const spec = specOf(key);
  if (!spec || !validValue(spec, value)) return null;
  return `${key}=${String(value)}`;
}

/** Whether a text option applies now (the new tab address only while the page is a specific page). */
export function textApplies(spec: TextSpec, values: TabOptionValues): boolean {
  return values[spec.when[0]] === spec.when[1];
}

/** --- two-level stacks (T10b) --- whether a switch applies now ("Compact second row" only with two-level stacks). */
export function toggleApplies(spec: ToggleSpec, values: TabOptionValues): boolean {
  return spec.when === undefined || values[spec.when[0]] === spec.when[1];
}

/** The words next to a number: "Never ask", "Off", "3 tabs", "120 px". */
export function numberLabel(spec: NumberSpec, value: number): string {
  if (value === 0 && spec.zero !== undefined) return spec.zero;
  return `${value} ${spec.unit === 'tabs' && value === 1 ? 'tab' : spec.unit === 'days' && value === 1 ? 'day' : spec.unit}`;
}

/** What a typed number becomes: whole, clamped, and 1 stepped to 2 for the confirmation (1 is no bulk close). */
export function clampNumber(spec: NumberSpec, typed: number): number {
  let n = Math.min(spec.max, Math.max(spec.min, Math.round(Number.isFinite(typed) ? typed : spec.default)));
  if (spec.key === 'tabs_confirm_close' && n === 1) n = 2;
  return n;
}

/** Row specs for the page's search index and its row builder (the settings page lists ROWS in this order). */
export function tabOptionRows(): ReadonlyArray<{ id: string; section: 'tabs' | 'stacks'; label: string; description: string; keywords: string }> {
  // --- T10 --- the stacks card is its own settings section (Tabs and start page > Tab stacks, tiling and workspaces).
  return TAB_OPTIONS.map((spec) => ({ id: spec.key, section: spec.card === 'stacks' ? 'stacks' as const : 'tabs' as const, label: spec.label, description: spec.description, keywords: spec.keywords }));
}

/** The card a spec belongs in, with the specs of that card in page order. */
export function cardSpecs(card: TabCardKey): readonly TabOptionSpec[] {
  return TAB_OPTIONS.filter((spec) => spec.card === card);
}
