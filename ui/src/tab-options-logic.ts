export type OptionValue = string | number | boolean;
export type TabOptionValues = { readonly [key: string]: OptionValue };

interface Base {
  readonly key: string;
  readonly label: string;
  readonly aria?: string;
  readonly description: string;
  readonly keywords: string;
  readonly card: TabCardKey;
}
export interface ToggleSpec extends Base {
  readonly kind: 'toggle';
  readonly default: boolean;
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
  readonly zero?: string;
  readonly unit: string;
}
export interface TextSpec extends Base {
  readonly kind: 'text';
  readonly default: string;
  readonly max: number;
  readonly placeholder: string;
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
  stacks: 'Workspaces and tiles',
};

const toggle = (key: string, card: TabCardKey, def: boolean, label: string, description: string, keywords: string, aria?: string): ToggleSpec =>
  ({ kind: 'toggle', key, card, default: def, label, description, keywords, ...(aria === undefined ? {} : { aria }) });
const choice = (key: string, card: TabCardKey, def: string, choices: ChoiceSpec['choices'], label: string, description: string, keywords: string): ChoiceSpec =>
  ({ kind: 'choice', key, card, default: def, choices, label, description, keywords });
const number = (key: string, card: TabCardKey, def: number, min: number, max: number, unit: string, label: string, description: string, keywords: string, zero?: string): NumberSpec =>
  ({ kind: 'number', key, card, default: def, min, max, unit, label, description, keywords, ...(zero === undefined ? {} : { zero }) });

export const POSITION_KEY = 'tabs_new_tab_position';
export const MAX_PAGE_URL = 2048;

