// The settings page's theme rows (thread T8): schedule, custom colours, colouring mode, the page's
// accent, saturation, contrast and the theme file. settings-page.ts hands this module its own row
// and control helpers (so the rows look and behave like every other row) and calls `apply` with each
// snapshot. Every change is one `themeopt` command (`key=value`); Rust checks it again.
import { el } from './dom';
import { textButton } from './panels/kit';
import {
  COLOURINGS, COLOUR_KEYS, CONTRASTS, LOOKS, MAX_SATURATION, MIN_SATURATION, SCHEDULES, TIME_KEYS, lookApplies, percentText, scheduleNote, timesApply,
  type ColourKey, type ThemeOptionValue, type ThemeOptionValues, type TimeKey,
} from './theme-options-logic';

interface Ctl<T> { readonly root: HTMLElement; set(value: T): void; disable(off: boolean): void }

/** What settings-page.ts lends this module. */
export interface ThemeRowsHost {
  readonly card: HTMLElement;
  row(into: HTMLElement, id: string, control?: HTMLElement, options?: { stack?: boolean }): { readonly root: HTMLElement; readonly desc: HTMLElement | null };
  switchControl(onChange: (on: boolean) => void): Ctl<boolean>;
  segmentedControl<T extends string>(label: string, options: ReadonlyArray<readonly [T, string]>, onSelect: (value: T) => void): Ctl<T | null>;
  /** Sends one option change (`themeopt`). */
  send(key: string, value: ThemeOptionValue): void;
  /** Sends the theme file's commands. */
  action(name: 'themeexport' | 'themeimport' | 'themereset'): void;
}

