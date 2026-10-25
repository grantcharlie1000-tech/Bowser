// The theme options of the settings page's Appearance area (thread T8): the same keys and values as
// src/theme_custom.rs (a Rust test writes the snapshot fixture this file's test parses). Each option
// is one `key=value` text sent in a `themeopt` command; Rust validates it again. No DOM here.

export type ThemeOptionValue = string | number | boolean;
export type ThemeOptionValues = { readonly [key: string]: ThemeOptionValue };

export const SCHEDULES = [['system', 'System', 'Follow Windows'], ['none', 'Fixed', 'One look'], ['time', 'Timed', 'By time']] as const;
export const LOOKS = [['light', 'Light'], ['dark', 'Dark']] as const;
export const COLOURINGS = [['off', 'Off'], ['tab_bar', 'Tabs', 'Tab bar'], ['address_bar', 'Address', 'Address bar'], ['unified', 'Both']] as const;
export const CONTRASTS = [['standard', 'Standard'], ['high', 'High']] as const;

/** The three optional colours, in the order the page lists them. An empty value is Bowser's own colour. */
export const COLOUR_KEYS = [
  ['theme_background', 'Background'],
  ['theme_foreground', 'Text'],
  ['theme_highlight', 'Highlight'],
] as const;
export type ColourKey = typeof COLOUR_KEYS[number][0];

export const TIME_KEYS = [['theme_light_from', 'Light from'], ['theme_dark_from', 'Dark from']] as const;
export type TimeKey = typeof TIME_KEYS[number][0];

export const MIN_SATURATION = 0;
export const MAX_SATURATION = 100;

type Kind = 'choice' | 'colour' | 'time' | 'toggle' | 'percent';
interface Spec {
  readonly key: string;
  readonly kind: Kind;
  readonly default: ThemeOptionValue;
  readonly choices?: ReadonlyArray<readonly [string, string] | readonly [string, string, string]>;
}

/** Every option, in the order Rust lists them. */
export const THEME_OPTIONS: readonly Spec[] = [
  { key: 'theme_schedule', kind: 'choice', default: 'system', choices: SCHEDULES },
  { key: 'theme_look', kind: 'choice', default: 'light', choices: LOOKS },
  { key: 'theme_light_from', kind: 'time', default: '07:00' },
  { key: 'theme_dark_from', kind: 'time', default: '19:00' },
  { key: 'theme_background', kind: 'colour', default: '' },
  { key: 'theme_foreground', kind: 'colour', default: '' },
  { key: 'theme_highlight', kind: 'colour', default: '' },
  { key: 'theme_colouring', kind: 'choice', default: 'off', choices: COLOURINGS },
  { key: 'theme_accent_page', kind: 'toggle', default: false },
  { key: 'theme_saturation', kind: 'percent', default: 100 },
  { key: 'theme_contrast', kind: 'choice', default: 'standard', choices: CONTRASTS },
];

export const THEME_OPTION_KEYS: readonly string[] = THEME_OPTIONS.map((spec) => spec.key);
export const THEME_OPTION_DEFAULTS: ThemeOptionValues = Object.freeze(Object.fromEntries(THEME_OPTIONS.map((spec) => [spec.key, spec.default])));

const specOf = (key: string): Spec | undefined => THEME_OPTIONS.find((spec) => spec.key === key);

export const COLOUR_RE = /^#[0-9a-fA-F]{6}$/;
export const CLOCK_RE = /^([01]\d|2[0-3]):[0-5]\d$/;

/** Whether `value` is a legal value for the option `key`. */
export function validThemeValue(key: string, value: unknown): value is ThemeOptionValue {
  const spec = specOf(key);
  if (!spec) return false;
  switch (spec.kind) {
    case 'toggle': return typeof value === 'boolean';
    case 'choice': return typeof value === 'string' && (spec.choices ?? []).some(([choice]) => choice === value);
    case 'colour': return typeof value === 'string' && (value === '' || COLOUR_RE.test(value));
    case 'time': return typeof value === 'string' && CLOCK_RE.test(value);
    case 'percent': return typeof value === 'number' && Number.isInteger(value) && value >= MIN_SATURATION && value <= MAX_SATURATION;
  }
}

/** Strict check of the snapshot's `themeOptions` object: every option present, none extra, every value legal. */
export function parseThemeOptions(value: unknown): ThemeOptionValues | null {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record);
  if (keys.length !== THEME_OPTION_KEYS.length || keys.some((key) => !THEME_OPTION_KEYS.includes(key))) return null;
  const out: Record<string, ThemeOptionValue> = {};
  for (const spec of THEME_OPTIONS) {
    const raw = record[spec.key];
    if (!validThemeValue(spec.key, raw)) return null;
    out[spec.key] = raw;
  }
  // The two schedule times never meet (Rust refuses it as well).
  if (out.theme_light_from === out.theme_dark_from) return null;
  return out;
}

