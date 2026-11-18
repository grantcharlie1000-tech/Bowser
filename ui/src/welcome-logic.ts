// Pure logic for the first-run welcome (`bowser://settings/welcome`, docs/design/welcome.md): the
// steps, their words, which choice each step shows as picked for a settings snapshot, and where Back
// and Continue go. No DOM here; test/settings-page.mjs covers it.
//
// The flow borrows from three browsers' first runs: Zen's (pick a tab layout and a look, each drawn
// as a small picture), Helium's (privacy is already on, say so plainly, ask almost nothing) and
// Dia's (bring your browsing over first, offer to be the default, end on a few keys worth knowing).

export const WELCOME_STEPS = ['hello', 'import', 'look', 'search', 'privacy', 'default', 'done'] as const;
export type WelcomeStep = typeof WELCOME_STEPS[number];

export interface StepCopy {
  readonly title: string;
  readonly lead: string;
}

export const STEP_COPY: Readonly<Record<WelcomeStep, StepCopy>> = {
  hello: { title: 'Welcome to Bowser', lead: '' },
  import: { title: 'Import from', lead: 'Bookmarks and search settings.' },
  look: { title: 'Look', lead: '' },
  search: { title: 'Search with', lead: '' },
  privacy: { title: 'Ad blocking', lead: '' },
  default: { title: 'Make Bowser your default', lead: '' },
  done: { title: 'You’re all set', lead: '' },
};

/** The steps' short names, for the progress dots' labels. */
export const STEP_NAMES: Readonly<Record<WelcomeStep, string>> = {
  hello: 'Welcome', import: 'Import', look: 'Look', search: 'Search', privacy: 'Ad blocking', default: 'Default browser', done: 'Done',
};

export const START_LABEL = 'Get started';
export const SKIP_SETUP_LABEL = 'Skip setup';
export const CONTINUE_LABEL = 'Continue';
export const NOT_NOW_LABEL = 'Not now';
export const BACK_LABEL = 'Back';
export const FINISH_LABEL = 'Start browsing';
export const SETTINGS_LABEL = 'Open Settings';
export const IMPORT_FILE_LABEL = 'File';
export const IMPORT_OPENED = 'Importing…';
export const DEFAULT_LABEL = 'Set as default';
export const DEFAULT_ALREADY = 'Bowser is your default browser.';
export const DEFAULT_ASKED = 'Pick Bowser in Windows settings.';
export const READ_ONLY_NOTE = 'Settings could not be read. Choices won’t be saved.';

/** Where Back and Continue go from `step`; `null` past either end. */
export function stepAfter(step: WelcomeStep): WelcomeStep | null {
  const at = WELCOME_STEPS.indexOf(step);
  return WELCOME_STEPS[at + 1] ?? null;
}

export function stepBefore(step: WelcomeStep): WelcomeStep | null {
  const at = WELCOME_STEPS.indexOf(step);
  return at > 0 ? WELCOME_STEPS[at - 1] ?? null : null;
}

/** "Step 2 of 7", for the progress dots' accessible name. */
export function progressText(step: WelcomeStep): string {
  return `Step ${WELCOME_STEPS.indexOf(step) + 1} of ${WELCOME_STEPS.length}`;
}

/** The first and last steps have their own buttons; the ones between get Back and Continue. */
export function hasBack(step: WelcomeStep): boolean {
  return step !== 'hello' && step !== 'done';
}

// ---- Choices ----

/** Where the tabs are, for the Look step's preview window. */
export type TabChoice = 'top' | 'side';

/** The tab position a saved layout draws in the preview: `null` for tabs along the bottom, which is neither. */
export function tabChoiceOf(tabLayout: string): TabChoice | null {
  if (tabLayout === 'vertical' || tabLayout === 'vertical-right') return 'side';
  if (tabLayout === 'bottom') return null;
  return 'top';
}

export type LookChoice = 'light' | 'dark' | 'system';
export const LOOK_CHOICES: ReadonlyArray<readonly [LookChoice, string]> = [['light', 'Light'], ['dark', 'Dark'], ['system', 'System']];

/** The look a theme schedule and look show as picked; `null` while a time-of-day schedule is in use. */
export function lookChoiceOf(schedule: string, look: string): LookChoice | null {
  if (schedule === 'system') return 'system';
  if (schedule === 'none') return look === 'dark' ? 'dark' : 'light';
  return null;
}

/** Accent swatches: the Windows accent (empty) first, then a few that read on light and dark glass. */
export const ACCENTS: ReadonlyArray<readonly [string, string]> = [
  ['', 'Windows accent'],
  ['#2f7cf6', 'Blue'],
  ['#7c5cff', 'Violet'],
  ['#0f9d8a', 'Teal'],
  ['#d9822b', 'Amber'],
  ['#e0457b', 'Rose'],
  ['#5c6b7a', 'Slate'],
];

/** The ad blocking answer: the blocking level `ads` (Bowser's default) or `none`. The word before the comma is strong. */
export type BlockChoice = 'yes' | 'no';
export const BLOCK_CHOICES: ReadonlyArray<readonly [BlockChoice, string, string]> = [
  ['yes', 'Yes', ', block ads for me.'],
  ['no', 'No', ', show me everything.'],
];

/** The browsers the Import step offers, by the `importfrom` key Rust takes (import_panel::BROWSERS). */
export type ImportBrowser = 'chrome' | 'edge' | 'brave' | 'firefox' | 'opera' | 'vivaldi' | 'helium';
export const IMPORT_BROWSERS: ReadonlyArray<readonly [ImportBrowser, string]> = [
  ['chrome', 'Chrome'], ['edge', 'Edge'], ['brave', 'Brave'], ['firefox', 'Firefox'], ['opera', 'Opera'], ['vivaldi', 'Vivaldi'], ['helium', 'Helium'],
];

/** The logos fanned out on the Import step's transfer picture before a browser is picked. */
export const IMPORT_FAN: readonly ImportBrowser[] = ['firefox', 'chrome', 'edge'];

/** What the Search step's preview bar says for an engine's name. */
export function searchPlaceholder(name: string): string {
  return `Search ${name} or type an address`;
}

/** The built-in engines the Search step offers, by their ids in search_engines.rs. Ones missing from the
 * address snapshot's list (removed by the person) are not offered. */
export const ENGINE_CHOICES: ReadonlyArray<readonly [string, string]> = [
  ['google', 'Google'], ['bing', 'Bing'], ['duckduckgo', 'DuckDuckGo'], ['brave', 'Brave'],
];

/** The engines to show for the snapshot's engine ids, in the welcome's order. */
export function offeredEngines(ids: readonly string[]): ReadonlyArray<readonly [string, string]> {
  return ENGINE_CHOICES.filter(([id]) => ids.includes(id));
}

export interface Shortcut {
  readonly keys: readonly string[];
  readonly label: string;
}

/** The keys the last step teaches (docs/shortcuts.md). */
export const SHORTCUTS: readonly Shortcut[] = [
  { keys: ['Ctrl', 'L'], label: 'Address bar' },
  { keys: ['Ctrl', 'T'], label: 'New tab' },
  { keys: ['Ctrl', 'Shift', 'K'], label: 'Commands' },
  { keys: ['Ctrl', 'Shift', 'T'], label: 'Reopen tab' },
];

/** What the Default step says for the snapshot's `defaultBrowser` and whether the page asked Windows. */
export function defaultStatus(state: string, asked: boolean): string {
  if (state === 'yes') return DEFAULT_ALREADY;
  return asked ? DEFAULT_ASKED : '';
}