export function mountThemeRows(host: ThemeRowsHost): { apply(values: ThemeOptionValues, readable: boolean): void } {
  let latest: ThemeOptionValues = {};
  let unreadable = false;

  // Theme schedule: the one core row. A line under its description says what it means now.
  const schedule = host.segmentedControl<string>('Theme schedule', SCHEDULES, (value) => host.send('theme_schedule', value));
  const scheduleRow = host.row(host.card, 'themeSchedule', schedule.root);
  const scheduleNoteText = el('span', 'sp-desc sp-schedule-note');
  scheduleRow.desc?.after(scheduleNoteText);

  const look = host.segmentedControl<string>('Look without a schedule', LOOKS, (value) => host.send('theme_look', value));
  host.row(host.card, 'themeLook', look.root);

  // Schedule times: two time fields side by side.
  const times = el('div', 'sp-theme-times');
  const timeInputs = new Map<TimeKey, HTMLInputElement>();
  for (const [key, label] of TIME_KEYS) {
    const field = el('label', 'sp-theme-field');
    const caption = el('span', 'sp-value');
    caption.textContent = label;
    const input = el('input', 'bw-input sp-time-input');
    input.type = 'time';
    input.required = true;
    input.setAttribute('aria-label', label);
    input.addEventListener('change', () => {
      // An empty or out-of-range time is put back; Rust refuses two equal times as well.
      if (!/^([01]\d|2[0-3]):[0-5]\d$/.test(input.value) || input.value === otherTime(key)) { input.value = String(latest[key] ?? ''); return; }
      host.send(key, input.value);
    });
    timeInputs.set(key, input);
    field.append(caption, input);
    times.append(field);
  }
  const otherTime = (key: TimeKey): string => String(latest[key === 'theme_light_from' ? 'theme_dark_from' : 'theme_light_from'] ?? '');
  host.row(host.card, 'themeTimes', times);

  // Custom colours: background, text and highlight, each with a swatch and a way back to Bowser's own.
  const colours = el('div', 'sp-theme-colours');
  const colourInputs = new Map<ColourKey, { input: HTMLInputElement; reset: HTMLButtonElement; value: HTMLElement }>();
  for (const [key, label] of COLOUR_KEYS) {
    const field = el('div', 'sp-theme-field');
    const caption = el('span', 'sp-value');
    caption.textContent = label;
    const input = el('input', 'sp-color');
    input.type = 'color';
    input.setAttribute('aria-label', label);
    const value = el('span', 'sp-value sp-theme-hex');
    const reset = textButton('Default', () => host.send(key, ''), { subtle: true, aria: `Use Bowser’s own ${label.toLowerCase()} colour` });
    input.addEventListener('change', () => host.send(key, input.value.toLowerCase()));
    colourInputs.set(key, { input, reset, value });
    field.append(caption, input, value, reset);
    colours.append(field);
  }
  host.row(host.card, 'themeColours', colours);

  const colouring = host.segmentedControl<string>('Colour the bars with the accent', COLOURINGS, (value) => host.send('theme_colouring', value));
  host.row(host.card, 'themeColouring', colouring.root);

  const accentPage = host.switchControl((on) => host.send('theme_accent_page', on));
  host.row(host.card, 'themeAccentPage', accentPage.root);

  const saturation = el('input', 'sp-range');
  saturation.type = 'range';
  saturation.min = String(MIN_SATURATION);
  saturation.max = String(MAX_SATURATION);
  saturation.step = '5';
  saturation.setAttribute('aria-label', 'Limit accent saturation');
  const saturationValue = el('output', 'sp-range-value');
  const saturationWrap = el('div', 'sp-range-wrap');
  saturationWrap.append(saturation, saturationValue);
  const paintSaturation = (): void => {
    const n = Number(saturation.value);
    saturationValue.textContent = percentText(n);
    saturation.style.setProperty('--sp-fill', `${((n - MIN_SATURATION) / (MAX_SATURATION - MIN_SATURATION)) * 100}%`);
    saturation.setAttribute('aria-valuetext', percentText(n));
  };
  saturation.addEventListener('input', paintSaturation);
  saturation.addEventListener('change', () => host.send('theme_saturation', Number(saturation.value)));
  host.row(host.card, 'themeSaturation', saturationWrap);

  const contrast = host.segmentedControl<string>('Colour contrast', CONTRASTS, (value) => host.send('theme_contrast', value));
  host.row(host.card, 'themeContrast', contrast.root);

  // Theme file: one row. Export and Import open Windows' own file dialogs; the page never names a path.
  const file = el('div', 'sp-reset sp-theme-file');
  const exportButton = textButton('Export…', () => host.action('themeexport'), { icon: 'save' });
  const importButton = textButton('Import…', () => host.action('themeimport'), { icon: 'download' });
  let confirming = false;
  const resetButton = textButton('Reset theme', () => {
    if (!confirming) { confirming = true; paintReset(); cancelButton.focus(); return; }
    confirming = false;
    paintReset();
    host.action('themereset');
  }, { icon: 'reset' });
  const cancelButton = textButton('Cancel', () => { confirming = false; paintReset(); resetButton.focus(); });
  const paintReset = (): void => {
    resetButton.lastChild!.textContent = confirming ? 'Reset all theme settings' : 'Reset theme';
    resetButton.classList.toggle('is-danger', confirming);
    cancelButton.hidden = !confirming;
  };
  paintReset();
  file.append(exportButton, importButton, cancelButton, resetButton);
  host.row(host.card, 'themeFile', file);

  return {
    apply(values, readable) {
      latest = values;
      unreadable = !readable;
      const disabled = unreadable;
      schedule.set(String(values.theme_schedule));
      schedule.disable(disabled);
      scheduleNoteText.textContent = scheduleNote(values);
      look.set(String(values.theme_look));
      look.disable(disabled || !lookApplies(values));
      for (const [key, input] of timeInputs) {
        input.value = String(values[key]);
        input.disabled = disabled || !timesApply(values);
      }
      for (const [key, parts] of colourInputs) {
        const value = String(values[key]);
        parts.input.value = value !== '' ? value : '#808080';
        parts.input.disabled = disabled;
        parts.value.textContent = value !== '' ? value : 'Default';
        parts.reset.disabled = disabled || value === '';
      }
      colouring.set(String(values.theme_colouring));
      colouring.disable(disabled);
      accentPage.set(values.theme_accent_page === true);
      accentPage.disable(disabled);
      saturation.value = String(values.theme_saturation);
      saturation.disabled = disabled;
      paintSaturation();
      contrast.set(String(values.theme_contrast));
      contrast.disable(disabled);
      exportButton.disabled = disabled;
      importButton.disabled = disabled;
      resetButton.disabled = disabled;
      cancelButton.disabled = disabled;
    },
  };
}