/** The `key=value` text of a change, or null for anything Bowser would refuse. */
export function themeAssignment(key: string, value: unknown): string | null {
  if (!validThemeValue(key, value)) return null;
  return `${key}=${String(value)}`;
}

/** The look-choice row only matters while the schedule is "One look". */
export const lookApplies = (values: ThemeOptionValues): boolean => values.theme_schedule === 'none';
/** The times row only matters while the schedule is "By time". */
export const timesApply = (values: ThemeOptionValues): boolean => values.theme_schedule === 'time';

/** The page's rows for these options; the accent, the theme and the website look are older rows. */
export const THEME_ROW_IDS = ['themeSchedule', 'themeLook', 'themeTimes', 'themeColours', 'themeColouring', 'themeAccentPage', 'themeSaturation', 'themeContrast', 'themeFile'] as const;

/** Which option keys each row sets (a row with several: the colours and the two times). */
export const THEME_ROW_KEYS: Readonly<Record<typeof THEME_ROW_IDS[number], readonly string[]>> = {
  themeSchedule: ['theme_schedule'],
  themeLook: ['theme_look'],
  themeTimes: ['theme_light_from', 'theme_dark_from'],
  themeColours: ['theme_background', 'theme_foreground', 'theme_highlight'],
  themeColouring: ['theme_colouring'],
  themeAccentPage: ['theme_accent_page'],
  themeSaturation: ['theme_saturation'],
  themeContrast: ['theme_contrast'],
  themeFile: [],
};

export interface ThemeRowSpec {
  readonly id: typeof THEME_ROW_IDS[number];
  readonly section: 'appearance';
  readonly label: string;
  readonly aria?: string;
  readonly description: string;
  readonly keywords: string;
}

export const THEME_ROWS: readonly ThemeRowSpec[] = [
  { id: 'themeSchedule', section: 'appearance', label: 'Mode', aria: 'Light or dark', description: '', keywords: 'dark light mode schedule automatic night day time follow windows operating system none manual theme appearance Light or dark Follow Windows, keep one look, or switch at set times.' },
  { id: 'themeLook', section: 'appearance', label: 'Look', aria: 'Look without a schedule', description: '', keywords: 'light dark fixed always theme no schedule Look without a schedule The look Bowser keeps while the schedule is set to one look.' },
  { id: 'themeTimes', section: 'appearance', label: 'Schedule', aria: 'Schedule times', description: '', keywords: 'time schedule sunrise sunset night day hours light dark manual Schedule times The light look starts at the first time and the dark look at the second. Used while the schedule is by time.' },
  { id: 'themeColours', section: 'appearance', label: 'Colours', aria: 'Custom colours', description: '', keywords: 'background foreground text highlight selection colour color custom window theme Custom colours The background, text and highlight of Bowser’s own window. Text and highlights are kept readable on the colours you choose.' },
  { id: 'themeColouring', section: 'appearance', label: 'Bar tint', aria: 'Colour the bars with the accent', description: '', keywords: 'accent colouring coloring tab bar address bar unified tint colour Colour the bars with the accent The accent colour tints the tab bar, the address bar, or both.' },
  { id: 'themeAccentPage', section: 'appearance', label: 'Page accent', aria: 'Take the accent from the page', description: '', keywords: 'accent colour webpage page theme-color favicon website Take the accent from the page Uses the page’s own theme colour, or its icon’s colour when it has none. Nothing is fetched for it.' },
  { id: 'themeSaturation', section: 'appearance', label: 'Saturation', aria: 'Limit accent saturation', description: '', keywords: 'accent saturation vivid muted colour limit Limit accent saturation Keeps a bright accent from being too vivid. 100% leaves it as it is.' },
  { id: 'themeContrast', section: 'appearance', label: 'Contrast', aria: 'Colour contrast', description: '', keywords: 'contrast accessibility readable text high wcag colour Colour contrast Standard keeps text readable (WCAG AA). High asks for more wherever the colours allow it.' },
  { id: 'themeFile', section: 'appearance', label: 'Theme file', description: '', keywords: 'theme export import file reset defaults share backup colours Theme file Export your colours to a file, import one, or put every theme setting back to Bowser’s own.' },
];

/** "07:30" as minutes since midnight. */
export function clockMinutes(text: string): number | null {
  const match = CLOCK_RE.exec(text);
  return match ? Number(text.slice(0, 2)) * 60 + Number(text.slice(3)) : null;
}

/** The phrase the schedule row states under its control: what Bowser will look like now. */
export function scheduleNote(values: ThemeOptionValues): string {
  switch (values.theme_schedule) {
    case 'none': return values.theme_look === 'dark' ? 'Always dark.' : 'Always light.';
    case 'time': return `Light from ${String(values.theme_light_from)}, dark from ${String(values.theme_dark_from)}.`;
    default: return 'Follows the Windows app mode.';
  }
}

/** "40%". */
export const percentText = (value: number): string => `${value}%`;
