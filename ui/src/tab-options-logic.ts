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
  readonly choices: ReadonlyArray<readonly [value: string, label: string]>;
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
  new: 'New tabs',
  close: 'Closing tabs',
  bar: 'Tab bar',
  select: 'Selecting tabs',
  audio: 'Audio',
  pinned: 'Pinned tabs',
  restore: 'Restoring tabs',
  stacks: 'Tab stacks, tiling and workspaces', // --- T10 ---
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
  choice('tabs_new_tab_page', 'new', 'start', [['start', 'Start page'], ['homepage', 'Homepage'], ['blank', 'Blank page'], ['url', 'Specific page']], 'New tab page',
    'What a new tab opens. Homepage is the one set in Start page; while none is set, the start page shows. Blank page shows nothing, not even the start page.', 'new tab page start homepage blank url speed dial'),
  {
    kind: 'text', key: 'tabs_new_tab_url', card: 'new', default: '', max: MAX_PAGE_URL, placeholder: 'https://example.com', when: ['tabs_new_tab_page', 'url'],
    label: 'Address of the new tab page', description: 'Opened by every new tab while the page above is a specific page.', keywords: 'new tab address url website',
  },
  choice(POSITION_KEY, 'new', 'active', [['related', 'After related'], ['active', 'After active'], ['last', 'As last'], ['stack', 'As stack with related']], 'New tab position',
    'Where a new tab goes. After related keeps tabs opened from a page next to it; as stack with related also puts them in its stack.', 'new tab position related active last order next beside stack'),
  toggle('tabs_link_background', 'new', false, 'Open links in new tabs in the background', 'A link opened in a new tab loads without leaving the page you are reading.', 'link background new tab open'),
  choice('tabs_duplicate_position', 'new', 'beside', [['beside', 'Next to the original'], ['last', 'Last']], 'Duplicated tab position',
    'Where Duplicate tab puts the copy.', 'duplicate tab position copy'),
  toggle('tabs_focus_page', 'new', false, 'Focus the page on a new tab', 'A new tab with an address puts the keyboard in the page instead of the address bar.', 'focus page content new tab address bar keyboard'),

  toggle('tabs_close_related', 'close', false, 'Return to the related tab when closing', 'Closing a tab activates the tab that opened it, when it is still open.', 'close tab activation related opener'),
  choice('tabs_close_activation', 'close', 'right', [['right', 'Tab on the right'], ['left', 'Tab on the left'], ['recent', 'Recently used']], 'Tab to activate after closing',
    'Which tab is shown when the active tab closes and no related tab applies.', 'close tab activation recently used left right'),
  toggle('tabs_align_close_button', 'close', false, 'Keep the next close button under the pointer', 'After closing a tab with the mouse, the tabs keep their width until the pointer leaves the tab bar, so several tabs close in a row.', 'align close button pointer cursor'),
  toggle('tabs_keep_window', 'close', true, 'Keep the window open when the last tab closes', 'Off closes the window with its last tab.', 'last tab close window keep open'),
  number('tabs_confirm_close', 'close', 0, 0, 99, 'tabs', 'Confirm when closing many tabs', 'Asks first when you close this many tabs at once, such as with Close other tabs.', 'confirm closing multiple tabs warn', 'Never ask'),
  choice('tabs_double_click', 'close', 'ignore', [['ignore', 'Nothing'], ['rename', 'Rename'], ['close', 'Close']], 'Double click on a tab',
    'What a double click on a tab does.', 'double click tab rename close'),
  toggle('tabs_close_left', 'close', false, 'Close button on the left side', 'Puts each tab’s close button before its title.', 'close button left side'),
  toggle('tabs_close_always', 'close', false, 'Always show the close button', 'The close button shows on every tab, not just the active and hovered ones.', 'close button permanent always visible'),

  toggle('tabs_scroll', 'bar', false, 'Scroll the tab bar horizontally', 'Tabs keep a readable width and the bar scrolls when they do not fit.', 'horizontal scrolling scrollable tabs overflow'),
  toggle('tabs_remove_spacing_maximised', 'bar', false, 'Remove tab spacing when maximised', 'Takes away the gap above the tabs while the window is maximised.', 'remove spacing gap maximized maximised'),
  number('tabs_active_min_width', 'bar', 0, 0, 400, 'px', 'Active tab minimum width', 'The narrowest the active tab gets.', 'active tab minimum width size', 'Automatic'),
  choice('tabs_load_indicator', 'bar', 'spinner', [['spinner', 'Favicon spinner'], ['bar', 'Bar'], ['favicon', 'Favicon progress'], ['none', 'None']], 'Page load progress',
    'How a tab shows that its page is loading.', 'page load progress bar spinner favicon indicator loading'),
  toggle('tabs_wheel_switch', 'bar', false, 'Switch tabs by scrolling', 'The mouse wheel over the tab bar switches to the next or previous tab.', 'switch tabs scroll wheel mouse'),
  toggle('tabs_minimise_active', 'bar', false, 'Minimise the active tab', 'Clicking the active tab goes back to the tab you used before it.', 'minimize minimise active tab click previous cycling'),
  toggle('tabs_title_notifications', 'bar', true, 'Detect page title notifications', 'A title such as “(3) Inbox” puts its count on the tab.', 'page title notifications count badge'),
  toggle('tabs_unread_indicator', 'bar', true, 'Show unread indicators', 'A tab that changed while you were elsewhere carries a dot until you open it.', 'unread indicator dot background'),
  toggle('tabs_dim_hibernated', 'bar', true, 'Dim the icon of hibernated tabs', 'A tab whose page is asleep shows a dimmed icon.', 'dim icon hibernated sleeping tabs'),
  number('tabs_inactive_days', 'bar', 0, 0, 365, 'days', 'Inactive tabs threshold', 'Tabs not used for this many days are shown dimmed.', 'inactive tabs days old unused threshold', 'Off'),

  toggle('tabs_selection', 'select', true, 'Select tabs with Ctrl and Shift', 'Ctrl+click adds a tab to the selection and Shift+click selects a range. Tab actions then apply to the selection.', 'tab selection multiple select ctrl shift'),
  toggle('tabs_selection_active', 'select', true, 'Include the active tab in the selection', 'The active tab joins the selected tabs when you act on them.', 'tab selection include active tab'),

  choice('tabs_mute_mode', 'audio', 'all', [['all', 'Play all'], ['active', 'Only the active tab'], ['prioritise', 'Prioritise the active tab']], 'Tab audio',
    'Only the active tab plays, or other tabs are muted while the active tab is playing.', 'mute tab audio play all only active prioritise sound'),

  choice('tabs_pinned_close', 'pinned', 'close', [['close', 'Close as other tabs'], ['keep', 'Do not close'], ['minimise', 'Minimise instead of close']], 'Closing a pinned tab',
    'What closing a pinned tab does. Minimise puts it to sleep and moves on.', 'pinned tabs close do not close minimize minimise'),
  toggle('tabs_pinned_separator', 'pinned', false, 'Separator below pinned tabs', 'A thin line marks where the pinned tabs end.', 'pinned tabs separator divider'),
  toggle('tabs_pinned_restrict', 'pinned', false, 'Keep pinned tabs on their site', 'A link to another site opens in a new tab and the pinned tab stays where it is.', 'restrict pinned tabs current site link'),

  toggle('tabs_lazy_restore', 'restore', true, 'Load restored tabs when first selected', 'Off loads every tab at once when Bowser reopens them.', 'lazy load restored tabs session restore'),
  toggle('tabs_load_pinned', 'restore', false, 'Always load pinned tabs', 'Pinned tabs load as soon as Bowser reopens them, even when the others wait.', 'always load pinned tabs restore'),

  // --- tab stacks, tiling and workspaces (T10; src/stacks.rs) --- a stack is a site island.
  choice('tabs_stack_style', 'stacks', 'islands', [['islands', 'Islands'], ['compact', 'Compact'], ['accordion', 'Accordion'], ['two-level', 'Two-level'], ['off', 'Off']], 'Tab stacks',
    'Islands sit side by side and fold when you fold them. Compact folds each stack to its ball and the tab in use. Accordion opens only the stack you are in. Two-level shows the tabs of the stack you are in on a second row under the tab bar; with the tab bar on a side, the sidebar shows stacks as usual.',
    'tab stacks stacking islands groups compact accordion two level second row fold off'),
  toggle('tabs_workspaces', 'stacks', true, 'Workspaces', 'Shows workspaces in the rail, the sidebar, the menu and Quick commands. Off hides them; their tabs wait until you turn them back on.',
    'workspaces enable disable rail switcher'),
  toggle('tabs_stack_open_in_current', 'stacks', false, 'Open new tabs in the current stack', 'A new tab opened while a stacked tab is in front joins that stack.', 'open new tab current stack join'),
  toggle('tabs_stack_drag', 'stacks', true, 'Stack tabs by dragging one onto another', 'Hold a dragged tab over the middle of another tab, then let go, to stack the two.', 'stack drag drop tab onto another'),
  number('tabs_stack_drop_delay', 'stacks', 400, 0, 2000, 'ms', 'Stacking delay', 'How long a dragged tab rests over another before a drop stacks them.', 'stacking drop delay milliseconds hold wait', 'At once'),
  toggle('tabs_stack_editor', 'stacks', false, 'Open the stack editor when a stack is made', 'Making a stack by dragging or with a Stack command opens its name and colour.', 'stack editor name colour rename auto open'),
  toggle('tabs_tile_titles', 'stacks', false, 'Show a title bar on each tile', 'Tiled tabs in a split view each show their page’s title above the page.', 'tile title bar split view tiling header'),
  toggle('tabs_tile_tight', 'stacks', false, 'Remove the space around tiles', 'Tiles meet with a hairline instead of the usual gap.', 'tile gap space spacing tight split view tiling'),
  toggle('tabs_workspace_scroll', 'stacks', false, 'Switch workspaces by scrolling', 'The mouse wheel over the workspaces in the rail or the sidebar switches to the next or previous one.', 'switch workspaces scroll wheel mouse rail'),
  toggle('tabs_workspace_counts', 'stacks', false, 'Show tab counts on workspaces', 'Each workspace in the rail and the workspace menu shows how many tabs it holds.', 'workspace tab count number badge'),
  // --- two-level stacks (T10b) --- only two-level stacks have a second row to make smaller.
  { ...toggle('tabs_stack_compact_row', 'stacks', false, 'Compact second row', 'Two-level stacks: the row with the tabs of the stack you are in is smaller.',
    'two level stacks second row compact small smaller'), when: ['tabs_stack_style', 'two-level'] },
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