export const TAB_OPTIONS: readonly TabOptionSpec[] = [
  choice('tabs_new_tab_page', 'new', 'start', [['start', 'Start', 'Start page'], ['homepage', 'Homepage'], ['blank', 'Blank', 'Blank page'], ['url', 'Page', 'Specific page']], 'New tab page',
    '', 'new tab page start homepage blank url speed dial New tab page What a new tab opens: the start page, your homepage, a blank page, or an address.'),
  {
    kind: 'text', key: 'tabs_new_tab_url', card: 'new', default: '', max: MAX_PAGE_URL, placeholder: '', when: ['tabs_new_tab_page', 'url'],
    label: 'Page address', description: '', keywords: 'new tab address url website Address of the new tab page Opened by every new tab while the page above is a specific page.',
  },
  choice(POSITION_KEY, 'new', 'active', [['related', 'Related', 'After related'], ['active', 'Active', 'After active'], ['last', 'Last', 'As last'], ['stack', 'Group', 'Grouped with related']], 'New tab position',
    '', 'new tab position related active last order next beside group grouped stack New tab position Where a new tab goes. After related keeps tabs opened from a page next to it; grouped with related also puts them in its group.'),
  toggle('tabs_link_background', 'new', false, 'Links in background', '', 'link background new tab open Background Open links in new tabs in the background A link opened in a new tab loads without leaving the page you are reading.',
    'Open links in new tabs in the background'),
  choice('tabs_duplicate_position', 'new', 'beside', [['beside', 'Beside', 'Next to the original'], ['last', 'Last']], 'Duplicated tab',
    '', 'duplicate tab position copy Duplicate Duplicated tab position Where Duplicate tab puts the copy.'),
  toggle('tabs_new_tab_in_group', 'new', true, 'Keep in group', '', 'new tab group tab group join keep ctrl+t plus button Open new tabs in the current tab group A new tab opened while you are on a tab in a group (Ctrl+T, the + button, New tab to the right) joins that group.',
    'Open new tabs in the current tab group'),
  toggle('tabs_focus_page', 'new', false, 'Focus page', '', 'focus page content new tab address bar keyboard Focus the page on a new tab A new tab that opens your homepage or a set page puts the keyboard in the page instead of the address bar.',
    'Focus the page, not the address bar, on a new tab'),

  toggle('tabs_close_related', 'close', false, 'Back to opener', '', 'close tab activation related opener Related tab Return to the related tab when closing Closing a tab activates the tab that opened it, when it is still open.',
    'Return to the tab that opened it when closing'),
  choice('tabs_close_activation', 'close', 'right', [['right', 'Right', 'Tab on the right'], ['left', 'Left', 'Tab on the left'], ['recent', 'Recent', 'Recently used']], 'After close',
    '', 'close tab activation recently used left right Tab to activate after closing Which tab is shown when the active tab closes and no related tab applies.'),
  toggle('tabs_align_close_button', 'close', false, 'Close under pointer', '', 'align close button pointer cursor Close in place Keep the next close button under the pointer After closing a tab with the mouse, the tabs keep their width until the pointer leaves the tab bar, so several tabs close in a row.',
    'Keep the next close button under the pointer'),
  toggle('tabs_keep_window', 'close', true, 'Keep window', '', 'last tab close window keep open Keep the window open when the last tab closes Off closes the window with its last tab.',
    'Keep the window open when the last tab closes'),
  number('tabs_confirm_close', 'close', 0, 0, 99, 'tabs', 'Confirm close', '', 'confirm closing multiple tabs warn Confirm when closing many tabs Asks first when you close this many tabs at once, such as with Close other tabs.', 'Never ask'),
  choice('tabs_double_click', 'close', 'ignore', [['ignore', 'Nothing'], ['rename', 'Rename'], ['close', 'Close']], 'Double-click',
    '', 'double click tab rename close Double click on a tab What a double click on a tab does.'),
  toggle('tabs_close_left', 'close', false, 'Close on left', '', 'close button left side Close button on the left side Puts each tab’s close button before its title.',
    'Close button on the left side'),
  toggle('tabs_close_always', 'close', false, 'Close on every tab', '', 'close button permanent always visible Always close Always show the close button The close button shows on every tab, not just the active and hovered ones.',
    'Always show the close button'),

  toggle('tabs_scroll', 'bar', false, 'Keep tabs wide', '', 'horizontal scrolling scrollable tabs overflow width wide readable Scroll tabs Scroll the tab bar horizontally Tabs keep a readable width and the bar scrolls as soon as they do not fit, instead of shrinking them first.',
    'Keep tabs at a readable width and scroll the tab bar sooner'),
  number('tabs_active_min_width', 'bar', 0, 0, 400, 'px', 'Active tab min width', '', 'active tab minimum width size Active width Active tab minimum width The narrowest the active tab gets.', 'Automatic'),
  toggle('tabs_wheel_switch', 'bar', false, 'Wheel switches tabs', '', 'switch tabs scroll wheel mouse Wheel switch Switch tabs by scrolling The mouse wheel over the tab bar switches to the next or previous tab.',
    'Switch tabs by scrolling over the tab bar'),
  toggle('tabs_remove_spacing_maximised', 'bar', false, 'No gap when maximised', '', 'remove spacing gap maximized maximised Tight when max Remove tab spacing when maximised Tabs fill the height of the tab bar while the window is maximised, with no gap above them.',
    'Remove tab spacing when maximised'),
  choice('tabs_load_indicator', 'bar', 'spinner', [['spinner', 'Spinner', 'Favicon spinner'], ['bar', 'Bar'], ['favicon', 'Progress', 'Favicon progress'], ['none', 'None']], 'Load progress',
    '', 'page load progress bar spinner favicon indicator loading Page load progress How a tab shows that its page is loading.'),
  toggle('tabs_minimise_active', 'bar', false, 'Click active to go back', '', 'minimize minimise active tab click previous cycling Click to previous Minimise the active tab Clicking the active tab goes back to the tab you used before it.',
    'Clicking the active tab goes back to the tab used before it'),
  toggle('tabs_unread_indicator', 'bar', true, 'Unread dots', '', 'unread indicator dot background Show unread indicators A tab that changed while you were elsewhere carries a dot until you open it.',
    'Show unread indicators'),
  toggle('tabs_title_notifications', 'bar', true, 'Title counts as unread', '', 'page title notifications count alert Title alerts Detect page title notifications A background tab whose title gains a count, such as “(3) Inbox”, gets the unread dot.',
    'Mark a tab unread when a count appears in its title'),
  toggle('tabs_dim_hibernated', 'bar', true, 'Dim sleeping', '', 'dim icon hibernated sleeping tabs Dim the icon of hibernated tabs A tab whose page is asleep shows a dimmed icon.',
    'Dim the icon of sleeping tabs'),
  number('tabs_inactive_days', 'bar', 0, 0, 365, 'days', 'Fade unused tabs after', '', 'inactive tabs days old unused threshold Inactive days Inactive tabs threshold Tabs not used for this many days are shown dimmed.', 'Off'),

  toggle('tabs_selection', 'select', true, 'Multi-select', '', 'tab selection multiple select ctrl shift Select tabs with Ctrl and Shift Ctrl+click adds a tab to the selection and Shift+click selects a range. Tab actions then apply to the selection.',
    'Select tabs with Ctrl and Shift click'),
  toggle('tabs_selection_active', 'select', true, 'Include active tab', '', 'tab selection include active tab Include active Include the active tab in the selection The active tab joins the selected tabs when you act on them.',
    'Include the active tab in the selection'),

  choice('tabs_mute_mode', 'audio', 'all', [['all', 'All', 'Play all'], ['active', 'Active', 'Only the active tab'], ['prioritise', 'Priority', 'Prioritise the active tab']], 'Tab audio',
    '', 'mute tab audio play all only active prioritise sound Tab audio Only the active tab plays, or other tabs are muted while the active tab is playing.'),

  choice('tabs_pinned_close', 'pinned', 'close', [['close', 'Close', 'Close as other tabs'], ['keep', 'Keep', 'Do not close'], ['minimise', 'Minimise', 'Minimise instead of close']], 'Close pinned',
    '', 'pinned tabs close do not close minimize minimise Closing a pinned tab What closing a pinned tab does. Minimise puts it to sleep and moves on.'),
  toggle('tabs_pinned_separator', 'pinned', false, 'Pinned separator', '', 'pinned tabs separator divider Separator Separator below pinned tabs A thin line marks where the pinned tabs end.',
    'Separator after pinned tabs'),
  toggle('tabs_pinned_restrict', 'pinned', false, 'Stay on site', '', 'restrict pinned tabs current site link Keep pinned tabs on their site A link to another site opens in a new tab and the pinned tab stays where it is.',
    'Keep pinned tabs on their site'),

  toggle('tabs_lazy_restore', 'restore', true, 'Load tabs when selected', '', 'lazy load restored tabs session restore Lazy restore Load restored tabs when first selected Off loads every tab at once when Bowser reopens them.',
    'Load restored tabs when first selected'),
  toggle('tabs_load_pinned', 'restore', false, 'Load pinned at once', '', 'always load pinned tabs restore Load pinned Always load pinned tabs Pinned tabs load as soon as Bowser reopens them, even when the others wait.',
    'Always load pinned tabs at once'),

  toggle('tabs_workspaces', 'stacks', true, 'Workspaces', '',
    'workspaces enable disable rail switcher Workspaces Shows workspaces in the rail, the sidebar, the menu and Quick commands. Off hides them; their tabs wait until you turn them back on.',
    'Use workspaces'),
  toggle('tabs_tile_titles', 'stacks', false, 'Tile titles', '', 'tile title bar split view tiling header Show a title bar on each tile Tiled tabs in a split view each show their page’s title above the page.',
    'Show a title bar on each tile'),
  toggle('tabs_tile_tight', 'stacks', false, 'Tight tiles', '', 'tile gap space spacing tight split view tiling Remove the space around tiles Tiles meet with a hairline instead of the usual gap.',
    'Remove the space around tiles'),
  toggle('tabs_workspace_scroll', 'stacks', false, 'Wheel switches workspaces', '', 'switch workspaces scroll wheel mouse rail Workspace wheel Switch workspaces by scrolling The mouse wheel over the workspaces in the rail or the sidebar switches to the next or previous one.',
    'Switch workspaces by scrolling over them'),
  toggle('tabs_workspace_counts', 'stacks', false, 'Workspace tab counts', '', 'workspace tab count number badge Tab counts Show tab counts on workspaces Each workspace button in the rail shows how many tabs it holds.',
    'Show tab counts on the workspace buttons in the rail'),
];

