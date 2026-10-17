// Pure logic for the Window appearance and Panels options (thread T9): the `window` object of the
// settings snapshot, its save lines, and the helpers the settings page and the chrome share.
// Mirrors src/window_prefs.rs. No DOM here; test/window.mjs covers it.

/** The `window` object of the settings snapshot: one boolean per option. */
export interface WindowSettings {
  animations: boolean;
  rangeButtons: boolean;
  fadeBackground: boolean;
  simpleScrollbars: boolean;
  monoIcons: boolean;
  panelCloseButton: boolean;
  panelFolderCounts: boolean;
  panelAutoClose: boolean;
}

export const DEFAULT_WINDOW: Readonly<WindowSettings> = {
  animations: true,
  rangeButtons: false,
  fadeBackground: false,
  simpleScrollbars: false,
  monoIcons: false,
  panelCloseButton: true,
  panelFolderCounts: true,
  panelAutoClose: true,
};

/** Snapshot key -> settings.txt key, in the order Rust lists them. */
export const WINDOW_FILE_KEYS: Readonly<Record<keyof WindowSettings, string>> = {
  animations: 'animations',
  rangeButtons: 'range_buttons',
  fadeBackground: 'fade_background',
  simpleScrollbars: 'simple_scrollbars',
  monoIcons: 'mono_icons',
  panelCloseButton: 'panel_close_button',
  panelFolderCounts: 'panel_folder_counts',
  panelAutoClose: 'panel_auto_close',
};
const NAMES = Object.keys(WINDOW_FILE_KEYS) as Array<keyof WindowSettings>;
export const WINDOW_APPEARANCE_KEYS: ReadonlyArray<keyof WindowSettings> = ['animations', 'rangeButtons', 'fadeBackground', 'simpleScrollbars', 'monoIcons'];
export const WINDOW_PANEL_KEYS: ReadonlyArray<keyof WindowSettings> = ['panelCloseButton', 'panelFolderCounts', 'panelAutoClose'];

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);

/** Rejects a partial or extended `window` object. */
export function parseWindowSettings(value: unknown): WindowSettings | null {
  if (!isRecord(value) || Object.keys(value).length !== NAMES.length) return null;
  const out: Partial<WindowSettings> = {};
  for (const name of NAMES) {
    const v = value[name];
    if (typeof v !== 'boolean') return null;
    out[name] = v;
  }
  return out as WindowSettings;
}

/** The save lines, in Rust's order. */
export function windowSaveLines(window: WindowSettings): string[] {
  return NAMES.map((name) => `${WINDOW_FILE_KEYS[name]}=${window[name]}`);
}

/** A copy of `window` with the keys of `group` back at their defaults (the section's Reset button). */
export function resetWindowGroup(window: WindowSettings, group: ReadonlyArray<keyof WindowSettings>): WindowSettings {
  const next = { ...window };
  for (const key of group) next[key] = DEFAULT_WINDOW[key];
  return next;
}

/** Whether every key of `group` is at its default (the Reset button is disabled then). */
export function windowGroupIsDefault(window: WindowSettings, group: ReadonlyArray<keyof WindowSettings>): boolean {
  return group.every((key) => window[key] === DEFAULT_WINDOW[key]);
}

/**
 * The `look` object of a chrome `state` message and the attributes it sets on the page root.
 * `bg` is true while the window is not in front.
 */
export interface Look {
  readonly motion: boolean;
  readonly scroll: boolean;
  readonly mono: boolean;
  readonly fade: boolean;
  readonly bg: boolean;
  readonly panelClose: boolean;
  readonly folderCounts: boolean;
}

export const LOOK_KEYS = ['motion', 'scroll', 'mono', 'fade', 'bg', 'panelClose', 'folderCounts'] as const;

export function parseLook(value: unknown): Look | null {
  if (!isRecord(value) || Object.keys(value).length !== LOOK_KEYS.length) return null;
  for (const key of LOOK_KEYS) if (typeof value[key] !== 'boolean') return null;
  return value as unknown as Look;
}

/** `[data-*]` name and value for each look field, so the CSS has one switch per option. */
export function lookAttributes(look: Look): ReadonlyArray<readonly [string, string]> {
  return [
    ['motion', look.motion ? 'on' : 'off'],
    ['scroll', look.scroll ? 'simple' : 'default'],
    ['icons', look.mono ? 'mono' : 'default'],
    // Fading applies only while the window is in the background.
    ['fade', look.fade && look.bg ? 'on' : 'off'],
    ['panelClose', look.panelClose ? 'on' : 'off'],
    ['folderCounts', look.folderCounts ? 'on' : 'off'],
  ];
}

/** A range control as stepper buttons: the next value for a press, kept in [min, max]. */
export function stepValue(value: number, direction: -1 | 1, step: number, min: number, max: number): number {
  const next = Math.round((value + direction * step) / step) * step;
  return Math.min(max, Math.max(min, next));
}