export const TAB_OPTION_KEYS: readonly string[] = [...TAB_OPTIONS.map((spec) => spec.key)];

const specOf = (key: string): TabOptionSpec | undefined => TAB_OPTIONS.find((spec) => spec.key === key);

export const TAB_OPTION_DEFAULTS: TabOptionValues = Object.freeze(Object.fromEntries(TAB_OPTIONS.map((spec) => [spec.key, spec.default])));

export function validValue(spec: TabOptionSpec, value: unknown): value is OptionValue {
  switch (spec.kind) {
    case 'toggle': return typeof value === 'boolean';
    case 'choice': return typeof value === 'string' && spec.choices.some(([choiceValue]) => choiceValue === value);
    case 'number': return typeof value === 'number' && Number.isInteger(value) && value >= spec.min && value <= spec.max;
    case 'text': return typeof value === 'string' && value.length <= spec.max && !/[\u0000-\u001f\u007f]/.test(value) && (value === '' || /^https?:\/\//i.test(value));
  }
}

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

export function assignmentText(key: string, value: unknown): string | null {
  const spec = specOf(key);
  if (!spec || !validValue(spec, value)) return null;
  return `${key}=${String(value)}`;
}

export function textApplies(spec: TextSpec, values: TabOptionValues): boolean {
  return values[spec.when[0]] === spec.when[1];
}


export function numberLabel(spec: NumberSpec, value: number): string {
  if (value === 0 && spec.zero !== undefined) return spec.zero;
  return `${value} ${spec.unit === 'tabs' && value === 1 ? 'tab' : spec.unit === 'days' && value === 1 ? 'day' : spec.unit}`;
}

export function clampNumber(spec: NumberSpec, typed: number): number {
  let n = Math.min(spec.max, Math.max(spec.min, Math.round(Number.isFinite(typed) ? typed : spec.default)));
  if (spec.key === 'tabs_confirm_close' && n === 1) n = 2;
  return n;
}

export function tabOptionRows(): ReadonlyArray<{ id: string; section: 'tabs'; label: string; aria?: string; description: string; keywords: string }> {
  return TAB_OPTIONS.map((spec) => ({
    id: spec.key, section: 'tabs' as const, label: spec.label, ...(spec.aria === undefined ? {} : { aria: spec.aria }), description: spec.description, keywords: spec.keywords,
  }));
}

export function cardSpecs(card: TabCardKey): readonly TabOptionSpec[] {
  return TAB_OPTIONS.filter((spec) => spec.card === card);
}

export interface TabRowContext {
  readonly tabsOnSide: boolean;
  readonly homepage: string;
  readonly tabListHidden?: boolean;
}

export const TAB_LIST_KEYS: readonly string[] = [
  'tabs_scroll', 'tabs_active_min_width', 'tabs_wheel_switch', 'tabs_remove_spacing_maximised', 'tabs_load_indicator',
  'tabs_minimise_active', 'tabs_unread_indicator', 'tabs_title_notifications', 'tabs_dim_hibernated', 'tabs_inactive_days',
  'tabs_align_close_button', 'tabs_double_click', 'tabs_close_left', 'tabs_close_always', 'tabs_pinned_separator',
  'tabs_selection', 'tabs_selection_active',
];

export const STRIP_ONLY_KEYS: readonly string[] = ['tabs_scroll', 'tabs_active_min_width', 'tabs_wheel_switch', 'tabs_remove_spacing_maximised', 'tabs_align_close_button'];

export function newTabOpensPage(values: TabOptionValues, homepage: string): boolean {
  switch (values.tabs_new_tab_page) {
    case 'homepage': return homepage !== '';
    case 'url': return typeof values.tabs_new_tab_url === 'string' && values.tabs_new_tab_url !== '';
    default: return false;
  }
}

export function tabRowRests(key: string, values: TabOptionValues, context: TabRowContext): boolean {
  if (context.tabListHidden === true && TAB_LIST_KEYS.includes(key)) return true;
  if (STRIP_ONLY_KEYS.includes(key)) return context.tabsOnSide;
  switch (key) {
    case 'tabs_new_tab_url': return values.tabs_new_tab_page !== 'url';
    case 'tabs_focus_page': return !newTabOpensPage(values, context.homepage);
    case 'tabs_title_notifications': return values.tabs_unread_indicator !== true;
    case 'tabs_selection_active': return values.tabs_selection !== true;
    case 'tabs_load_pinned': return values.tabs_lazy_restore !== true;
    case 'tabs_workspace_scroll':
    case 'tabs_workspace_counts':
    case 'workspaceRules': return values.tabs_workspaces !== true;
    default: return false;
  }
}

export const NO_HOMEPAGE_TIP = 'No homepage is set, so a new tab shows the start page';
export function homepageChoiceTip(homepage: string): string {
  return homepage === '' ? NO_HOMEPAGE_TIP : '';
}

